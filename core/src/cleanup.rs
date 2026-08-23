//! Disk-cleanup scanning and direct deletion for regenerable junk.
//!
//! Scanning and deleting are deliberately separate steps (same
//! "show findings, then act" split as persistence scanning): every
//! `scan_*` function only reports [`CleanupCandidate`]s, and nothing is
//! removed until [`delete_candidates`] is called explicitly.
//!
//! These are DIRECT deletes, never quarantine moves: the targets are
//! regenerable caches/junk, not potentially-malicious binaries or user
//! documents. Cookies, history, bookmarks, passwords and anything outside
//! the enumerated roots are out of scope and never touched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/// One reclaimable item found by a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupCandidate {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub category: CleanupCategory,
}

/// What kind of junk a candidate is. `DownloadsInstaller` is flagged
/// distinctly because those files may still be wanted by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupCategory {
    Temp,
    BrowserCache,
    RecycleBin,
    WindowsOld,
    DownloadsInstaller,
}

impl CleanupCategory {
    /// Stable string key used by the CLI/GUI to select categories.
    pub fn key(self) -> &'static str {
        match self {
            CleanupCategory::Temp => "temp",
            CleanupCategory::BrowserCache => "browser_cache",
            CleanupCategory::RecycleBin => "recycle_bin",
            CleanupCategory::WindowsOld => "windows_old",
            CleanupCategory::DownloadsInstaller => "downloads_installer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CleanupCategory::Temp => "Temp files",
            CleanupCategory::BrowserCache => "Browser caches",
            CleanupCategory::RecycleBin => "Recycle Bin",
            CleanupCategory::WindowsOld => "Windows.old",
            CleanupCategory::DownloadsInstaller => "Old installers in Downloads",
        }
    }

    pub const ALL: [CleanupCategory; 5] = [
        CleanupCategory::Temp,
        CleanupCategory::BrowserCache,
        CleanupCategory::RecycleBin,
        CleanupCategory::WindowsOld,
        CleanupCategory::DownloadsInstaller,
    ];
}

/// One file that could not be deleted, with the OS reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupFailure {
    pub path: PathBuf,
    pub reason: String,
}

/// Outcome of [`delete_candidates`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupResult {
    pub attempted: usize,
    pub deleted: usize,
    pub failed: usize,
    pub bytes_freed: u64,
    pub failures: Vec<CleanupFailure>,
}

/// Per-category aggregate for showing a human a real summary up front.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySummary {
    pub category: CleanupCategory,
    pub item_count: usize,
    pub total_bytes: u64,
}

// ---------------------------------------------------------------------------
// generic walkers
// ---------------------------------------------------------------------------

/// Sum of all regular-file sizes anywhere under `dir`. Unreadable entries
/// simply contribute nothing instead of failing the whole scan.
pub fn dir_size(dir: &Path) -> u64 {
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.metadata().ok())
        .filter(|md| md.is_file())
        .map(|md| md.len())
        .sum()
}

/// All regular files under `dir` (any depth) with their sizes. Directories
/// themselves are not returned. Locked/unreadable entries are skipped rather
/// than aborting the walk.
fn collect_files_under(dir: &Path) -> Vec<(PathBuf, u64)> {
    WalkDir::new(dir)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let size = entry.metadata().ok()?.len();
            Some((entry.into_path(), size))
        })
        .collect()
}

fn push_tree(candidates: &mut Vec<CleanupCandidate>, path: PathBuf, category: CleanupCategory) {
    let size_bytes = if path.is_dir() {
        dir_size(&path)
    } else {
        fs::metadata(&path).map(|md| md.len()).unwrap_or(0)
    };
    candidates.push(CleanupCandidate {
        path,
        size_bytes,
        category,
    });
}

// ---------------------------------------------------------------------------
// 1. temp files (%TEMP% + C:\Windows\Temp)
// ---------------------------------------------------------------------------

pub fn scan_temp_files() -> Vec<CleanupCandidate> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(t) = std::env::var_os("TEMP").or_else(|| std::env::var_os("TMP")) {
        roots.push(PathBuf::from(t));
    }
    roots.push(system_drive_root().join("Windows").join("Temp"));
    scan_temp_files_in(&roots)
}

