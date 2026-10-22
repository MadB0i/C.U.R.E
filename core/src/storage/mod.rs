//! Storage intelligence engine.
//!
//! Three phases, each of which can be stopped without leaving anything
//! behind:
//!
//! 1. **Measure** — walk the catalogue roots (plus anything the user picked),
//!    record every file with its real logical and allocated size, and record
//!    what could not be read.
//! 2. **Understand** — group files by provenance and size, verify duplicate
//!    groups by full content hash, and describe dormant application data.
//! 3. **Act** — build a preview from an explicit selection, then re-validate
//!    every single target against the live filesystem before removing it.
//!
//! The separation is the point. Nothing in phase 1 or 2 writes to disk, and
//! phase 3 never trusts a value produced by phases 1 or 2 without re-checking
//! it: a path that moved, changed size, turned into a link, or is no longer
//! permitted by its tier is dropped from the plan rather than deleted.

pub mod abandoned;
pub mod casefile;
pub mod catalog;
pub mod detective;
pub mod duplicates;
pub mod fsx;
pub mod reclaim;
pub mod safedelete;
pub mod tiers;
pub mod types;
pub mod walk;

// The DTO vocabulary is re-exported flat so callers write
// `storage::StorageScan` rather than reaching through `storage::types::`.
pub use types::*;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use catalog::ScanDepth;

/// The window inside which a regenerable file is treated as still in use.
///
/// It is the same window the walk uses, so a file the walk downgraded is
/// never promoted again by the plan preview or the execution re-check.
fn activity_guard_window() -> u64 {
    catalog::limits_for(ScanDepth::Quick).recent_file_seconds
}

/// Everything the caller can choose about a scan.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub depth: ScanDepth,
    /// Extra drives or folders the user selected.
    pub extra_roots: Vec<PathBuf>,
    pub find_duplicates: bool,
    pub find_abandoned: bool,
    /// Files at or above this size enter the large-file list.
    pub large_file_threshold: u64,
    pub large_file_limit: usize,
    /// How many folders are carried forward as the "largest folders" list.
    pub large_folder_limit: usize,
    /// `false` measures only `extra_roots`. Used by the benchmark, where the
    /// point is to time a known tree rather than the whole machine.
    pub use_catalogue: bool,
    pub walk_limits: walk::WalkLimits,
    pub duplicate_options: duplicates::DuplicateOptions,
    pub idle_days: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            depth: ScanDepth::Quick,
            extra_roots: Vec::new(),
            find_duplicates: true,
            find_abandoned: false,
            use_catalogue: true,
            large_file_threshold: 50 * 1024 * 1024,
            large_file_limit: 300,
            large_folder_limit: 400,
            walk_limits: catalog::limits_for(ScanDepth::Quick),
            duplicate_options: duplicates::DuplicateOptions::default(),
            idle_days: abandoned::DEFAULT_IDLE_DAYS,
        }
    }
}

/// Phase names used in progress events and in the UI's coverage report.
pub const PHASE_WALKING: &str = "walking";
pub const PHASE_HASHING: &str = "hashing";
pub const PHASE_CORRELATING: &str = "correlating";
pub const PHASE_FINISHED: &str = "finished";
pub const PHASE_CANCELLED: &str = "cancelled";

/// Build a scan id that is unique per run without needing a random source.
fn make_scan_id(started: SystemTime, roots: usize) -> String {
    let nanos = started
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("scan-{:x}-{:x}", nanos, roots)
}

