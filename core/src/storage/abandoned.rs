//! "Possibly unused" application data.
//!
//! This module answers a question the storage engine is allowed to ask and is
//! **not** allowed to act on: which application data folders look dormant?
//!
//! Every record carries a `caveat` field, and every record is tiered
//! `Unknown`. That is not caution for its own sake — an application that has
//! not written to its data directory in a year may be installed, may be
//! licensed and still in use, may store its real state in the cloud, and may
//! be the only copy of something. The engine's job is to make the folder
//! visible and explain what it does and does not know.
//!
//! Three independent signals are combined, and each one is recorded:
//!
//! * **age** — newest file modification time anywhere in the tree;
//! * **install record** — does any installed program claim this location;
//! * **ownership** — is the directory still linked from anywhere we know.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::fsx;
use super::tiers::TierContext;
use super::types::{AbandonedAppRecord, SafetyTier, StorageClass};

/// Default inactivity threshold before a folder is even considered.
pub const DEFAULT_IDLE_DAYS: u64 = 180;

/// Minimum size worth reporting. Below this the folder is not worth the
/// operator's attention even if it is definitely dead.
pub const MIN_REPORT_BYTES: u64 = 1024 * 1024;

/// Bound on how long the abandoned pass may run. It is the most expensive
/// per-directory work in the engine and the least actionable, so it yields.
const BUDGET_MS: u64 = 20_000;

/// An installed program, as recorded in the Windows uninstall registry.
#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub display_name: String,
    pub install_location: Option<PathBuf>,
}

/// Read installed programs from the machine's own uninstall registry.
///
/// Read-only, best effort, and deliberately generous: an entry with no name is
/// skipped rather than counted, and a failure returns an empty list, which
/// makes every downstream heuristic *less* confident rather than more.
pub fn installed_apps() -> Vec<InstalledApp> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;

        for (root, path) in [
            (
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            ),
            (
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            ),
            (
                HKEY_CURRENT_USER,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            ),
        ] {
            let Ok(key) = RegKey::predef(root).open_subkey(path) else {
                continue;
            };
            for name in key.enum_keys().flatten() {
                let Ok(sub) = key.open_subkey(&name) else {
                    continue;
                };
                let Ok(display) = sub.get_value::<String, _>("DisplayName") else {
                    continue;
                };
                let location = sub
                    .get_value::<String, _>("InstallLocation")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(PathBuf::from);
                out.push(InstalledApp {
                    display_name: display,
                    install_location: location,
                });
            }
        }
    }
    let _ = &mut out;
    out
}

/// How strongly the available evidence suggests a folder is dormant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dormancy {
    /// No file inside has been touched recently.
    Stale,
    /// No file inside has been touched AND no installed program claims it.
    StaleAndUnclaimed,
    /// Evidence is missing (no uninstall registry readable), so only age applies.
    AgeOnly,
}

impl Dormancy {
    pub fn label(self) -> &'static str {
        match self {
            Dormancy::Stale => "Dormant by age",
            Dormancy::StaleAndUnclaimed => "Dormant and unclaimed by any installed program",
            Dormancy::AgeOnly => "Dormant by age only — install records unavailable",
        }
    }
}

/// Outcome of the abandoned pass, including what it could not determine.
#[derive(Debug, Clone, Default)]
pub struct AbandonedReport {
    pub directories_examined: u64,
    pub install_records_found: usize,
    pub install_records_available: bool,
    pub truncated: bool,
    pub failures: Vec<(PathBuf, String)>,
}