/// Files living directly or nested inside any of `roots`; the root folders
/// themselves are never candidates. Currently-locked files cannot be detected
/// reliably during a scan - they stay listed here and surface later as
/// graceful per-item failures in [`delete_candidates`].
pub fn scan_temp_files_in(roots: &[PathBuf]) -> Vec<CleanupCandidate> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut candidates = Vec::new();
    for root in roots {
        if !root.is_dir() || seen.iter().any(|s| same_dir(s, root)) {
            continue;
        }
        seen.push(root.clone());
        for (path, size) in collect_files_under(root) {
            candidates.push(CleanupCandidate {
                path,
                size_bytes: size,
                category: CleanupCategory::Temp,
            });
        }
    }
    candidates
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Root of the Windows system drive (`"C:\"`), read from `SystemDrive`.
fn system_drive_root() -> PathBuf {
    match std::env::var("SystemDrive") {
        Ok(drive) if drive.len() == 2 && drive.ends_with(':') => {
            PathBuf::from(format!("{drive}\\"))
        }
        _ => PathBuf::from("C:\\"),
    }
}

// ---------------------------------------------------------------------------
// 2. browser caches (Chrome / Edge, per-profile Cache dirs ONLY)
// ---------------------------------------------------------------------------

const BROWSER_VENDORS: [&str; 2] = [r"Google\Chrome", r"Microsoft\Edge"];

pub fn scan_browser_cache() -> Vec<CleanupCandidate> {
    match std::env::var_os("LOCALAPPDATA") {
        Some(base) => scan_browser_cache_in(&PathBuf::from(base)),
        None => Vec::new(),
    }
}

/// Under `<localappdata>\<vendor>\User Data\<profile>\Cache` only. Cookies,
/// history, bookmarks, passwords, Local Storage etc. are intentionally not
/// matched - they are user data.
pub fn scan_browser_cache_in(localappdata: &Path) -> Vec<CleanupCandidate> {
    let mut candidates = Vec::new();
    for vendor in BROWSER_VENDORS {
        let user_data = localappdata.join(vendor).join("User Data");
        let Ok(entries) = fs::read_dir(&user_data) else {
            continue;
        };
        let mut profiles: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        profiles.sort();
        for profile in profiles {
            let cache = profile.join("Cache");
            if cache.is_dir() {
                push_tree(&mut candidates, cache, CleanupCategory::BrowserCache);
            }
        }
    }
    candidates
}

// ---------------------------------------------------------------------------
// 3. recycle bin (<system drive>\$Recycle.Bin\<SID>\$R*)
// ---------------------------------------------------------------------------

/// Each `$R...` entry is one deleted item's actual payload; the sibling `$I...`
/// files are metadata that Windows regenerates and are skipped.
pub fn scan_recycle_bin() -> Vec<CleanupCandidate> {
    let root = system_drive_root().join("$Recycle.Bin");
    scan_recycle_bin_in(&root)
}

pub fn scan_recycle_bin_in(recycle_root: &Path) -> Vec<CleanupCandidate> {
    let mut candidates = Vec::new();
    let Ok(sids) = fs::read_dir(recycle_root) else {
        return candidates;
    };
    for sid in sids.filter_map(|e| e.ok()).map(|e| e.path()) {
        let Ok(items) = fs::read_dir(&sid) else {
            continue;
        };
        let mut items: Vec<PathBuf> = items
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("$R"))
                    .unwrap_or(false)
            })
            .collect();
        items.sort();
        for item in items {
            push_tree(&mut candidates, item, CleanupCategory::RecycleBin);
        }
    }
    candidates
}

// ---------------------------------------------------------------------------
// 4. C:\Windows.old
// ---------------------------------------------------------------------------

pub fn scan_windows_old() -> Vec<CleanupCandidate> {
    match scan_windows_old_at(&system_drive_root().join("Windows.old")) {
        Some(candidate) => vec![candidate],
        None => Vec::new(),
    }
}

/// Single coarse entry for the whole folder - never enumerated file-by-file.
pub fn scan_windows_old_at(path: &Path) -> Option<CleanupCandidate> {
    if !path.is_dir() {
        return None;
    }
    Some(CleanupCandidate {
        path: path.to_path_buf(),
        size_bytes: dir_size(path),
        category: CleanupCategory::WindowsOld,
    })
}

// ---------------------------------------------------------------------------
// 5. old installers sitting in Downloads
// ---------------------------------------------------------------------------

const INSTALLER_EXTENSIONS: [&str; 2] = ["exe", "msi"];

pub fn scan_old_downloads(older_than_days: u32) -> Vec<CleanupCandidate> {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    scan_old_downloads_in(&home.join("Downloads"), older_than_days, SystemTime::now())
}