/// Run a complete measurement pass.
///
/// `emit` receives progress for every root transition and roughly eight times
/// a second while walking. `cancel` is checked at entry boundaries throughout.
pub fn scan(
    options: &ScanOptions,
    cancel: &Arc<AtomicBool>,
    scan_id: &str,
    mut emit: impl FnMut(ScanProgress),
) -> StorageScan {
    let started_at = Instant::now();
    let started_unix = unix_now();
    let ctx = tiers::TierContext::from_env();
    let (roots, dropped_roots) = if options.use_catalogue {
        catalog::roots_with_report(options.depth, &options.extra_roots)
    } else {
        let mut only = options
            .extra_roots
            .iter()
            .map(|path| types::ScanRoot {
                label: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string()),
                path: path.clone(),
                class: StorageClass::UserSelected,
                is_catalogue: false,
            })
            .collect::<Vec<types::ScanRoot>>();
        let dropped = catalog::drop_nested_roots(&mut only);
        (only, dropped)
    };

    let mut coverage = ScanCoverage {
        overlapping_roots_dropped: dropped_roots as u64,
        ..Default::default()
    };
    let mut sink = walk::WalkSink::new();
    let mut coverage_failures: Vec<types::ScanFailure> = Vec::new();
    let mut root_ms: Vec<u128> = Vec::new();
    let mut any_cancelled = false;
    let mut limited: Option<&'static str> = None;

    for (index, root) in roots.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            any_cancelled = true;
            break;
        }

        let exists = std::fs::symlink_metadata(&root.path)
            .map(|md| md.is_dir() && !fsx::is_reparse_point(&md))
            .unwrap_or(false);

        if !exists {
            coverage.roots.push(RootCoverage {
                path: root.path.clone(),
                label: root.label.clone(),
                class: root.class,
                resolved: false,
                reason: "not present on this machine".to_string(),
                files_seen: 0,
                dirs_seen: 0,
                bytes_logical: 0,
                bytes_allocated: 0,
                incomplete: false,
            });
            continue;
        }

        let root_started = Instant::now();
        let mut root_sink = walk::WalkSink::new();
        let stats = {
            let mut p = |progress: ScanProgress| {
                let remaining = roots.len().saturating_sub(index + 1);
                let mut progress = progress;
                progress.eta_ms = eta_for(&root_ms, remaining);
                emit(progress);
            };
            walk::walk_root(
                walk::WalkRequest {
                    root: &root.path,
                    class: root.class,
                    ctx: &ctx,
                    limits: &options.walk_limits,
                    cancel,
                    scan_id,
                    root_label: &root.label,
                },
                &mut root_sink,
                &mut p,
            )
        };
        root_ms.push(root_started.elapsed().as_millis());

        sink.files.append(&mut root_sink.files);
        sink.folders.append(&mut root_sink.folders);
        coverage_failures.extend(stats.failures.iter().cloned());
        coverage
            .skipped_links
            .extend(stats.skipped_links.iter().cloned());

        coverage.reparse_points_skipped += stats.outcome.reparse_points_skipped;
        coverage.permission_denied_dirs += stats.outcome.permission_denied_dirs;
        coverage.permission_denied_files += stats.outcome.permission_denied_files;
        coverage.locked_or_vanished += stats.outcome.locked_or_vanished;
        coverage.dirs_seen += stats.dirs_seen;
        if stats.outcome.cancelled {
            any_cancelled = true;
        }
        if let Some(reason) = stats.outcome.limit_reason() {
            limited = Some(reason);
        }

        coverage.roots.push(RootCoverage {
            path: root.path.clone(),
            label: root.label.clone(),
            class: root.class,
            resolved: true,
            reason: stats
                .outcome
                .limit_reason()
                .map(|r| format!("stopped early: {r}"))
                .unwrap_or_default(),
            files_seen: stats.files_seen,
            dirs_seen: stats.dirs_seen,
            bytes_logical: stats.bytes_logical,
            bytes_allocated: stats.bytes_allocated,
            incomplete: !stats.failures.is_empty() || stats.outcome.limited(),
        });
    }

    coverage.files_seen = sink.files.len() as u64;
    coverage.cancelled = any_cancelled;

    if let Some(reason) = limited {
        coverage.failures.push(types::ScanFailure {
            path: PathBuf::from("(scan)"),
            reason: reason.to_string(),
            is_permission: false,
        });
    }
    coverage.failures.extend(coverage_failures);

    // Free space on the volume holding the first resolved root: a real
    // measurement of the same volume the reclaim figure refers to.
    if let Some(first) = coverage
        .roots
        .iter()
        .find(|r| r.resolved)
        .map(|r| r.path.clone())
    {
        if let Some(vs) = walk::volume_of(&first) {
            coverage.volume_total_bytes = Some(vs.total_bytes);
            coverage.volume_free_bytes = Some(vs.free_bytes);
            coverage.volume_label = vs.label;
        }
    }

    // ---- large files ----------------------------------------------------
    let mut large_files: Vec<FileRecord> = sink
        .files
        .iter()
        .filter(|f| f.size_allocated.unwrap_or(f.size_logical) >= options.large_file_threshold)
        .cloned()
        .collect();
    large_files.sort_by(|a, b| {
        b.size_allocated
            .unwrap_or(b.size_logical)
            .cmp(&a.size_allocated.unwrap_or(a.size_logical))
            .then_with(|| a.path.cmp(&b.path))
    });
    large_files.truncate(options.large_file_limit);

    // ---- large folders ---------------------------------------------------
    // Every folder is measured; only the biggest are carried forward. The
    // UI shows a treemap and a ranked list, and neither needs the two
    // hundred thousand empty directories in between.
    let mut large_folders: Vec<types::FolderRecord> = sink.folders.clone();
    large_folders.retain(|f| f.size_allocated > 0);
    large_folders.sort_by(|a, b| {
        b.size_allocated
            .cmp(&a.size_allocated)
            .then_with(|| a.path.cmp(&b.path))
    });
    large_folders.truncate(options.large_folder_limit);

    // ---- duplicate verification ----------------------------------------
    let mut duplicate_sets = Vec::new();
    let mut duplicate_report = duplicates::DuplicateReport::default();
    if options.find_duplicates && !any_cancelled {
        let pool: Vec<duplicates::Candidate> = sink
            .files
            .iter()
            .filter(|f| !f.is_reparse_point)
            .map(|f| duplicates::Candidate {
                path: f.path.clone(),
                size_logical: f.size_logical,
                modified_unix: f.modified_unix,
                hard_link_count: f.hard_link_count,
                class: f.class,
                tier: f.tier,
            })
            .collect();
        let (sets, report) = duplicates::find_duplicates(
            &pool,
            &ctx,
            &options.duplicate_options,
            cancel,
            scan_id,
            &mut emit,
        );
        duplicate_sets = sets;
        duplicate_report = report;
    }

    // ---- dormant application data --------------------------------------
    let mut abandoned_records = Vec::new();
    if options.find_abandoned && !any_cancelled {
        let mut bases = Vec::new();
        for key in ["LOCALAPPDATA", "APPDATA"] {
            if let Some(p) = std::env::var_os(key).map(PathBuf::from) {
                if !bases.contains(&p) {
                    bases.push(p);
                }
            }
        }
        let (records, _) = abandoned::find_abandoned(&bases, options.idle_days, &ctx, cancel);
        abandoned_records = records;
    }

    // ---- aggregates -----------------------------------------------------
    //
    // The summaries and the headline come from *one* identity table, not from
    // a sum of overlapping views. A file that is both a "user temp" file and
    // a verified duplicate contributes its bytes once, so the headline can
    // never promise more space than the disk can give back.
    let summaries = summarize(&sink.files);
    let accounting = reclaim::compute(&sink.files, &duplicate_sets);
    let safe_bytes = accounting.safe_bytes;
    let careful_bytes = accounting.careful_bytes;
    let protected_bytes = accounting.protected_bytes;
    let duplicate_reclaimable = accounting.duplicate_bytes;
    let duplicate_bulk = accounting.duplicate_bulk_bytes;

    // Measured here, at the very end: duplicate verification is part of the
    // scan, and a duration that stops before it would under-report the cost
    // of exactly the part that reads every byte.
    coverage.duration_ms = started_at.elapsed().as_millis() as u64;

    emit(ScanProgress {
        scan_id: scan_id.to_string(),
        phase: if any_cancelled {
            PHASE_CANCELLED
        } else {
            PHASE_FINISHED
        }
        .to_string(),
        root_label: String::new(),
        current_path: String::new(),
        dirs_seen: coverage.dirs_seen,
        files_seen: coverage.files_seen,
        bytes_seen: safe_bytes,
        elapsed_ms: started_at.elapsed().as_millis() as u64,
        eta_ms: Some(0),
    });

    StorageScan {
        scan_id: scan_id.to_string(),
        started_unix,
        finished_unix: unix_now(),
        roots,
        files: sink.files,
        folders: sink.folders,
        summaries,
        coverage,
        duplicate_coverage: DuplicateCoverage {
            files_considered: duplicate_report.files_considered,
            fully_hashed: duplicate_report.fully_hashed,
            size_buckets: duplicate_report.size_buckets,
            partial_hash_groups: duplicate_report.partial_hash_groups,
            full_hash_groups: duplicate_report.full_hash_groups,
            verified_sets: duplicate_report.verified_sets,
            skipped_oversize: duplicate_report.skipped_oversize,
            skipped_hardlinked: duplicate_report.skipped_hardlinked,
            skipped_changed_during_scan: duplicate_report.skipped_changed_during_scan,
            skipped_unreadable: duplicate_report.skipped_unreadable,
            bytes_hashed: duplicate_report.bytes_hashed,
            truncated: duplicate_report.truncated,
            summary: duplicate_report.coverage_sentence(),
        },
        large_files,
        large_folders,
        duplicates: duplicate_sets,
        abandoned: abandoned_records,
        safe_reclaimable_bytes: safe_bytes,
        careful_reclaimable_bytes: careful_bytes,
        protected_bytes,
        protected_only_bytes: accounting.protected_only_bytes,
        unknown_bytes: accounting.unknown_bytes,
        duplicate_reclaimable_bytes: duplicate_reclaimable,
        duplicate_bulk_reclaimable_bytes: duplicate_bulk,
        safe_file_count: accounting.safe_file_count,
        careful_file_count: accounting.careful_file_count,
        protected_file_count: accounting.protected_file_count,
        distinct_file_count: accounting.distinct_file_count,
    }
}