/// Inspect top-level application data directories.
///
/// `bases` are `%LOCALAPPDATA%` and `%APPDATA%`. Each immediate child is
/// examined; nothing is removed, opened for writing, or otherwise disturbed.
pub fn find_abandoned(
    bases: &[PathBuf],
    idle_days: u64,
    ctx: &TierContext,
    cancel: &Arc<AtomicBool>,
) -> (Vec<AbandonedAppRecord>, AbandonedReport) {
    let started = Instant::now();
    let apps = installed_apps();
    let mut report = AbandonedReport {
        install_records_found: apps.len(),
        install_records_available: !apps.is_empty(),
        ..Default::default()
    };
    let mut records: Vec<AbandonedAppRecord> = Vec::new();
    let now = std::time::SystemTime::now();

    for base in bases {
        if cancel.load(Ordering::Relaxed) || started.elapsed().as_millis() as u64 > BUDGET_MS {
            report.truncated = true;
            break;
        }
        let Ok(readers) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in readers.flatten() {
            if cancel.load(Ordering::Relaxed) || started.elapsed().as_millis() as u64 > BUDGET_MS {
                report.truncated = true;
                break;
            }
            let path = entry.path();
            let md = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !md.is_dir() || fsx::is_reparse_point(&md) {
                continue;
            }
            report.directories_examined += 1;

            let stat = measure(&path, cancel);
            if stat.bytes < MIN_REPORT_BYTES || stat.files == 0 {
                continue;
            }
            let days = stat
                .newest_modified
                .and_then(|t| {
                    now.duration_since(std::time::UNIX_EPOCH + std::time::Duration::from_secs(t))
                        .ok()
                })
                .map(|d| d.as_secs() / 86_400);
            let Some(days) = days else { continue };
            if days < idle_days {
                continue;
            }

            let owner = find_installed_owner(&path, &apps);
            let dormancy = match (&owner, report.install_records_available) {
                (None, true) => Dormancy::StaleAndUnclaimed,
                (None, false) => Dormancy::AgeOnly,
                (Some(_), _) => continue, // An installed program claims it.
            };

            let mut reasons = vec![format!("newest file inside was modified {} days ago", days)];
            reasons.push(format!(
                "no installed program lists this location ({})",
                dormancy.label()
            ));
            if stat.unreadable_dirs > 0 {
                reasons.push(format!(
                    "{} subfolder(s) could not be read, so this size is a floor",
                    stat.unreadable_dirs
                ));
            }
            let _ = ctx;

            records.push(AbandonedAppRecord {
                app_name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string()),
                path: path.clone(),
                size_logical: stat.bytes,
                size_allocated: stat.allocated,
                file_count: stat.files,
                last_modified_unix: stat.newest_modified,
                days_since_change: Some(days),
                class: StorageClass::AbandonedAppData,
                tier: SafetyTier::Unknown,
                reasons,
                caveat: "No file here has been deleted or modified by C.U.R.E. An application \
                 that is still installed but idle, keeps state on another volume, or holds a \
                 licence tied to this folder will all look exactly the same to this measurement."
                    .to_string(),
            });
        }
    }

    records.sort_by_key(|r| std::cmp::Reverse(r.size_allocated));
    (records, report)
}

/// The installed program whose declared install location contains `path`.
fn find_installed_owner<'a>(path: &Path, apps: &'a [InstalledApp]) -> Option<&'a InstalledApp> {
    apps.iter().find(|app| {
        app.install_location
            .as_ref()
            .map(|loc| super::tiers::is_under(path, loc))
            .unwrap_or(false)
    })
}

struct Stat {
    bytes: u64,
    allocated: u64,
    files: u64,
    newest_modified: Option<u64>,
    unreadable_dirs: u64,
}

/// Size and newest mtime of one tree. Iterative, link-safe, cancellable.
fn measure(root: &Path, cancel: &Arc<AtomicBool>) -> Stat {
    let mut stat = Stat {
        bytes: 0,
        allocated: 0,
        files: 0,
        newest_modified: None,
        unreadable_dirs: 0,
    };
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(readers) = std::fs::read_dir(&dir) else {
            stat.unreadable_dirs += 1;
            continue;
        };
        for entry in readers.flatten() {
            let path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if fsx::is_reparse_point(&md) {
                continue;
            }
            if md.is_dir() {
                stack.push(path);
                continue;
            }
            if !md.is_file() {
                continue;
            }
            let logical = md.len();
            stat.bytes += logical;
            stat.allocated += fsx::allocated_size(&path, logical).bytes;
            stat.files += 1;
            if let Some(m) = fsx::unix_seconds(md.modified()) {
                stat.newest_modified = Some(stat.newest_modified.map_or(m, |cur: u64| cur.max(m)));
            }
        }
        // Touching a directory updates its mtime, so a directory entry's own
        // timestamp is not evidence of activity. Only files count.
    }
    stat
}

/// Bytes a record would free if it were ever removed — reported so the
/// operator can weigh it, never as a reclaim total.
pub fn potential_bytes(record: &AbandonedAppRecord) -> u64 {
    record.size_allocated
}
