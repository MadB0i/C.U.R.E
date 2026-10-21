//! Safety classification: the decision that stands between "we found a big
//! file" and "we offer to delete it".
//!
//! Two rules govern everything here:
//!
//! 1. **Provenance is a floor, not a verdict.** A path in a temp folder is
//!    never below `SafeToReview` because of the folder alone — but a temp
//!    folder can absolutely be `Protected` (`C:\Windows\Temp` under a broken
//!    ACL, say) or `Unknown`.
//! 2. **Uncertainty escalates.** When the engine cannot place a path, the
//!    answer is `Unknown`, which is not deletable. A false "safe" is far more
//!    expensive than a false "unknown".
//!
//! Every decision returns the clauses that produced it, so the UI can show
//! the reasoning next to the number instead of asking for trust.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::types::{SafetyTier, StorageClass};

/// Environment-derived locations used by the classifier.
///
/// Injectable so tests can classify synthetic trees without touching the
/// real machine's `Program Files`.
#[derive(Debug, Clone)]
pub struct TierContext {
    pub windows_dir: PathBuf,
    pub program_files: Vec<PathBuf>,
    pub program_data: Vec<PathBuf>,
    pub system_volume: Vec<PathBuf>,
}

impl TierContext {
    /// Resolve the real context from the environment, falling back to
    /// conventional locations when a variable is missing.
    pub fn from_env() -> TierContext {
        let mut windows_dir = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        if windows_dir.as_os_str().is_empty() {
            windows_dir = PathBuf::from(r"C:\Windows");
        }

        let mut program_files = Vec::new();
        for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(v) = std::env::var_os(key) {
                let p = PathBuf::from(v);
                if !p.as_os_str().is_empty() && !program_files.contains(&p) {
                    program_files.push(p);
                }
            }
        }
        if program_files.is_empty() {
            program_files.push(
                windows_dir
                    .parent()
                    .unwrap_or(Path::new(r"C:\"))
                    .join("Program Files"),
            );
        }

        let system_root = windows_dir
            .parent()
            .unwrap_or(Path::new(r"C:\"))
            .to_path_buf();
        let program_data = vec![system_root.join("ProgramData")];
        let system_volume = vec![
            system_root.join("System Volume Information"),
            system_root.join("Recovery"),
            system_root.join("EFI"),
            system_root.join("Boot"),
        ];

        TierContext {
            windows_dir,
            program_files,
            program_data,
            system_volume,
        }
    }

    /// A context whose roots are all under `base`, for tests.
    pub fn synthetic(base: &Path) -> TierContext {
        TierContext {
            windows_dir: base.join("Windows"),
            program_files: vec![base.join("Program Files"), base.join("Program Files (x86)")],
            program_data: vec![base.join("ProgramData")],
            system_volume: vec![
                base.join("System Volume Information"),
                base.join("Recovery"),
                base.join("EFI"),
            ],
        }
    }
}

/// Windows subtrees that are structurally required and are never candidates
/// regardless of how stale they look.
///
/// Matched component-wise against a normalized path so `C:\Windows.old\...`
/// and `C:\mywindows\system32` cannot match by accident.
const PROTECTED_WINDOWS_TOKENS: &[&str] = &[
    "windows/winsxs",
    "windows/installer",
    "windows/syswow64",
    "windows/system32/config",
    "windows/system32/drivers",
    "windows/driverstore",
    "windows/assembly",
    "windows/servicing",
    "windows/boot",
    "windows/recovery",
    "windows/systemapps",
    "windows/explorer.exe",
    "windows/microsoft.net",
    "windows/winsxs.old",
];

/// Top-level directories that hold system or shared state.
const PROTECTED_ROOT_TOKENS: &[&str] = &[
    "system volume information",
    "$recycle.bin",
    "recovery",
    "efi",
    "boot",
    "config/msiserver",
];

/// Whole-volume files that must never be touched, by exact file name.
const PROTECTED_FILE_NAMES: &[&str] = &[
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
    "dumpstack.log.tmp",
    "$mft",
    "$bitmap",
    "$logfile",
];

/// The tier a recognised cleanable Windows subtree may take.
///
/// `None` means the path is not one of the areas Windows itself recreates, so
/// everything else under `C:\Windows` stays protected.
///
/// `WindowsUpdateCache` is deliberately *not* bulk-eligible. Windows Update
/// owns that cache and expects its services to be stopped before it is
/// cleared; a bulk recommendation that ignores that breaks patching. It is
/// still removable one item at a time, and the UI points at the supported
/// mechanism instead.
pub fn windows_class_tier(class: StorageClass) -> Option<SafetyTier> {
    match class {
        StorageClass::WindowsTemp => Some(SafetyTier::SafeToReview),
        StorageClass::CrashDumps => Some(SafetyTier::SafeToReview),
        StorageClass::DiagnosticLogs => Some(SafetyTier::SafeToReview),
        StorageClass::WindowsUpdateCache => Some(SafetyTier::ReviewCarefully),
        _ => None,
    }
}

/// True for classes the catalogue only ever resolves to a directory that
/// Windows itself recreates.
pub fn is_cleanable_windows_class(class: StorageClass) -> bool {
    windows_class_tier(class).is_some()
}

/// The classes whose files are regenerable *in principle*.
///
/// A file in one of these places is only a leftover if nothing is using it
/// right now. The class says where the file is; the guard below says
/// whether it is safe to call it a leftover.
pub fn is_regenerable_class(class: StorageClass) -> bool {
    matches!(
        class,
        StorageClass::UserTemp
            | StorageClass::WindowsTemp
            | StorageClass::BrowserCache
            | StorageClass::CrashDumps
    )
}

/// The recency and in-use guard.
///
/// A temporary or cached file that was written moments ago is very
/// probably in use right now. The engine may still remove it if the
/// operator picks it, but it must never be *presented* as a regenerable
/// leftover: the bytes may come straight back, or the file may be held
/// open by a running program.
///
/// The guard is applied everywhere the tier is derived — the walk, the
/// plan preview and the execution re-check — so a file that was fresh when
/// it was measured cannot quietly be promoted back to "safe" later. The
/// earlier version applied it only during the walk, which meant the plan
/// and the execution re-derived a "safe to review" tier for a file the
/// walk had already downgraded.
pub fn apply_activity_guard(
    classification: &mut Classification,
    class: StorageClass,
    modified_unix: Option<u64>,
    now_unix: u64,
    recent_seconds: u64,
    in_use: bool,
) {
    if classification.tier != SafetyTier::SafeToReview {
        return;
    }
    if !is_regenerable_class(class) {
        return;
    }

    let mut fresh = false;
    if let Some(modified) = modified_unix {
        let age_seconds = now_unix.saturating_sub(modified);
        fresh = age_seconds < recent_seconds;
    }
    if fresh {
        classification.tier = SafetyTier::ReviewCarefully;
        classification
            .reasons
            .push("written moments ago, so something may still be using it".to_string());
        classification
            .reasons
            .push("regenerable in principle, but not while it is in use".to_string());
    }
    if in_use {
        classification.tier = SafetyTier::ReviewCarefully;
        classification
            .reasons
            .push("held open by another program right now, so it is still in use".to_string());
    }
}

/// Result of classifying one path.
#[derive(Debug, Clone)]
pub struct Classification {
    pub tier: SafetyTier,
    pub reasons: Vec<String>,
}
impl Classification {
    fn protected(reason: impl Into<String>, mut reasons: Vec<String>) -> Classification {
        reasons.push(reason.into());
        Classification {
            tier: SafetyTier::Protected,
            reasons,
        }
    }

    fn unknown(reason: impl Into<String>, mut reasons: Vec<String>) -> Classification {
        reasons.push(reason.into());
        Classification {
            tier: SafetyTier::Unknown,
            reasons,
        }
    }

    fn settled(tier: SafetyTier, mut reasons: Vec<String>) -> Classification {
        reasons.push(format!("classified as {}", tier.label().to_lowercase()));
        Classification { tier, reasons }
    }
}

/// Normalize to lowercase forward-slash form for component matching.
pub fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Component-wise token match: `token` must align to whole `/` segments.
pub fn has_token(normalized: &str, token: &str) -> bool {
    let parts: Vec<&str> = normalized.split('/').collect();
    let want: Vec<&str> = token.split('/').collect();
    if want.is_empty() || want.len() > parts.len() {
        return false;
    }
    parts.windows(want.len()).any(|w| w == want.as_slice())
}

/// True when `path` is `root` itself or lies beneath it.
///
/// Compares on components so `C:\Program Files` never authorizes
/// `C:\Program FilesX`.
pub fn is_under(path: &Path, root: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    let a = normalize(path);
    let b = normalize(root);
    if b.is_empty() {
        return false;
    }
    a == b || a.starts_with(&format!("{}/", b.trim_end_matches('/')))
}

/// Classify a path. `class` supplies the provenance floor.
///
/// The returned tier is always at least as cautious as
/// [`StorageClass::floor_tier`] — the floor can only be raised here.
pub fn classify(path: &Path, class: StorageClass, ctx: &TierContext) -> Classification {
    let normalized = normalize(path);
    let file_name = normalized.rsplit('/').next().unwrap_or("").to_string();
    let mut reasons: Vec<String> = Vec::new();

    // --- Hard stops, checked before anything else -----------------------
    // A name-based stop is intentionally first: no amount of "the folder says
    // temp" should unlock `pagefile.sys`.

    if PROTECTED_FILE_NAMES.contains(&file_name.as_str()) {
        return Classification::protected(
            format!("`{}` is a volume-level system file", file_name),
            reasons,
        );
    }

    for token in PROTECTED_ROOT_TOKENS {
        // The Recycle Bin is handled by its own rule below: the folder itself
        // and the shell's `$I` index records are untouchable, but the `$R`
        // payloads are exactly the bytes the Recycle Bin exists to hold.
        if *token == "$recycle.bin" {
            continue;
        }
        if has_token(&normalized, token) {
            return Classification::protected(
                format!("`{}` is an operating-system or shell directory", token),
                reasons,
            );
        }
    }

    if has_token(&normalized, "$recycle.bin") {
        let is_payload = file_name.starts_with("$r");
        if class == StorageClass::RecycleBin && is_payload {
            reasons.push("a deleted-item payload the shell itself already moved here".to_string());
            return Classification::settled(SafetyTier::SafeToReview, reasons);
        }
        return Classification::protected(
            "inside the Recycle Bin but not a deleted-item payload".to_string(),
            reasons,
        );
    }

    if ctx.system_volume.iter().any(|r| is_under(path, r)) {
        return Classification::protected(
            "inside a system volume, recovery or boot location".to_string(),
            reasons,
        );
    }

    for token in PROTECTED_WINDOWS_TOKENS {
        if has_token(&normalized, token) {
            return Classification::protected(
                format!("`{}` is required by Windows itself", token),
                reasons,
            );
        }
    }

    for root in &ctx.program_files {
        if is_under(path, root) {
            return Classification::protected(
                "inside an installed application directory".to_string(),
                reasons,
            );
        }
    }

    for root in &ctx.program_data {
        if is_under(path, root) {
            // ProgramData holds both app caches and shared machine state.
            // A named cache subtree is fine; the rest is not.
            if is_known_programdata_cache(&normalized) {
                reasons.push("a named application cache inside ProgramData".to_string());
                break;
            }
            return Classification::protected(
                "inside ProgramData, which holds shared machine state".to_string(),
                reasons,
            );
        }
    }

    if is_under(path, &ctx.windows_dir) {
        // `C:\Windows` is protected as a whole except for the specific
        // cleanable subtrees. The list is derived from the class so a new
        // cleanable category cannot be forgotten here — the earlier version
        // of this list omitted `DiagnosticLogs` and quietly downgraded
        // `C:\Windows\Logs` to protected.
        match windows_class_tier(class) {
            Some(SafetyTier::SafeToReview) => {
                return Classification::settled(SafetyTier::SafeToReview, reasons);
            }
            // A Windows-managed cache that may only be cleared one item at a
            // time falls through to the class rules below, which carry the
            // explanation and the supported alternative.
            Some(_) => {}
            None => {
                return Classification::protected(
                    "inside the Windows directory and not a recognised cleanable area".to_string(),
                    reasons,
                );
            }
        }
    }

    // --- Reparse points --------------------------------------------------
    // A link can point anywhere; a target reachable through one is outside
    // the root the user authorised. Never a candidate.
    // (Callers also refuse these at deletion time — this is the reporting
    // half of that same rule.)

    // --- Class-specific reasoning ---------------------------------------
    let floor = class.floor_tier();
    let tier = floor;

    match class {
        StorageClass::AbandonedAppData => {
            // Deliberately never promoted. Age is not evidence of
            // disuse, and this record exists to be read, not deleted.
            return Classification::unknown(
                "application data is never a deletion recommendation; age alone proves nothing"
                    .to_string(),
                reasons,
            );
        }
        StorageClass::OldDownload | StorageClass::OldInstaller => {
            reasons.push("user-visible files in a downloads folder".to_string());
        }
        StorageClass::DevCache => {
            reasons.push("developer build output — regenerated by the toolchain".to_string());
        }
        StorageClass::UserSelected => {
            reasons.push("the user chose this location; intent is not known".to_string());
        }
        StorageClass::WindowsUpdateCache => {
            // Windows repairs this itself, but only when its services are
            // stopped. Recommending a manual delete would break updates, so
            // this cache is never bulk-recommended and the supported
            // mechanism is named instead.
            reasons.push(
                "Windows Update manages this itself and expects its services to be stopped \
                 before it is cleared"
                    .to_string(),
            );
            reasons.push(
                "supported alternative: Settings > System > Storage > Temporary files, or \
                 DISM /Online /Cleanup-Image /StartComponentCleanup"
                    .to_string(),
            );
            return Classification::settled(SafetyTier::ReviewCarefully, reasons);
        }
        StorageClass::BrowserCache => {
            if !is_browser_cache_path(&normalized) {
                return Classification::unknown(
                    "inside a browser profile but not a recognised cache directory".to_string(),
                    reasons,
                );
            }
            reasons.push("a named cache directory inside a browser profile".to_string());
        }
        StorageClass::RecycleBin => {
            // Already settled above, where the `$R` name check lives.
            reasons.push("an item the shell already moved to the Recycle Bin".to_string());
        }
        StorageClass::WindowsOld => {
            reasons.push(
                "a previous Windows installation — removing it is an explicit Windows operation"
                    .to_string(),
            );
        }
        StorageClass::CrashDumps | StorageClass::DiagnosticLogs => {
            reasons.push("diagnostic output regenerated by the next failure".to_string());
        }
        StorageClass::Other => {
            return Classification::unknown(
                "no category could be established for this path".to_string(),
                reasons,
            );
        }
        _ => {
            reasons.push("in a location the operating system treats as temporary".to_string());
        }
    }

    if matches!(tier, SafetyTier::SafeToReview) {
        // Even in the safest class, say out loud that this is a class-level
        // judgement about a *folder*, not an inspection of this file.
        reasons.push(
            "regenerable by the program that wrote it; individual files are still listed \
             before anything is removed"
                .to_string(),
        );
    }

    Classification::settled(tier, reasons)
}

/// ProgramData subtrees that are unambiguously application caches.
const PROGRAMDATA_CACHE_TOKENS: &[&str] = &[
    "package cache",
    "microsoft/windows/wer",
    "microsoft/windows/search",
    "microsoft/windows/caches",
];

fn is_known_programdata_cache(normalized: &str) -> bool {
    PROGRAMDATA_CACHE_TOKENS
        .iter()
        .any(|t| has_token(normalized, t))
}

/// Browser cache directories. Anything else inside a browser profile —
/// cookies, history, bookmarks, passwords, local storage — is excluded on
/// purpose, because losing those is a real cost to the user and near-zero
/// benefit to this engine.
/// benefit to this engine.
///
/// These are the *directory* names, not marketing names: Brave installs to
/// `BraveSoftware\Brave-Browser\User Data`, so a token written as
/// `brave-software` matches nothing at all. The catalogue builds its vendor
/// list from this same constant so the two cannot drift apart.
pub const BROWSER_VENDOR_ROOTS: &[&str] = &[
    "google/chrome/user data",
    "microsoft/edge/user data",
    "mozilla/firefox/profiles",
    "bravesoftware/brave-browser/user data",
    "chromium/user data",
    "opera software",
    "vivaldi/user data",
];

/// Path components that must never be treated as cache even when they sit
/// inside a browser profile.
///
/// These are matched against the path *below* the vendor root. Matching the
/// whole path would exclude Firefox entirely, because its vendor root is
/// `mozilla/firefox/profiles` — the word `profiles` appears in the root and
/// every path underneath it.
const BROWSER_NON_CACHE_TOKENS: &[&str] = &[
    "cookies",
    "history",
    "bookmarks",
    "login data",
    "passwords",
    "local storage",
    "session storage",
    "web data",
    "logins.json",
    "key3.db",
    "key4.db",
    "places.sqlite",
    "favicons",
    "extensions",
    "profiles",
];

/// True when the normalized path is a recognised cache directory.
///
/// Conservative by construction: an unrecognised path inside a browser
/// profile is `Unknown`, not `SafeToReview`.
/// Directory names inside a browser profile that hold regenerable cache.
///
/// Anything not on this list inside a profile — cookies, history,
/// bookmarks, passwords, local storage, extensions — is excluded on purpose:
/// losing those is a real cost to the user and near-zero benefit here.
pub const BROWSER_CACHE_DIR_NAMES: &[&str] = &[
    "cache",
    "cache2",
    "cache_data",
    "cachestorage",
    "code cache",
    "gpucache",
    "shadercache",
    "grshadercache",
    "dawncache",
    "graphitedawncache",
    "optimization_guide_hint_cache_store",
];

/// The path components that lie below the browser's vendor root.
fn below_vendor_root(normalized: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = normalized.split('/').collect();
    for token in BROWSER_VENDOR_ROOTS {
        let want: Vec<&str> = token.split('/').collect();
        if want.len() > parts.len() {
            continue;
        }
        for start in 0..=(parts.len() - want.len()) {
            if parts[start..start + want.len()] == want[..] {
                return Some(parts[start + want.len()..].to_vec());
            }
        }
    }
    None
}

pub fn is_browser_cache_path(normalized: &str) -> bool {
    let Some(below) = below_vendor_root(normalized) else {
        return false;
    };
    // Exclusions are checked against the path *below* the vendor root, so a
    // vendor whose own name contains an excluded word — Firefox's
    // `mozilla/firefox/profiles` — is still recognised.
    if BROWSER_NON_CACHE_TOKENS.iter().any(|t| below.contains(t)) {
        return false;
    }
    below.iter().any(|c| BROWSER_CACHE_DIR_NAMES.contains(c))
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Extensions that indicate an installable package.
pub const INSTALLER_EXTENSIONS: &[&str] = &[
    "exe",
    "msi",
    "msix",
    "appx",
    "cab",
    "iso",
    "zip",
    "7z",
    "rar",
    "msu",
    "appinstaller",
];

pub fn is_installer_extension(ext: &str) -> bool {
    INSTALLER_EXTENSIONS.contains(&ext)
}

/// Extensions of diagnostic output that is regenerated after any crash.
///
/// A file with one of these extensions inside a diagnostic location is
/// diagnostic output, whatever the folder it was found in.
pub const DIAGNOSTIC_EXTENSIONS: &[&str] = &[
    "dmp", "mdmp", "hdmp", "etl", "evtx", "log", "wer", "tmp", "old", "bak",
];

/// Developer cache markers: directory names whose contents are reproducible.
pub const DEV_CACHE_DIR_NAMES: &[&str] = &[
    "node_modules",
    ".nuget",
    ".gradle",
    ".m2",
    ".cargo",
    "target",
    "build",
    ".next",
    ".cache",
    "obj",
    "bin",
    ".terraform",
    ".ivy2",
    ".stack-work",
    ".dart_tool",
    "deriveddata",
    ".pytest_cache",
    "__pycache__",
    ".parcel-cache",
    ".turbo",
    ".svelte-kit",
    ".angular",
    ".expo",
    ".dart_tool",
    "deriveddata",
    ".pytest_cache",
    "__pycache__",
    ".terraform",
    ".ivy2",
    ".stack-work",
    "obj",
    "bin",
    "dist",
];

/// True when the path sits inside a recognised developer build directory.
///
/// The check is component-wise, so a user folder merely *named* `target`
/// matches but `my-target` does not. The name is matched wherever it
/// appears: a project on `D:\\Projects` is exactly as much a build
/// directory as one under the user's profile.
pub fn is_dev_cache_path(path: &Path) -> bool {
    let normalized = normalize(path);
    normalized
        .split('/')
        .any(|c| DEV_CACHE_DIR_NAMES.contains(&c))
}

pub fn extension(path: &Path) -> String {
    extension_of(path)
}

/// Sorted, de-duplicated set of paths, used when reporting skipped roots.
pub fn dedupe_sorted(paths: Vec<PathBuf>) -> BTreeSet<PathBuf> {
    paths.into_iter().collect()
}

/* ---------- per-file refinement ------------------------------------------ */

/// True when the path lies inside a folder the platform calls Downloads.
pub fn is_downloads_path(path: &Path) -> bool {
    let normalized = normalize(path);
    normalized
        .split('/')
        .any(|c| c == "downloads" || c == "download")
}

/// Refine a class using the file itself.
///
/// The catalogue says where a root *is*; this says what a particular file
/// inside it *is*: a build directory wherever it lives, an installer, or a
/// download the operator has not touched in a long time. The refined class
/// then drives the tier, so a build directory under `D:\\Projects` is
/// recognised the same way as one under the profile.
pub fn refine_class(
    path: &Path,
    class: StorageClass,
    modified_unix: Option<u64>,
    now_unix: u64,
    idle_days: u64,
) -> StorageClass {
    // A class the catalogue is confident about is left alone. A browser
    // cache is a browser cache even if it happens to sit in a folder called
    // `build`.
    match class {
        StorageClass::UserTemp
        | StorageClass::WindowsTemp
        | StorageClass::BrowserCache
        | StorageClass::RecycleBin
        | StorageClass::WindowsOld
        | StorageClass::WindowsUpdateCache
        | StorageClass::CrashDumps
        | StorageClass::DiagnosticLogs => return class,
        _ => {}
    }

    if is_dev_cache_path(path) {
        return StorageClass::DevCache;
    }

    if (class == StorageClass::DiagnosticLogs || class == StorageClass::CrashDumps)
        && DIAGNOSTIC_EXTENSIONS.contains(&extension_of(path).as_str())
    {
        return StorageClass::CrashDumps;
    }

    if class == StorageClass::UserSelected && is_downloads_path(path) {
        if is_installer_extension(&extension_of(path)) {
            return StorageClass::OldInstaller;
        }
        // Age is only used to *label* a download, never to call it unused.
        // A file that has not been modified for a long time is evidence
        // about its age, and nothing more.
        if let Some(modified) = modified_unix {
            let age_days = now_unix.saturating_sub(modified) / 86_400;
            if age_days >= idle_days {
                return StorageClass::OldDownload;
            }
        }
    }
    class
}
