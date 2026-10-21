//! The catalogue of locations the engine knows how to measure.
//!
//! Every entry is resolved from the environment at scan time and reported as
//! a [`ScanRoot`] with its real on-disk existence checked. A root that does
//! not exist is reported as *not resolved* rather than silently omitted, so
//! the coverage panel can distinguish "nothing there" from "never looked".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::types::{ScanRoot, StorageClass};
use super::walk::WalkLimits;

/// How deep a catalogue scan is willing to enumerate on its own.
///
/// `quick` walks only the known-safe caches. `deep` additionally enumerates
/// profile and application-data directories, which costs far more time and
/// returns much noisier results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDepth {
    Quick,
    Deep,
}

impl ScanDepth {
    pub fn key(self) -> &'static str {
        match self {
            ScanDepth::Quick => "quick",
            ScanDepth::Deep => "deep",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ScanDepth::Quick => "Quick scan",
            ScanDepth::Deep => "Deep inventory",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            ScanDepth::Quick => {
                "Known cache, temporary and diagnostic locations only. Fast, low risk of noise."
            }
            ScanDepth::Deep => {
                "Also inventories browser profiles, application data and developer caches. \
                 Slower, and reports far more that will not be recommended for removal."
            }
        }
    }

    pub fn parse(s: &str) -> ScanDepth {
        if s.eq_ignore_ascii_case("deep") || s.eq_ignore_ascii_case("full") {
            ScanDepth::Deep
        } else {
            ScanDepth::Quick
        }
    }
}

/// Build the root list for a scan.
///
/// `extra` holds drives and folders the user selected explicitly; they are
/// always included and never silently dropped.
pub fn roots(depth: ScanDepth, extra: &[PathBuf]) -> Vec<ScanRoot> {
    roots_with_report(depth, extra).0
}