/// `.exe`/`.msi` files in `dir` whose modification time is older than the
/// threshold. These MAY still be wanted, hence the distinct
/// [`CleanupCategory::DownloadsInstaller`] flag and mandatory explicit
/// per-file confirmation before deletion.
pub fn scan_old_downloads_in(
    dir: &Path,
    older_than_days: u32,
    now: SystemTime,
) -> Vec<CleanupCandidate> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let cutoff = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .saturating_sub(u64::from(older_than_days) * 86_400);
    let mut candidates: Vec<CleanupCandidate> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| INSTALLER_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
        })
        .filter_map(|p| {
            let md = fs::metadata(&p).ok()?;
            let mtime = md.modified().ok()?;
            let secs = mtime.duration_since(UNIX_EPOCH).ok()?.as_secs();
            if secs >= cutoff {
                return None;
            }
            Some(CleanupCandidate {
                path: p,
                size_bytes: md.len(),
                category: CleanupCategory::DownloadsInstaller,
            })
        })
        .collect();
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates
}

// ---------------------------------------------------------------------------
// deletion + aggregation
// ---------------------------------------------------------------------------

/// Deletes every candidate directly (no quarantine). Failures - files locked
/// by a running process, permission denied, vanished between scan and delete -
/// are collected per-item and never abort the batch.
pub fn delete_candidates(candidates: &[CleanupCandidate]) -> CleanupResult {
    let mut result = CleanupResult {
        attempted: candidates.len(),
        ..CleanupResult::default()
    };
    for candidate in candidates {
        match delete_path(&candidate.path) {
            Ok(()) => {
                result.deleted += 1;
                result.bytes_freed += candidate.size_bytes;
            }
            Err(err) => result.failures.push(CleanupFailure {
                path: candidate.path.clone(),
                reason: err.to_string(),
            }),
        }
    }
    result.failed = result.failures.len();
    result
}

fn delete_path(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn summarize(candidates: &[CleanupCandidate]) -> Vec<CategorySummary> {
    CleanupCategory::ALL
        .iter()
        .map(|&cat| {
            let matching: Vec<&CleanupCandidate> =
                candidates.iter().filter(|c| c.category == cat).collect();
            CategorySummary {
                category: cat,
                item_count: matching.len(),
                total_bytes: matching.iter().map(|c| c.size_bytes).sum(),
            }
        })
        .collect()
}

pub fn scan_all() -> Vec<CleanupCandidate> {
    let mut all = Vec::new();
    all.extend(scan_temp_files());
    all.extend(scan_browser_cache());
    all.extend(scan_recycle_bin());
    all.extend(scan_windows_old());
    all
}

/// Human-readable byte count ("1.9 GB"), shared by CLI and GUI summaries.
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

// ---------------------------------------------------------------------------
// DISM component-store cleanup (different in kind: shells out, no candidates)
// ---------------------------------------------------------------------------

pub fn dism_args() -> Vec<&'static str> {
    vec![
        "/Online",
        "/Cleanup-Image",
        "/StartComponentCleanup",
        "/NoRestart",
    ]
}