fn eta_for(root_ms: &[u128], remaining: usize) -> Option<u64> {
    if remaining == 0 {
        return Some(0);
    }
    // One sample is not an average. Two is the point where this stops being
    // arithmetic on a single data point and starts being a forecast.
    if root_ms.len() < 2 {
        return None;
    }
    let mean = root_ms.iter().sum::<u128>() / root_ms.len() as u128;
    Some((mean * remaining as u128) as u64)
}

fn summarize(files: &[FileRecord]) -> Vec<ClassSummary> {
    let mut map: std::collections::BTreeMap<StorageClass, ClassSummary> =
        std::collections::BTreeMap::new();
    for f in files {
        let e = map.entry(f.class).or_insert_with(|| ClassSummary {
            class: f.class,
            // A class summary reports the *most cautious* tier present, so a
            // category can never look safer than its worst member.
            tier: SafetyTier::SafeToReview,
            item_count: 0,
            bytes_logical: 0,
            bytes_allocated: 0,
        });
        e.item_count += 1;
        e.bytes_logical = e.bytes_logical.saturating_add(f.size_logical);
        e.bytes_allocated = e
            .bytes_allocated
            .saturating_add(f.size_allocated.unwrap_or(f.size_logical));
        if f.tier > e.tier {
            e.tier = f.tier;
        }
    }
    // Categories the scan did not reach are still listed, at zero, so the
    // filter bar never hides a class that simply had nothing to report.
    for class in StorageClass::ALL {
        map.entry(class).or_insert_with(|| ClassSummary {
            class,
            tier: class.floor_tier(),
            item_count: 0,
            bytes_logical: 0,
            bytes_allocated: 0,
        });
    }
    map.into_values().collect()
}