/// Same as [`roots`], also reporting how many locations were dropped because
/// they sat inside another root already being walked.
pub fn roots_with_report(depth: ScanDepth, extra: &[PathBuf]) -> (Vec<ScanRoot>, usize) {
    let mut roots: Vec<ScanRoot> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    let windows_dir = windows_dir();
    let system_root = windows_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(r"C:\"));
    // `Option` would force an `if let` at every use below; an empty path is a
    // clearer "this machine does not publish this location" and joins safely
    // into a path that then fails its existence check.
    let local = env_path("LOCALAPPDATA").unwrap_or_default();
    let roaming = env_path("APPDATA").unwrap_or_default();
    let profile = env_path("USERPROFILE").unwrap_or_default();
    let program_data = env_path("ProgramData").unwrap_or_default();

    let push = |roots: &mut Vec<ScanRoot>,
                seen: &mut BTreeSet<String>,
                path: PathBuf,
                class: StorageClass,
                label: &str| {
        if path.as_os_str().is_empty() {
            return;
        }
        let key = super::tiers::normalize(&path);
        if key.is_empty() || seen.contains(&key) {
            return;
        }
        seen.insert(key);
        roots.push(ScanRoot {
            path,
            class,
            is_catalogue: true,
            label: label.to_string(),
        });
    };

    // ---- user temp -------------------------------------------------------
    if let Some(t) = env_path("TEMP").or_else(|| env_path("TMP")) {
        push(
            &mut roots,
            &mut seen,
            t,
            StorageClass::UserTemp,
            "User temp",
        );
    }
    // Windows keeps a short-path twin here on many profiles; walking it twice
    // would double every number.
    push(
        &mut roots,
        &mut seen,
        local.join("Temp"),
        StorageClass::UserTemp,
        "User temp",
    );

    push(
        &mut roots,
        &mut seen,
        windows_dir.join("Temp"),
        StorageClass::WindowsTemp,
        "Windows temp",
    );

    // ---- browser caches --------------------------------------------------
    for vendor in browser_vendors() {
        let vendor_root = local.join(&vendor);
        for dir in browser_cache_dirs(&vendor_root) {
            push(
                &mut roots,
                &mut seen,
                dir,
                StorageClass::BrowserCache,
                &format!("{} cache", vendor_leaf(&vendor)),
            );
        }
    }
    // Firefox keeps its profile cache under AppData\Roaming.
    let firefox = roaming.join("Mozilla").join("Firefox").join("Profiles");
    for dir in child_dirs_named(&firefox, super::tiers::BROWSER_CACHE_DIR_NAMES) {
        push(
            &mut roots,
            &mut seen,
            dir,
            StorageClass::BrowserCache,
            "Firefox cache",
        );
    }

    // ---- Recycle Bin -----------------------------------------------------
    // One root per per-user SID directory. The engine reads only `$R*`
    // payloads; `$I*` metadata and the shell's own bookkeeping are left alone
    // and reported as protected.
    let recycle = system_root.join("$Recycle.Bin");
    if let Ok(readers) = std::fs::read_dir(&recycle) {
        for entry in readers.flatten() {
            let path = entry.path();
            if path.is_dir() && !fsx_is_link(&path) {
                push(
                    &mut roots,
                    &mut seen,
                    path,
                    StorageClass::RecycleBin,
                    "Recycle Bin",
                );
            }
        }
    }

    // ---- previous Windows install ---------------------------------------
    push(
        &mut roots,
        &mut seen,
        system_root.join("Windows.old"),
        StorageClass::WindowsOld,
        "Previous Windows install",
    );

    // ---- Windows Update cache -------------------------------------------
    // Reported, never recommended: clearing it out from under the update
    // service breaks patching.
    push(
        &mut roots,
        &mut seen,
        windows_dir.join("SoftwareDistribution").join("Download"),
        StorageClass::WindowsUpdateCache,
        "Windows Update cache",
    );

    // ---- crash dumps and diagnostics ------------------------------------
    for p in [
        local.join("CrashDumps"),
        windows_dir.join("Minidump"),
        windows_dir.join("LiveKernelReports"),
    ] {
        push(
            &mut roots,
            &mut seen,
            p,
            StorageClass::CrashDumps,
            "Crash dumps",
        );
    }
    if let Some(pd) = Some(&program_data).filter(|p| !p.as_os_str().is_empty()) {
        for p in [
            pd.join("Microsoft")
                .join("Windows")
                .join("WER")
                .join("ReportArchive"),
            pd.join("Microsoft")
                .join("Windows")
                .join("WER")
                .join("ReportQueue"),
        ] {
            push(
                &mut roots,
                &mut seen,
                p,
                StorageClass::CrashDumps,
                "Crash reports",
            );
        }
    }
    for p in [
        windows_dir.join("Logs"),
        windows_dir.join("Panther"),
        windows_dir.join("Debug"),
    ] {
        push(
            &mut roots,
            &mut seen,
            p,
            StorageClass::DiagnosticLogs,
            "Diagnostic logs",
        );
    }
    if !profile.as_os_str().is_empty() {
        push(
            &mut roots,
            &mut seen,
            profile
                .join("AppData")
                .join("Local")
                .join("Microsoft")
                .join("Windows")
                .join("INetCache"),
            StorageClass::DiagnosticLogs,
            "Internet cache",
        );
    }

    // ---- downloads -------------------------------------------------------
    // Measured here so old installers and downloads appear in the inventory,
    // but classified per-file by the caller: an `.msi` and a `.pdf` in the
    // same folder are not the same proposition.
    push(
        &mut roots,
        &mut seen,
        profile.join("Downloads"),
        StorageClass::UserSelected,
        "Downloads",
    );

    // ---- developer caches (deep only) -----------------------------------
    if depth == ScanDepth::Deep {
        if !profile.as_os_str().is_empty() {
            for name in super::tiers::DEV_CACHE_DIR_NAMES {
                let p = profile.join(name);
                if p.is_dir() {
                    push(
                        &mut roots,
                        &mut seen,
                        p,
                        StorageClass::DevCache,
                        &format!("Developer cache ({name})"),
                    );
                }
            }
            // Repository-shaped build output, one level down.
            if let Ok(readers) = std::fs::read_dir(&profile) {
                for entry in readers.flatten() {
                    let repo = entry.path();
                    if !repo.is_dir() || fsx_is_link(&repo) {
                        continue;
                    }
                    for name in ["node_modules", "target", "bin", "obj", ".next", "build"] {
                        let p = repo.join(name);
                        if p.is_dir() {
                            push(
                                &mut roots,
                                &mut seen,
                                p,
                                StorageClass::DevCache,
                                &format!("Developer cache ({name})"),
                            );
                        }
                    }
                }
            }
        }
        // Application data, for the abandoned-app record only.
        for base in [&local, &roaming] {
            if base.as_os_str().is_empty() {
                continue;
            }
            let Ok(readers) = std::fs::read_dir(base) else {
                continue;
            };
            for entry in readers.flatten() {
                let p = entry.path();
                if p.is_dir() && !fsx_is_link(&p) {
                    push(
                        &mut roots,
                        &mut seen,
                        p,
                        StorageClass::AbandonedAppData,
                        "Application data",
                    );
                }
            }
        }
    }

    // ---- user-selected roots --------------------------------------------
    for path in extra {
        if path.as_os_str().is_empty() {
            continue;
        }
        let key = super::tiers::normalize(path);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        roots.push(ScanRoot {
            path: path.clone(),
            class: StorageClass::UserSelected,
            is_catalogue: false,
            label: volume_or_folder_label(path),
        });
    }

    // ---- reconcile overlapping roots --------------------------------------
    // A root nested inside another root would be walked twice, inflating both
    // the file count and every byte total derived from it. This is reachable
    // in normal use: adding `C:\Users\bob\AppData\Local` as an extra location
    // overlaps the catalogue's `%LOCALAPPDATA%` entries.
    let dropped = drop_nested_roots(&mut roots);

    (roots, dropped)
}

/// Remove roots that sit inside another root in the same list.
///
/// Returns how many were dropped so the scan can say so rather than silently
/// producing a smaller total than the operator asked for.
pub fn drop_nested_roots(roots: &mut Vec<ScanRoot>) -> usize {
    let mut keep: Vec<ScanRoot> = Vec::new();
    let mut dropped = 0usize;
    for candidate in roots.drain(..) {
        let nested = keep
            .iter()
            .any(|k| k.path != candidate.path && super::tiers::is_under(&candidate.path, &k.path));
        if nested {
            dropped += 1;
        } else {
            keep.push(candidate);
        }
    }
    *roots = keep;
    dropped
}

/// Browser vendor roots relative to `%LOCALAPPDATA%`, built from the single
/// shared list so the catalogue and the classifier can never disagree about
/// which directories count as a browser profile.
fn browser_vendors() -> Vec<String> {
    super::tiers::BROWSER_VENDOR_ROOTS
        .iter()
        .map(|root| root.replace('/', "\\"))
        .collect()
}

fn vendor_leaf(vendor: &str) -> &str {
    vendor.split('\\').next().unwrap_or(vendor)
}

/// Enumerate cache directories inside a browser's user-data root.
fn browser_cache_dirs(vendor_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(readers) = std::fs::read_dir(vendor_root) else {
        return out;
    };
    for entry in readers.flatten() {
        let profile = entry.path();
        if !profile.is_dir() || fsx_is_link(&profile) {
            continue;
        }
        for dir in child_dirs_named(&profile, super::tiers::BROWSER_CACHE_DIR_NAMES) {
            out.push(dir);
        }
    }
    out.sort();
    out
}

fn child_dirs_named(parent: &Path, names: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(readers) = std::fs::read_dir(parent) else {
        return out;
    };
    for entry in readers.flatten() {
        let path = entry.path();
        if !path.is_dir() || fsx_is_link(&path) {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if names.contains(&name.as_str()) {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn fsx_is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|md| super::fsx::is_reparse_point(&md))
        .unwrap_or(true)
}

fn windows_dir() -> PathBuf {
    env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Short, human label for a user-selected root.
fn volume_or_folder_label(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    if s.len() <= 3 {
        return format!("Drive {}", s.trim_end_matches('\\'));
    }
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| s)
}

/// Limits appropriate to the chosen depth.
pub fn limits_for(depth: ScanDepth) -> WalkLimits {
    match depth {
        ScanDepth::Quick => WalkLimits {
            max_depth: 48,
            max_entries: 1_500_000,
            max_duration_ms: 90_000,
            min_item_size: 0,
            idle_days: 30,
            recent_file_seconds: 3600,
        },
        ScanDepth::Deep => WalkLimits {
            max_depth: 64,
            max_entries: 6_000_000,
            max_duration_ms: 240_000,
            min_item_size: 0,
            idle_days: 30,
            recent_file_seconds: 3600,
        },
    }
}