/// Runs `dism.exe /Online /Cleanup-Image /StartComponentCleanup`. Can take
/// several minutes; needs an elevated shell. Returns combined DISM output on
/// success or a descriptive error. Manually-verified only (see tests).
pub fn run_dism_cleanup() -> Result<String, String> {
    let output = std::process::Command::new("dism.exe")
        .args(dism_args())
        .output()
        .map_err(|err| format!("failed to start dism.exe: {err}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "dism.exe exited with {}:\n{}",
            output.status, text
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn write_file(path: &Path, size: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(path, vec![b'x'; size]).expect("write fixture file");
    }

    fn total(candidates: &[CleanupCandidate]) -> u64 {
        candidates.iter().map(|c| c.size_bytes).sum()
    }

    // ---- temp files -------------------------------------------------------

    #[test]
    fn temp_scan_lists_nested_files_but_not_the_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("temp-root");
        write_file(&root.join("a.log"), 100);
        write_file(&root.join("nested").join("deep").join("b.tmp"), 250);

        let found = scan_temp_files_in(&[root.clone()]);
        assert_eq!(found.len(), 2);
        assert!(found
            .iter()
            .all(|c| c.category == CleanupCategory::Temp && c.path != root));
        assert_eq!(total(&found), 350);
    }

    #[test]
    fn temp_scan_dedupes_equivalent_roots() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("t");
        write_file(&root.join("f.bin"), 10);
        let twin = tmp.path().join(".").join("t");

        let found = scan_temp_files_in(&[root.clone(), twin]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn temp_scan_skips_missing_roots() {
        let found = scan_temp_files_in(&[PathBuf::from("Z:\\definitely-not-here")]);
        assert!(found.is_empty());
    }

    // ---- browser cache ----------------------------------------------------

    #[test]
    fn browser_cache_matches_only_profile_cache_dirs() {
        let la = TempDir::new().unwrap();
        let chrome_ud = la.path().join("Google").join("Chrome").join("User Data");
        let edge_ud = la.path().join("Microsoft").join("Edge").join("User Data");
        write_file(&chrome_ud.join("Default").join("Cache").join("f_000001"), 500);
        write_file(
            &chrome_ud
                .join("Profile 2")
                .join("Cache")
                .join("Cache_Data")
                .join("index"),
            700,
        );
        write_file(&edge_ud.join("Default").join("Cache").join("e_000001"), 300);
        // user-data decoys that must never match:
        write_file(&chrome_ud.join("Default").join("Cookies"), 99);
        write_file(
            &chrome_ud.join("Default").join("Local Storage").join("leveldb"),
            99,
        );
        write_file(&chrome_ud.join("Cache").join("not-a-profile"), 99);

        let mut found = scan_browser_cache_in(la.path());
        found.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(found.len(), 3, "got: {found:?}");
        assert_eq!(total(&found), 1500);
        assert!(found.iter().all(|c| c.category == CleanupCategory::BrowserCache));
        assert!(found.iter().all(|c| c.path.file_name().unwrap() == "Cache"));
    }

    #[test]
    fn browser_cache_scan_is_empty_without_browsers() {
        let la = TempDir::new().unwrap();
        assert!(scan_browser_cache_in(la.path()).is_empty());
    }

    // ---- recycle bin ------------------------------------------------------

    #[test]
    fn recycle_bin_scans_r_items_and_skips_i_metadata() {
        let root = TempDir::new().unwrap();
        let sid = root.path().join("$Recycle.Bin").join("S-1-5-21-1001");
        write_file(&sid.join("$R000001.txt"), 4096);
        write_file(&sid.join("$RDELETED.dir").join("payload.dll"), 512);
        write_file(&sid.join("$I000001.meta"), 24); // metadata, must be skipped
        let other_sid = root.path().join("$Recycle.Bin").join("S-1-5-18");
        write_file(&other_sid.join("$RABC.exe"), 16);

        let found = scan_recycle_bin_in(&root.path().join("$Recycle.Bin"));
        assert_eq!(found.len(), 3, "got: {found:?}");
        assert_eq!(total(&found), 4096 + 512 + 16);
        assert!(found.iter().all(|c| c.category == CleanupCategory::RecycleBin));
    }

    #[test]
    fn recycle_bin_scan_tolerates_missing_root() {
        let tmp = TempDir::new().unwrap();
        assert!(scan_recycle_bin_in(&tmp.path().join("nope")).is_empty());
    }

    // ---- Windows.old ------------------------------------------------------

    #[test]
    fn windows_old_found_as_single_coarse_entry() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("Windows.old");
        write_file(&old.join("Windows").join("System32").join("x"), 1000);
        write_file(&old.join("Users").join("y"), 500);

        let candidate = scan_windows_old_at(&old).expect("Windows.old detected");
        assert_eq!(candidate.category, CleanupCategory::WindowsOld);
        assert_eq!(candidate.size_bytes, 1500);
        assert!(scan_windows_old_at(&root.path().join("absent")).is_none());
    }

    // ---- old downloads ----------------------------------------------------

    #[test]
    fn downloads_threshold_and_extension_filter() {
        let dl = TempDir::new().unwrap();
        write_file(&dl.path().join("setup.exe"), 1000);
        write_file(&dl.path().join("tool.msi"), 2000);
        write_file(&dl.path().join("notes.txt"), 50);
        write_file(&dl.path().join("Installer.EXE"), 4000); // case-insensitive ext

        let fresh_now = SystemTime::now();
        assert!(
            scan_old_downloads_in(dl.path(), 30, fresh_now).is_empty(),
            "nothing is 30 days old yet"
        );

        // Pretend 31 days passed by shifting `now` forward past the cutoff.
        let later = fresh_now + Duration::from_secs(31 * 86_400);
        let found = scan_old_downloads_in(dl.path(), 30, later);
        assert_eq!(found.len(), 3, "exe/msi only: {found:?}");
        assert_eq!(total(&found), 7000);
        assert!(found.iter().all(|c| c.category == CleanupCategory::DownloadsInstaller));

        // A window longer than the simulated elapsed time keeps everything out.
        let none_yet = scan_old_downloads_in(dl.path(), 90, later);
        assert!(none_yet.is_empty(), "31 days < 90 days, so nothing qualifies");
    }

    #[test]
    fn downloads_scan_tolerates_missing_folder() {
        let tmp = TempDir::new().unwrap();
        let found = scan_old_downloads_in(
            &tmp.path().join("Downloads"),
            30,
            SystemTime::now(),
        );
        assert!(found.is_empty());
    }

    // ---- deletion ---------------------------------------------------------

    #[test]
    fn delete_removes_files_dirs_and_counts_bytes() {
        let tmp = TempDir::new().unwrap();
        let f1 = tmp.path().join("a.log");
        let tree = tmp.path().join("cache-dir");
        write_file(&f1, 111);
        write_file(&tree.join("sub").join("b"), 222);
        let candidates = vec![
            CleanupCandidate {
                path: f1,
                size_bytes: 111,
                category: CleanupCategory::Temp,
            },
            CleanupCandidate {
                path: tree,
                size_bytes: 222,
                category: CleanupCategory::BrowserCache,
            },
        ];

        let result = delete_candidates(&candidates);
        assert_eq!(result.attempted, 2);
        assert_eq!(result.deleted, 2);
        assert_eq!(result.failed, 0);
        assert_eq!(result.bytes_freed, 333);
        assert!(result.failures.is_empty());
        assert!(!tmp.path().join("a.log").exists());
    }

    #[test]
    fn delete_reports_missing_paths_gracefully() {
        let ghost = CleanupCandidate {
            path: PathBuf::from("Z:/gone/never-was.bin"),
            size_bytes: 7,
            category: CleanupCategory::Temp,
        };
        let result = delete_candidates(&[ghost]);
        assert_eq!(result.deleted, 0);
        assert_eq!(result.failed, 1);
        assert!(!result.failures[0].reason.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn delete_reports_locked_files_instead_of_panicking() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        let tmp = TempDir::new().unwrap();
        let locked_path = tmp.path().join("in-use.db");
        write_file(&locked_path, 64);
        let free_path = tmp.path().join("freeable.log");
        write_file(&free_path, 32);

        // Hold the file open with no sharing allowed -> deletion must fail.
        let _guard = OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&locked_path)
            .expect("open lock handle");

        let result = delete_candidates(&[
            CleanupCandidate {
                path: locked_path.clone(),
                size_bytes: 64,
                category: CleanupCategory::Temp,
            },
            CleanupCandidate {
                path: free_path,
                size_bytes: 32,
                category: CleanupCategory::Temp,
            },
        ]);
        assert_eq!(result.deleted, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures[0].path, locked_path);
        assert!(locked_path.exists());
        assert_eq!(result.bytes_freed, 32);
    }

    // ---- aggregation / formatting ------------------------------------------

    #[test]
    fn summarize_buckets_by_category() {
        let candidates = vec![
            CleanupCandidate {
                path: PathBuf::from("a"),
                size_bytes: 100,
                category: CleanupCategory::Temp,
            },
            CleanupCandidate {
                path: PathBuf::from("b"),
                size_bytes: 50,
                category: CleanupCategory::Temp,
            },
            CleanupCandidate {
                path: PathBuf::from("c"),
                size_bytes: 900,
                category: CleanupCategory::RecycleBin,
            },
        ];
        let summary = summarize(&candidates);
        assert_eq!(summary.len(), 5);
        let temp = summary
            .iter()
            .find(|s| s.category == CleanupCategory::Temp)
            .unwrap();
        assert_eq!((temp.item_count, temp.total_bytes), (2, 150));
        let rb = summary
            .iter()
            .find(|s| s.category == CleanupCategory::RecycleBin)
            .unwrap();
        assert_eq!((rb.item_count, rb.total_bytes), (1, 900));
    }

    #[test]
    fn format_size_scales() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn dism_args_are_the_documented_ones() {
        assert_eq!(
            dism_args(),
            vec![
                "/Online",
                "/Cleanup-Image",
                "/StartComponentCleanup",
                "/NoRestart"
            ]
        );
        // run_dism_cleanup itself shells out to a real system tool and can run
        // for minutes - manually verified only.
    }

    #[test]
    fn category_keys_are_stable() {
        assert_eq!(CleanupCategory::Temp.key(), "temp");
        assert_eq!(CleanupCategory::BrowserCache.key(), "browser_cache");
        assert_eq!(CleanupCategory::RecycleBin.key(), "recycle_bin");
        assert_eq!(CleanupCategory::WindowsOld.key(), "windows_old");
        assert_eq!(CleanupCategory::DownloadsInstaller.key(), "downloads_installer");
        assert_eq!(CleanupCategory::ALL.len(), 5);
    }
}
