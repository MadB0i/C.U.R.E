//! The directory walker.
//!
//! Every traversal in this engine goes through here so that the safety
//! properties hold in exactly one place and are testable in isolation:
//!
//! * **links are never followed** — a reparse point is recorded and skipped,
//!   so a junction cannot walk the walker out of the authorised root, and a
//!   self-referential link cannot make it loop;
//! * **unreadable entries never abort a scan** — they are counted, reported
//!   as partial, and the walk continues;
//! * **the walk is cancellable** at any entry boundary, and says so;
//! * **bounds are explicit** — depth, entry count and wall-clock, so a
//!   pathological tree degrades into a reported limit instead of a hang.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::fsx;
use super::tiers::{self, TierContext};
use super::types::{FileRecord, FolderRecord, SafetyTier, ScanFailure, ScanProgress, StorageClass};

/// Hard limits. A scan that hits one reports the limit instead of pretending
/// to have finished.
#[derive(Debug, Clone)]
pub struct WalkLimits {
    pub max_depth: usize,
    pub max_entries: u64,
    pub max_duration_ms: u64,
    /// Files smaller than this are counted but not itemised. Keeps a temp
    /// directory from producing a million rows for a few megabytes.
    pub min_item_size: u64,
    /// A download that has not been modified for this many days is labelled
    /// as an old download. Age is a label, never a verdict.
    pub idle_days: u64,
    /// A temporary or cached file modified more recently than this is not
    /// treated as a regenerable leftover: it is probably in use.
    pub recent_file_seconds: u64,
}

impl Default for WalkLimits {
    fn default() -> Self {
        WalkLimits {
            max_depth: 64,
            max_entries: 4_000_000,
            max_duration_ms: 120_000,
            min_item_size: 0,
            idle_days: 30,
            recent_file_seconds: 3600,
        }
    }
}

/// Why a walk stopped early, when it did.
#[derive(Debug, Clone, Default)]
pub struct WalkOutcome {
    pub cancelled: bool,
    pub hit_depth_limit: bool,
    pub hit_entry_limit: bool,
    pub hit_time_limit: bool,
    pub reparse_points_skipped: u64,
    pub permission_denied_dirs: u64,
    pub permission_denied_files: u64,
    pub locked_or_vanished: u64,
}

impl WalkOutcome {
    pub fn limited(&self) -> bool {
        self.hit_depth_limit || self.hit_entry_limit || self.hit_time_limit
    }

    pub fn limit_reason(&self) -> Option<&'static str> {
        if self.hit_time_limit {
            Some("time limit reached")
        } else if self.hit_entry_limit {
            Some("entry limit reached")
        } else if self.hit_depth_limit {
            Some("directory depth limit reached")
        } else {
            None
        }
    }
}

/// Per-root counters, merged into the scan coverage afterwards.
#[derive(Debug, Clone, Default)]
pub struct RootStats {
    pub files_seen: u64,
    pub dirs_seen: u64,
    pub bytes_logical: u64,
    pub bytes_allocated: u64,
    pub failures: Vec<ScanFailure>,
    /// Links deliberately not followed. Not a failure.
    pub skipped_links: Vec<ScanFailure>,
    pub outcome: WalkOutcome,
}

/// Sink for walker output.
pub struct WalkSink {
    pub files: Vec<FileRecord>,
    pub folders: Vec<FolderRecord>,
}

impl WalkSink {
    pub fn new() -> WalkSink {
        WalkSink {
            files: Vec::new(),
            folders: Vec::new(),
        }
    }
}

impl Default for WalkSink {
    fn default() -> Self {
        WalkSink::new()
    }
}

/// Live progress callback. Invoked at most a few hundred times a second.
pub type ProgressFn<'a> = &'a mut dyn FnMut(ScanProgress);

/// Everything one walk needs. Grouped so the call site cannot get the
/// arguments in the wrong order — a mis-ordered safety argument here would be
/// invisible and load-bearing.
#[derive(Debug, Clone, Copy)]
pub struct WalkRequest<'a> {
    pub root: &'a Path,
    pub class: StorageClass,
    pub ctx: &'a TierContext,
    pub limits: &'a WalkLimits,
    pub cancel: &'a Arc<AtomicBool>,
    pub scan_id: &'a str,
    pub root_label: &'a str,
}