pub fn new_scan_id() -> String {
    make_scan_id(SystemTime::now(), 0)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Phase 3: preview and execute
// ---------------------------------------------------------------------------

/// Selectable item handed from the UI: a path plus what the user expects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionItem {
    pub path: PathBuf,
    /// Size the UI displayed when the operator chose it. Re-checked at
    /// execution; a mismatch drops the item instead of deleting something
    /// that replaced it.
    #[serde(default)]
    pub expected_size: Option<u64>,
    /// `true` when this selection came from a bulk action rather than from
    /// the operator picking each item. A bulk selection may never carry a
    /// `ReviewCarefully` item: those need individual approval, and the engine
    /// enforces that here rather than trusting the caller.
    #[serde(default)]
    pub bulk: bool,
}

/// Build the preview. Nothing is deleted by this call.
///
/// Every requested path lands in exactly one of `items` or `refused`, and the
/// reason it was refused is carried with it.
pub fn build_plan(scan_result: &StorageScan, selection: &[SelectionItem]) -> CleanupPlan {
    let ctx = tiers::TierContext::from_env();
    let mut items: Vec<PlanItem> = Vec::new();
    let mut refused: Vec<PlanItem> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let by_path: std::collections::HashMap<String, &FileRecord> = scan_result
        .files
        .iter()
        .map(|f| (tiers::normalize(&f.path), f))
        .collect();

    for sel in selection {
        let key = tiers::normalize(&sel.path);
        if !seen.insert(key.clone()) {
            continue;
        }
        let mut stale = false;
        let (class, mut reasons) = match by_path.get(&key) {
            Some(f) => (f.class, f.reasons.clone()),
            None => {
                // Not in this scan. Could be a Recycle Bin directory unit, or
                // something the caller asked for by hand. Either way it must
                // not be assumed to be a scanned file.
                stale = true;
                (
                    StorageClass::Other,
                    vec!["not present in the current scan".to_string()],
                )
            }
        };

        let live_md = std::fs::symlink_metadata(&sel.path).ok();
        if live_md.is_none() {
            stale = true;
        } else if let Some(md) = &live_md {
            if fsx::is_reparse_point(md) {
                stale = true;
            }
        }
        if let (Some(expected), Some(md)) = (sel.expected_size, live_md.as_ref()) {
            if md.is_file() && expected != md.len() {
                stale = true;
            }
        }
        if !is_under_any_root(&sel.path, &scan_result.roots) {
            stale = true;
        }
        // Re-derive the tier from the live path: a file could have been
        // re-parented under Program Files between scan and preview.
        let mut live_classification = tiers::classify(&sel.path, class, &ctx);
        // ...and re-apply the activity guard to the *live* file. A
        // temporary file that was written moments ago, or that is held open
        // by a program right now, is not a regenerable leftover, and it must
        // not be presented as one here. Without this the plan showed a
        // "safe to review" tier for exactly the files the walk had already
        // downgraded.
        let live_modified = live_md
            .as_ref()
            .and_then(|m| fsx::unix_seconds(m.modified()));
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let in_use = fsx::is_in_use(&sel.path);
        tiers::apply_activity_guard(
            &mut live_classification,
            class,
            live_modified,
            now_unix,
            activity_guard_window(),
            in_use,
        );
        let live_tier = live_classification.tier;

        reasons.extend(live_classification.reasons);
        if !live_tier.allows_deletion_request() {
            reasons.push(format!(
                "refused: {} is not deletable",
                live_tier.label().to_lowercase()
            ));
        } else if sel.bulk && !live_tier.allows_bulk_recommendation() {
            // A bulk action may only carry bulk-eligible items. Anything
            // that needs individual approval is refused here, in the engine,
            // so no caller can route around it.
            reasons.push(
                "refused: this item needs individual approval and cannot be part of a bulk \
                 action"
                    .to_string(),
            );
        }

        let size_logical = live_md
            .as_ref()
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .or_else(|| {
                sel.expected_size
                    .or_else(|| by_path.get(&key).map(|f| f.size_logical))
            })
            .unwrap_or(0);
        let size_allocated = if live_md.as_ref().is_some_and(|m| m.is_file()) {
            fsx::allocated_size(&sel.path, size_logical).bytes
        } else {
            size_logical
        };

        let item = PlanItem {
            path: sel.path.clone(),
            size_logical,
            size_allocated,
            tier: live_tier,
            class,
            reasons,
            stale,
            // The identity is recorded here, at the moment the operator is
            // shown the item. Execution re-checks it through the handle it
            // is about to delete.
            file_id: safedelete::file_identity(&sel.path),
            approved_size: sel
                .expected_size
                .or_else(|| by_path.get(&key).map(|f| f.size_logical))
                .unwrap_or(size_logical),
        };

        if stale
            || !live_tier.allows_deletion_request()
            || (sel.bulk && !live_tier.allows_bulk_recommendation())
        {
            refused.push(item);
        } else {
            items.push(item);
        }
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.size_allocated));
    refused.sort_by_key(|i| std::cmp::Reverse(i.size_allocated));
    let estimated: u64 = items.iter().map(|i| i.size_allocated).sum();
    let careful = items
        .iter()
        .filter(|i| i.tier == SafetyTier::ReviewCarefully)
        .count();
    let consequence = consequence_text(items.len(), careful, estimated);
    let plan_id = plan_digest(&scan_result.scan_id, &items);

    CleanupPlan {
        scan_id: scan_result.scan_id.clone(),
        plan_id,
        items,
        refused,
        estimated_bytes: estimated,
        estimated_certainty: Certainty::from_coverage(&scan_result.coverage),
        consequence,
        confirmation_phrase: types::CONFIRM_PHRASE.to_string(),
    }
}

/// A stable digest of the approved selections.
///
/// Execution recomputes this and refuses a plan whose contents no longer
/// match what the operator approved. A plan that was edited after approval —
/// by a bug, a stale UI, or something else in the process — is rejected
/// rather than silently executed.
pub fn plan_digest(scan_id: &str, items: &[PlanItem]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    scan_id.hash(&mut hasher);
    for item in items {
        tiers::normalize(&item.path).hash(&mut hasher);
        item.size_logical.hash(&mut hasher);
        item.size_allocated.hash(&mut hasher);
        item.tier.hash(&mut hasher);
        item.file_id.hash(&mut hasher);
    }
    format!("plan-{:016x}", hasher.finish())
}

/// Plain-language statement of what approving this plan will do.
fn consequence_text(count: usize, careful: usize, bytes: u64) -> String {
    if count == 0 {
        return "Nothing in this selection can be removed. See the refused list for why."
            .to_string();
    }
    let mut s = format!(
        "Removes {} file{} permanently, about {}. This is a direct delete: there is no \
         quarantine copy and no undo.",
        count,
        if count == 1 { "" } else { "s" },
        human_bytes(bytes)
    );
    if careful > 0 {
        s.push_str(&format!(
            " {careful} of them are user files that were selected individually."
        ));
    }
    s
}