/// Walk one root and append its files and folder aggregates to `sink`.
///
/// `class` is the provenance assigned to every file found under `root`.
/// Roots are already class-specific (the catalogue hands over resolved cache
/// directories, not `AppData`), so inheritance is the correct rule.
pub fn walk_root(req: WalkRequest<'_>, sink: &mut WalkSink, progress: ProgressFn<'_>) -> RootStats {
    let WalkRequest {
        root,
        class,
        ctx,
        limits,
        cancel,
        scan_id,
        root_label,
    } = req;
    let mut stats = RootStats::default();
    let started = Instant::now();
    /* One clock reading for the whole root, so every file in it is judged
    against the same moment. */
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut entries_seen: u64 = 0;
    let mut last_emit = Instant::now();

    // Identity set of directories already entered, so a hardlink-free tree
    // that somehow re-presents the same directory is only visited once.
    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();

    // Explicit stack: recursion on a deep tree would blow the real stack.
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            stats.outcome.cancelled = true;
            break;
        }
        if started.elapsed().as_millis() as u64 > limits.max_duration_ms {
            stats.outcome.hit_time_limit = true;
            break;
        }
        if entries_seen > limits.max_entries {
            stats.outcome.hit_entry_limit = true;
            break;
        }

        let readers = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                let denied = e.kind() == std::io::ErrorKind::PermissionDenied;
                if denied {
                    stats.outcome.permission_denied_dirs += 1;
                }
                stats.failures.push(ScanFailure {
                    path: dir.clone(),
                    reason: describe_io(&e),
                    is_permission: denied,
                });
                continue;
            }
        };

        if !visited_dirs.insert(dir.clone()) {
            continue;
        }
        stats.dirs_seen += 1;

        let mut subtree_files: u64 = 0;
        let mut subtree_logical: u64 = 0;
        let mut subtree_allocated: u64 = 0;
        let mut subtree_dirs: u64 = 0;
        let mut subtree_incomplete = false;

        for entry in readers {
            if cancel.load(Ordering::Relaxed) {
                stats.outcome.cancelled = true;
                break;
            }
            entries_seen += 1;
            if entries_seen > limits.max_entries {
                stats.outcome.hit_entry_limit = true;
                break;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    stats.outcome.locked_or_vanished += 1;
                    stats.failures.push(ScanFailure {
                        path: dir.clone(),
                        reason: describe_io(&e),
                        is_permission: e.kind() == std::io::ErrorKind::PermissionDenied,
                    });
                    continue;
                }
            };
            let path = entry.path();

            // `symlink_metadata` never resolves a link, so every check below
            // describes the *entry*, not whatever it might point at.
            let md = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    stats.outcome.locked_or_vanished += 1;
                    stats.failures.push(ScanFailure {
                        path: path.clone(),
                        reason: describe_io(&e),
                        is_permission: e.kind() == std::io::ErrorKind::PermissionDenied,
                    });
                    continue;
                }
            };

            if fsx::is_reparse_point(&md) {
                stats.outcome.reparse_points_skipped += 1;
                /* A link inside this folder was not followed, so the
                folder's totals do not include what is behind it. */
                subtree_incomplete = true;
                /* Recorded separately from the failures. Not following a link
                is the policy working, not a read that went wrong, and
                mixing the two would downgrade an otherwise complete
                measurement. */
                stats.skipped_links.push(ScanFailure {
                    path: path.clone(),
                    reason: "link or reparse point — not followed".to_string(),
                    is_permission: false,
                });
                continue;
            }

            if md.is_dir() {
                if depth + 1 > limits.max_depth {
                    stats.outcome.hit_depth_limit = true;
                    /* The tree below this point was not walked, so this
                    folder's totals are partial. Saying so is the point
                    of the field. */
                    subtree_incomplete = true;
                    continue;
                }
                subtree_dirs += 1;
                stack.push((path, depth + 1));
                continue;
            }

            if !md.is_file() {
                // Sockets, FIFOs and device nodes carry no reclaimable bytes.
                continue;
            }

            let logical = md.len();
            let allocated = fsx::allocated_size(&path, logical);
            subtree_files += 1;
            subtree_logical += logical;
            subtree_allocated += allocated.bytes;
            stats.files_seen += 1;
            stats.bytes_logical += logical;
            stats.bytes_allocated += allocated.bytes;

            let modified = fsx::unix_seconds(md.modified());
            let accessed = fsx::unix_seconds(md.accessed());
            let created = fsx::unix_seconds(md.created());
            /* The catalogue says what the root is; the file itself says
            what it is. A build directory under `D:\\Projects` and an
            installer in Downloads are recognised here, not by guessing
            at the location afterwards. */
            let file_class =
                tiers::refine_class(&path, class, modified, now_unix, limits.idle_days);
            let mut classification = tiers::classify(&path, file_class, ctx);
            let extension = tiers::extension(&path);

            /* A temporary file that was written moments ago is very probably
            in use right now. The engine may still remove it if the
            operator picks it, but it must not be presented as a
            regenerable leftover, because the bytes may come back or the
            file may be held open by a running program. The same guard is
            applied when the plan is built and again when it is executed,
            so a file that was fresh when it was measured cannot be
            quietly promoted back to "safe" later. */
            tiers::apply_activity_guard(
                &mut classification,
                file_class,
                modified,
                now_unix,
                limits.recent_file_seconds,
                false,
            );

            if logical >= limits.min_item_size {
                sink.files.push(FileRecord {
                    path: path.clone(),
                    size_logical: logical,
                    size_allocated: Some(allocated.bytes),
                    modified_unix: modified,
                    // Recorded, deliberately never used as evidence: Windows
                    // frequently has last-access updates switched off, so a
                    // stale atime proves nothing either way.
                    accessed_unix: accessed,
                    created_unix: created,
                    extension,
                    hard_link_count: fsx::hard_link_count(&path),
                    is_reparse_point: false,
                    is_read_only: fsx::is_read_only(&md),
                    is_hidden: fsx::is_hidden(&md),
                    is_system: fsx::is_system(&md),
                    class: file_class,
                    tier: classification.tier,
                    reasons: classification.reasons,
                });
            }

            if last_emit.elapsed().as_millis() >= 120 {
                last_emit = Instant::now();
                progress(ScanProgress {
                    scan_id: scan_id.to_string(),
                    phase: "walking".to_string(),
                    root_label: root_label.to_string(),
                    current_path: path.to_string_lossy().to_string(),
                    dirs_seen: stats.dirs_seen,
                    files_seen: stats.files_seen,
                    bytes_seen: stats.bytes_logical,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    // An ETA from one root's samples is a guess. The caller
                    // decides when enough samples exist to show one.
                    eta_ms: None,
                });
            }
        }

        sink.folders.push(FolderRecord {
            path: dir.clone(),
            depth,
            size_logical: subtree_logical,
            size_allocated: subtree_allocated,
            file_count: subtree_files,
            dir_count: subtree_dirs,
            class,
            tier: tiers::classify(&dir, class, ctx).tier,
            incomplete: subtree_incomplete,
        });
    }

    if cancel.load(Ordering::Relaxed) {
        stats.outcome.cancelled = true;
    }
    stats
}

/// Turn an `io::Error` into wording that tells the operator something useful.
pub fn describe_io(e: &std::io::Error) -> String {
    // Raw codes first: Windows reports a sharing violation as an uncategorised
    // error, so matching on `ErrorKind` alone would print the raw Win32 text
    // for the single most common real-world cause.
    match e.raw_os_error() {
        Some(32) => return "locked by another process".to_string(),
        Some(5) => return "access denied".to_string(),
        Some(3) => return "path not found".to_string(),
        _ => {}
    }
    use std::io::ErrorKind::*;
    match e.kind() {
        PermissionDenied => "access denied".to_string(),
        NotFound => "no longer exists".to_string(),
        _ => e.to_string(),
    }
}

/// Free space on the volume holding `path`, for the "is this even worth it"
/// question.
pub fn volume_of(path: &Path) -> Option<fsx::VolumeSpace> {
    fsx::volume_space(path)
}

/// Tier helper re-exported so callers do not need to reach into `tiers`.
pub fn classify(path: &Path, class: StorageClass, ctx: &TierContext) -> (SafetyTier, Vec<String>) {
    let c = tiers::classify(path, class, ctx);
    (c.tier, c.reasons)
}