/// Format a byte count without inventing precision.
pub fn human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Execute an approved plan.
///
/// Each target is re-validated against the live filesystem immediately before
/// removal: still present, still a regular file, still not a link, still under
/// an authorised root, still permitted by its tier, and unchanged in size since
/// the plan was built. Anything that fails a check is recorded and skipped.
/// The batch never aborts early.
///
/// The receipt separates what was *estimated* (the bytes each file occupied
/// before it was removed) from what was *observed* (the volume's free space
/// afterwards). The second is a property of the whole volume, not of any
/// individual file.
pub fn execute_plan(
    scan_result: &StorageScan,
    plan: &CleanupPlan,
    cancel: &Arc<AtomicBool>,
) -> CleanupReceipt {
    let ctx = tiers::TierContext::from_env();

    // ---- the plan must be the plan that was approved --------------------
    if plan.scan_id != scan_result.scan_id {
        return refused_receipt(
            scan_result,
            plan,
            "the plan belongs to a different scan; nothing was removed".to_string(),
        );
    }
    let recomputed = plan_digest(&plan.scan_id, &plan.items);
    if recomputed != plan.plan_id {
        return refused_receipt(
            scan_result,
            plan,
            "the approved plan no longer matches its own digest; nothing was removed".to_string(),
        );
    }

    // Probe every volume the plan touches, before anything is removed.
    let mut volumes: Vec<VolumeDelta> = Vec::new();
    for item in &plan.items {
        let Some(root) = fsx::volume_root(&item.path) else {
            continue;
        };
        let key = root.to_string_lossy().to_string();
        if volumes.iter().any(|v| v.root == key) {
            continue;
        }
        let space = fsx::volume_space_at(&root);
        volumes.push(VolumeDelta {
            root: key,
            free_before: space.as_ref().map(|v| v.free_bytes),
            free_after: None,
            delta: None,
            files_removed: 0,
        });
    }

    let mut deleted = 0usize;
    let mut attempted = 0usize;
    let mut failures: Vec<types::ScanFailure> = Vec::new();
    let mut removed_estimate: u64 = 0;
    let mut uncertain_bytes: u64 = 0;
    let mut planned_bytes: u64 = 0;
    let mut refused: Vec<PlanItem> = Vec::new();

    for item in &plan.items {
        if cancel.load(Ordering::Relaxed) {
            refused.push(PlanItem {
                reasons: vec!["scan cancelled before this item was reached".to_string()],
                ..item.clone()
            });
            continue;
        }

        // --- re-validate everything the scan told us, now -----------------
        let md = match std::fs::symlink_metadata(&item.path) {
            Ok(md) => md,
            Err(e) => {
                failures.push(types::ScanFailure {
                    path: item.path.clone(),
                    reason: format!("re-check failed: {}", walk::describe_io(&e)),
                    is_permission: e.kind() == std::io::ErrorKind::PermissionDenied,
                });
                continue;
            }
        };
        if fsx::is_reparse_point(&md) {
            refused.push(PlanItem {
                reasons: vec!["became a link after the plan was built".to_string()],
                ..item.clone()
            });
            continue;
        }
        if !md.is_file() {
            refused.push(PlanItem {
                reasons: vec!["is no longer a regular file".to_string()],
                ..item.clone()
            });
            continue;
        }
        if md.len() != item.size_logical {
            refused.push(PlanItem {
                reasons: vec![format!(
                    "changed size since the scan ({} then, {} now)",
                    item.size_logical,
                    md.len()
                )],
                ..item.clone()
            });
            continue;
        }
        if !is_under_any_root(&item.path, &scan_result.roots) {
            refused.push(PlanItem {
                reasons: vec!["is no longer inside an authorised scan root".to_string()],
                ..item.clone()
            });
            continue;
        }
        // The tier is re-derived from the live file, and the activity guard
        // is applied to it again. A temporary file that a program has
        // opened since the plan was approved is not removed as a
        // regenerable leftover: it is refused, with the reason.
        let mut live_classification = tiers::classify(&item.path, item.class, &ctx);
        let live_modified = fsx::unix_seconds(md.modified());
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let in_use = fsx::is_in_use(&item.path);
        tiers::apply_activity_guard(
            &mut live_classification,
            item.class,
            live_modified,
            now_unix,
            activity_guard_window(),
            in_use,
        );
        let live_tier = live_classification.tier;
        if !live_tier.allows_deletion_request() || live_tier != item.tier {
            let mut reasons = live_classification.reasons;
            reasons.push(format!(
                "tier changed after the plan was built (now {})",
                live_tier.label().to_lowercase()
            ));
            refused.push(PlanItem {
                tier: live_tier,
                reasons,
                ..item.clone()
            });
            continue;
        }

        // Read-only files are a user decision, not an obstacle to clear.
        // Skipping them is always safe, so the default is to skip.
        if fsx::is_read_only(&md) {
            failures.push(types::ScanFailure {
                path: item.path.clone(),
                reason: "marked read-only; left in place".to_string(),
                is_permission: false,
            });
            continue;
        }

        // A parent directory that has become a link makes the target
        // ambiguous. Refuse rather than guess.
        if let Err(reason) = safedelete::ancestors_are_real(&item.path) {
            refused.push(PlanItem {
                reasons: vec![reason],
                ..item.clone()
            });
            continue;
        }

        attempted += 1;
        planned_bytes = planned_bytes.saturating_add(item.size_allocated);
        let freed = fsx::allocated_size(&item.path, item.size_logical).bytes;

        // Remove the *verified object*, not the path. The identity recorded
        // when the operator approved this item is re-checked through the
        // handle, so a file that was swapped in the meantime is refused and
        // a parent that was turned into a junction cannot redirect the
        // delete.
        match safedelete::remove_if_unchanged(&item.path, item.file_id.as_ref()) {
            Ok(outcome) => {
                if item.path.symlink_metadata().is_err() {
                    deleted += 1;
                    removed_estimate = removed_estimate.saturating_add(freed);
                    if !outcome.reclaim_certain {
                        uncertain_bytes = uncertain_bytes.saturating_add(freed);
                    }
                    if let Some(root) = fsx::volume_root(&item.path) {
                        let key = root.to_string_lossy().to_string();
                        if let Some(v) = volumes.iter_mut().find(|v| v.root == key) {
                            v.files_removed += 1;
                        }
                    }
                } else {
                    failures.push(types::ScanFailure {
                        path: item.path.clone(),
                        reason: "still present after the delete was reported successful"
                            .to_string(),
                        is_permission: false,
                    });
                }
            }
            Err(e) => {
                // A refusal is not a failure of the disk; it is the safety
                // check working. It is reported separately.
                refused.push(PlanItem {
                    reasons: vec![e.reason()],
                    ..item.clone()
                });
            }
        }
    }

    // Probe the volumes again, after the batch.
    for v in volumes.iter_mut() {
        let root = std::path::PathBuf::from(&v.root);
        if let Some(space) = fsx::volume_space_at(&root) {
            v.free_after = Some(space.free_bytes);
            if let (Some(before), Some(after)) = (v.free_before, v.free_after) {
                v.delta = Some(after as i64 - before as i64);
            }
        }
    }

    refused.extend(plan.refused.iter().cloned());

    CleanupReceipt {
        scan_id: scan_result.scan_id.clone(),
        planned: plan.items.len(),
        attempted,
        deleted,
        refused: refused.len(),
        failed: failures.len(),
        bytes_removed_estimate: removed_estimate,
        bytes_uncertain: uncertain_bytes,
        bytes_estimated: planned_bytes,
        failures,
        refused_items: refused,
        finished_unix: unix_now(),
        note: receipt_note(deleted, removed_estimate, uncertain_bytes),
        volumes,
    }
}

/// A receipt for a plan that was refused before anything was attempted.
fn refused_receipt(
    scan_result: &StorageScan,
    plan: &CleanupPlan,
    reason: String,
) -> CleanupReceipt {
    CleanupReceipt {
        scan_id: scan_result.scan_id.clone(),
        planned: plan.items.len(),
        attempted: 0,
        deleted: 0,
        refused: plan.items.len() + plan.refused.len(),
        failed: 0,
        bytes_removed_estimate: 0,
        bytes_uncertain: 0,
        bytes_estimated: 0,
        failures: Vec::new(),
        refused_items: plan.items.clone(),
        finished_unix: unix_now(),
        note: reason,
        volumes: Vec::new(),
    }
}

/// One sentence stating exactly what the receipt's numbers mean.
fn receipt_note(deleted: usize, removed: u64, uncertain: u64) -> String {
    if deleted == 0 {
        return "Nothing was removed. The figures below describe what was attempted, not what \
                was freed."
            .to_string();
    }
    let mut note = format!(
        "{} file{} confirmed removed. The {} figure is what those files occupied on disk before \
         they were deleted.",
        deleted,
        if deleted == 1 { "" } else { "s" },
        human_bytes(removed)
    );
    if uncertain > 0 {
        note.push_str(&format!(
            " {} of that is uncertain: the hard-link count could not be read, so the space may not \
             come back.",
            human_bytes(uncertain)
        ));
    }
    note.push_str(
        " The volume's free-space change is a separate observation of the whole drive; other \
         software writing at the same time affects it.",
    );
    note
}

/// True when `path` is one of the roots, or lies inside one.
pub fn is_under_any_root(path: &Path, roots: &[types::ScanRoot]) -> bool {
    roots.iter().any(|r| tiers::is_under(path, &r.path))
}

#[cfg(test)]
mod tests;
