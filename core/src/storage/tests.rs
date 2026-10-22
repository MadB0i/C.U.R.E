//! Tests for the storage intelligence engine.
//!
//! Every destructive test runs against a `tempfile` fixture. Nothing here
//! reads, writes or deletes anything outside a directory the test created and
//! the `TempDir` owns.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::*;

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

fn filler(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn ctx_for(base: &Path) -> tiers::TierContext {
    tiers::TierContext::synthetic(base)
}

fn quiet_progress(_: ScanProgress) {}

/// Walk one directory tree with permissive limits.
fn walk_fixture(
    root: &Path,
    class: StorageClass,
    ctx: &tiers::TierContext,
) -> (WalkSinkAlias, walk::RootStats) {
    let mut sink = WalkSinkAlias::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let limits = walk::WalkLimits {
        max_duration_ms: 30_000,
        ..Default::default()
    };
    let stats = walk::walk_root(
        walk::WalkRequest {
            root,
            class,
            ctx,
            limits: &limits,
            cancel: &cancel,
            scan_id: "test-scan",
            root_label: "fixture",
        },
        &mut sink.inner,
        &mut quiet_progress,
    );
    (sink, stats)
}

/// Borrowed alias so the test file does not depend on the sink's path.
struct WalkSinkAlias {
    inner: walk::WalkSink,
}

impl WalkSinkAlias {
    fn new() -> Self {
        WalkSinkAlias {
            inner: walk::WalkSink::new(),
        }
    }
}

/// A minimal but complete scan over one fixture tree, so plan and execute can
/// be tested without running the environment-driven catalogue.
fn synthetic_scan(base: &Path, ctx: &tiers::TierContext, class: StorageClass) -> StorageScan {
    let (sink, stats) = walk_fixture(base, class, ctx);
    let files = sink.inner.files;
    // The same union the engine uses, so the fixture and the engine agree on
    // what the headline means.
    let accounting = super::reclaim::compute(&files, &[]);
    StorageScan {
        scan_id: "test-scan".to_string(),
        started_unix: 0,
        finished_unix: 0,
        roots: vec![types::ScanRoot {
            path: base.to_path_buf(),
            class,
            is_catalogue: false,
            label: "fixture".to_string(),
        }],
        files,
        folders: sink.inner.folders,
        summaries: vec![],
        coverage: ScanCoverage {
            files_seen: stats.files_seen,
            dirs_seen: stats.dirs_seen,
            reparse_points_skipped: stats.outcome.reparse_points_skipped,
            permission_denied_dirs: stats.outcome.permission_denied_dirs,
            ..Default::default()
        },
        duplicate_coverage: Default::default(),
        large_files: vec![],
        large_folders: vec![],
        duplicates: vec![],
        abandoned: vec![],
        safe_reclaimable_bytes: accounting.safe_bytes,
        careful_reclaimable_bytes: accounting.careful_bytes,
        protected_bytes: accounting.protected_bytes,
        protected_only_bytes: accounting.protected_only_bytes,
        unknown_bytes: accounting.unknown_bytes,
        duplicate_reclaimable_bytes: accounting.duplicate_bytes,
        duplicate_bulk_reclaimable_bytes: accounting.duplicate_bulk_bytes,
        safe_file_count: accounting.safe_file_count,
        careful_file_count: accounting.careful_file_count,
        protected_file_count: accounting.protected_file_count,
        distinct_file_count: accounting.distinct_file_count,
    }
}

fn select(paths: &[PathBuf]) -> Vec<SelectionItem> {
    paths
        .iter()
        .map(|p| SelectionItem {
            path: p.clone(),
            expected_size: fs::symlink_metadata(p)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len()),
            bulk: false,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// reclaim accounting: the headline must never over-promise
// ---------------------------------------------------------------------------

/// Build a file record directly, so a test can describe a file that was
/// measured but never touched on disk.
fn record(path: &Path, bytes: u64, tier: SafetyTier, class: StorageClass) -> FileRecord {
    FileRecord {
        path: path.to_path_buf(),
        size_logical: bytes,
        size_allocated: Some(bytes),
        modified_unix: Some(1),
        accessed_unix: Some(1),
        created_unix: Some(1),
        extension: "bin".to_string(),
        hard_link_count: Some(1),
        is_reparse_point: false,
        is_read_only: false,
        is_hidden: false,
        is_system: false,
        class,
        tier,
        reasons: vec![],
    }
}

#[test]
fn the_headline_counts_a_file_once_even_when_two_views_see_it() {
    // The same file is both a "user temp" file and a verified duplicate copy.
    // The two views overlap; the headline must not add them together.
    let path = PathBuf::from(r"C:\Temp\a.bin");
    let file = record(
        &path,
        1000,
        SafetyTier::SafeToReview,
        StorageClass::UserTemp,
    );
    let copy = DuplicateCopy {
        path: path.clone(),
        size_logical: 1000,
        size_allocated: 1000,
        modified_unix: Some(1),
        hard_link_count: Some(1),
        tier: SafetyTier::SafeToReview,
        class: StorageClass::UserTemp,
        is_kept_candidate: false,
        reclaim_is_certain: true,
        reasons: vec![],
    };
    let set = DuplicateSet {
        id: "set".to_string(),
        size_logical: 1000,
        size_allocated: 1000,
        sha256: "0".repeat(64),
        copies: vec![copy],
        reclaimable_bytes: 1000,
        bulk_reclaimable_bytes: 1000,
        uncertain_copies: 0,
        protected_copies: 0,
    };

    let out = super::reclaim::compute(std::slice::from_ref(&file), std::slice::from_ref(&set));
    assert_eq!(
        out.safe_bytes, 1000,
        "the file's bytes must be counted once, not twice"
    );
    assert_eq!(out.duplicate_bytes, 1000);
    assert_eq!(out.duplicate_bulk_bytes, 1000);
    assert_eq!(out.distinct_file_count, 1);
}

#[test]
fn the_duplicate_figure_is_a_subset_of_the_headline_never_an_addition() {
    let mut files = Vec::new();
    let mut sets = Vec::new();
    for i in 0..5u64 {
        let path = PathBuf::from(format!(r"C:\Temp\dup{i}.bin"));
        files.push(record(
            &path,
            100 * (i + 1),
            SafetyTier::SafeToReview,
            StorageClass::UserTemp,
        ));
    }
    // One group holding two of the files.
    let copies: Vec<DuplicateCopy> = files[0..2]
        .iter()
        .map(|f| DuplicateCopy {
            path: f.path.clone(),
            size_logical: f.size_logical,
            size_allocated: f.size_allocated.unwrap_or(f.size_logical),
            modified_unix: f.modified_unix,
            hard_link_count: f.hard_link_count,
            tier: f.tier,
            class: f.class,
            is_kept_candidate: false,
            reclaim_is_certain: true,
            reasons: vec![],
        })
        .collect();
    sets.push(DuplicateSet {
        id: "g".to_string(),
        size_logical: 100,
        size_allocated: 100,
        sha256: "a".repeat(64),
        copies,
        reclaimable_bytes: 100,
        bulk_reclaimable_bytes: 100,
        uncertain_copies: 0,
        protected_copies: 0,
    });

    let out = super::reclaim::compute(&files, &sets);
    let total: u64 = files.iter().map(|f| f.reclaimable_bytes()).sum();
    assert_eq!(
        out.safe_bytes, total,
        "the safe figure is the union of the files"
    );
    assert!(
        out.duplicate_bytes <= out.safe_bytes,
        "the duplicate figure is a subset, never an addition"
    );
    assert_eq!(out.safe_bytes + out.careful_bytes, total);
}

#[test]
fn overlapping_roots_do_not_inflate_the_headline() {
    // The same identity measured twice must contribute its bytes once.
    let path = PathBuf::from(r"C:\Temp\same.bin");
    let a = record(&path, 500, SafetyTier::SafeToReview, StorageClass::UserTemp);
    let b = record(&path, 500, SafetyTier::SafeToReview, StorageClass::UserTemp);
    let out = super::reclaim::compute(&[a, b], &[]);
    assert_eq!(out.safe_bytes, 500);
    assert_eq!(out.distinct_file_count, 1);
}

#[test]
fn a_bulk_selection_refuses_items_that_need_individual_approval() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("user.bin");
    write(&file, &filler(2048, 5));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserSelected);
    // `UserSelected` is a `ReviewCarefully` class.
    assert_eq!(
        scan.files[0].tier,
        SafetyTier::ReviewCarefully,
        "the fixture must be individually-approved material"
    );

    let bulk = build_plan(
        &scan,
        &[SelectionItem {
            path: file.clone(),
            expected_size: None,
            bulk: true,
        }],
    );
    assert!(bulk.items.is_empty(), "a bulk action must not carry it");
    assert_eq!(bulk.refused.len(), 1);
    assert!(bulk.refused[0]
        .reasons
        .iter()
        .any(|r| r.contains("individual approval")));

    let individual = build_plan(
        &scan,
        &[SelectionItem {
            path: file,
            expected_size: None,
            bulk: false,
        }],
    );
    assert_eq!(individual.items.len(), 1, "an individual choice is allowed");
}

// ---------------------------------------------------------------------------
// plan binding: execution must run the plan that was approved
// ---------------------------------------------------------------------------

#[test]
fn a_plan_whose_contents_changed_after_approval_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let a = base.join("a.bin");
    let b = base.join("b.bin");
    write(&a, &filler(2048, 1));
    write(&b, &filler(2048, 2));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let mut plan = build_plan(&scan, &select(&[a.clone(), b.clone()]));
    assert_eq!(plan.items.len(), 2);

    // Something edits the approved plan before it is executed.
    plan.items.push(PlanItem {
        path: base.join("smuggled.bin"),
        size_logical: 1,
        size_allocated: 1,
        tier: SafetyTier::SafeToReview,
        class: StorageClass::UserTemp,
        reasons: vec![],
        stale: false,
        file_id: None,
        approved_size: 1,
    });

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(receipt.deleted, 0, "nothing may be removed");
    assert_eq!(receipt.attempted, 0);
    assert!(a.exists() && b.exists());
    assert!(receipt.note.contains("digest"));
}

#[test]
fn a_plan_from_another_scan_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let a = base.join("a.bin");
    write(&a, &filler(2048, 1));
    let ctx = ctx_for(tmp.path());
    let mut scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&a)));
    scan.scan_id = "a-different-scan".to_string();
    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(receipt.deleted, 0);
    assert!(a.exists());
    assert!(receipt.note.contains("different scan"));
}

#[test]
fn execute_refuses_a_file_that_was_replaced_after_approval() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let original = base.join("original.bin");
    let replacement = base.join("replacement.bin");
    write(&original, &filler(2048, 1));
    write(&replacement, &filler(2048, 9));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&original)));
    assert_eq!(plan.items.len(), 1);

    // The approved file is removed and a different file of exactly the same
    // size takes its place. Size alone cannot tell them apart; the recorded
    // file identity can.
    fs::remove_file(&original).unwrap();
    fs::rename(&replacement, &original).unwrap();

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(receipt.deleted, 0, "a different file must not be removed");
    assert!(original.exists(), "the substitute must survive");
    assert_eq!(receipt.refused_items.len(), 1);
}

// ---------------------------------------------------------------------------
// per-file classification
// ---------------------------------------------------------------------------

#[test]
fn a_build_directory_is_recognised_wherever_it_lives() {
    // The catalogue used to look only under the user's profile, so a project
    // on D:\ was invisible. The rule is about the directory, not the drive.
    for p in [
        r"D:\\Projects\\thing\\target\\debug\\app.exe",
        r"D:\\Projects\\thing\\node_modules\\pkg\\index.js",
        r"C:\\Users\\b\\src\\web\\.next\\build\\page.js",
    ] {
        let refined = tiers::refine_class(
            Path::new(p),
            StorageClass::UserSelected,
            Some(1),
            2_000_000_000,
            30,
        );
        assert_eq!(
            refined,
            StorageClass::DevCache,
            "{} is developer build output",
            p
        );
        assert_eq!(
            StorageClass::DevCache.floor_tier(),
            SafetyTier::ReviewCarefully,
            "build output needs individual approval, never a bulk action"
        );
    }
}

#[test]
fn an_old_download_is_labelled_by_its_age_and_nothing_else() {
    let now = 2_000_000_000u64;
    let ninety_days = now - 90 * 86_400;
    let refined = tiers::refine_class(
        Path::new(r"C:\\Users\\b\\Downloads\\holiday-photo.jpg"),
        StorageClass::UserSelected,
        Some(ninety_days),
        now,
        30,
    );
    assert_eq!(refined, StorageClass::OldDownload);
    assert_eq!(
        StorageClass::OldDownload.floor_tier(),
        SafetyTier::ReviewCarefully
    );

    // A download touched yesterday is not old.
    let fresh = tiers::refine_class(
        Path::new(r"C:\\Users\\b\\Downloads\\holiday-photo.jpg"),
        StorageClass::UserSelected,
        Some(now - 86_400),
        now,
        30,
    );
    assert_eq!(fresh, StorageClass::UserSelected);
}

#[test]
fn an_installer_in_downloads_is_recognised_as_an_installer() {
    let refined = tiers::refine_class(
        Path::new(r"C:\\Users\\b\\Downloads\\setup.exe"),
        StorageClass::UserSelected,
        Some(2_000_000_000 - 86_400),
        2_000_000_000,
        30,
    );
    assert_eq!(refined, StorageClass::OldInstaller);
    assert!(tiers::is_installer_extension("msi"));
    assert!(tiers::is_installer_extension("zip"));
    assert!(!tiers::is_installer_extension("pdf"));
    assert!(!tiers::is_installer_extension("jpg"));
}

#[test]
fn a_confident_catalogue_class_is_never_refined_away() {
    // A browser cache is a browser cache even when it sits in a folder
    // called `build`.
    let refined = tiers::refine_class(
        Path::new(r"C:\\Users\\b\\build\\Cache\\blob.bin"),
        StorageClass::BrowserCache,
        Some(1),
        2_000_000_000,
        30,
    );
    assert_eq!(refined, StorageClass::BrowserCache);
}

// ---------------------------------------------------------------------------
// classification
// ---------------------------------------------------------------------------

#[test]
fn the_windows_update_cache_is_never_bulk_recommended() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    for p in [
        r"C:\Windows\SoftwareDistribution\Download\update.dat",
        r"C:\WINDOWS\SoftwareDistribution\Download\abc.exe",
    ] {
        let c = tiers::classify(Path::new(p), StorageClass::WindowsUpdateCache, &ctx);
        assert_eq!(
            c.tier,
            SafetyTier::ReviewCarefully,
            "{} must need individual approval",
            p
        );
        assert!(
            !c.tier.allows_bulk_recommendation(),
            "{} must never be bulk-recommended",
            p
        );
        assert!(
            c.reasons.iter().any(|r| r.contains("Temporary files")),
            "{} must name the supported mechanism",
            p
        );
    }
}

#[test]
fn a_firefox_cache_is_recognised_as_a_cache() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    for p in [
        r"C:\Users\b\AppData\Local\Mozilla\Firefox\Profiles\abc.default\cache2",
        r"C:\Users\b\AppData\Roaming\Mozilla\Firefox\Profiles\abc.default\cache2",
    ] {
        let c = tiers::classify(Path::new(p), StorageClass::BrowserCache, &ctx);
        assert_eq!(
            c.tier,
            SafetyTier::SafeToReview,
            "{} is a Firefox cache and must be recognised",
            p
        );
    }
}

// ---------------------------------------------------------------------------
// the activity guard
// ---------------------------------------------------------------------------

#[test]
fn a_temporary_file_written_moments_ago_is_not_offered_as_safe() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let now = 1_700_000_000u64;
    let mut c = tiers::classify(
        Path::new(r"C:\Users\b\AppData\Local\Temp\live.tmp"),
        StorageClass::UserTemp,
        &ctx,
    );
    assert_eq!(c.tier, SafetyTier::SafeToReview);
    tiers::apply_activity_guard(
        &mut c,
        StorageClass::UserTemp,
        Some(now - 60),
        now,
        3600,
        false,
    );
    assert_eq!(
        c.tier,
        SafetyTier::ReviewCarefully,
        "a file written a minute ago is probably in use"
    );
    assert!(
        c.reasons.iter().any(|r| r.contains("moments ago")),
        "the reason must be stated: {:?}",
        c.reasons
    );
}

#[test]
fn an_old_temporary_file_is_still_offered_as_safe() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let now = 1_700_000_000u64;
    let mut c = tiers::classify(
        Path::new(r"C:\Users\b\AppData\Local\Temp\old.tmp"),
        StorageClass::UserTemp,
        &ctx,
    );
    tiers::apply_activity_guard(
        &mut c,
        StorageClass::UserTemp,
        Some(now - 86_400 * 3),
        now,
        3600,
        false,
    );
    assert_eq!(
        c.tier,
        SafetyTier::SafeToReview,
        "a three-day-old temporary file is a leftover, not something in use"
    );
}

#[test]
fn a_temporary_file_held_open_by_another_program_is_not_offered_as_safe() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let now = 1_700_000_000u64;
    let mut c = tiers::classify(
        Path::new(r"C:\Users\b\AppData\Local\Temp\held.tmp"),
        StorageClass::UserTemp,
        &ctx,
    );
    tiers::apply_activity_guard(
        &mut c,
        StorageClass::UserTemp,
        Some(now - 86_400 * 3),
        now,
        3600,
        true,
    );
    assert_eq!(
        c.tier,
        SafetyTier::ReviewCarefully,
        "a file another program is holding open is in use right now"
    );
    assert!(
        c.reasons.iter().any(|r| r.contains("held open")),
        "the reason must be stated: {:?}",
        c.reasons
    );
}

#[test]
fn the_activity_guard_never_reaches_a_class_that_is_not_regenerable() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let now = 1_700_000_000u64;
    // A download in the user's own folder is never a regenerable leftover,
    // and the guard must not turn it into one either way.
    let mut c = tiers::classify(
        Path::new(r"C:\Users\b\Downloads\thing.zip"),
        StorageClass::UserSelected,
        &ctx,
    );
    let before = c.tier;
    tiers::apply_activity_guard(
        &mut c,
        StorageClass::UserSelected,
        Some(now - 10),
        now,
        3600,
        true,
    );
    assert_eq!(c.tier, before, "the guard only ever makes a tier stricter");
}

#[test]
fn the_activity_guard_only_ever_makes_a_tier_stricter() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let now = 1_700_000_000u64;
    for class in StorageClass::ALL {
        let mut c = tiers::classify(
            Path::new(r"C:\Users\b\AppData\Local\Temp\x.tmp"),
            class,
            &ctx,
        );
        let before = c.tier;
        tiers::apply_activity_guard(&mut c, class, Some(now - 10), now, 3600, true);
        let stricter = matches!(
            (before, c.tier),
            (SafetyTier::SafeToReview, SafetyTier::ReviewCarefully)
                | (SafetyTier::SafeToReview, SafetyTier::Unknown)
                | (SafetyTier::SafeToReview, SafetyTier::Protected)
                | (SafetyTier::ReviewCarefully, SafetyTier::Unknown)
                | (SafetyTier::ReviewCarefully, SafetyTier::Protected)
        );
        assert!(
            stricter || before == c.tier,
            "{:?} went from {:?} to {:?}, which is not stricter",
            class,
            before,
            c.tier
        );
    }
}

#[cfg(windows)]
fn open_exclusive(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}

/// Age a file so the recency guard does not fire for it.
///
/// A fixture that was written a moment ago would be downgraded by the
/// guard, which is the point of the guard and not what these tests are
/// about. The tests that *do* want the guard leave the mtime alone.
#[cfg(windows)]
fn age_file(path: &Path, days: u64) {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{FILETIME, HANDLE};
    use windows::Win32::Storage::FileSystem::SetFileTime;

    let handle = std::fs::OpenOptions::new()
        .write(true)
        .share_mode(0)
        .open(path)
        .unwrap();
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - (days as i64) * 86_400;
    // A FILETIME counts 100-nanosecond intervals since 1601-01-01.
    let ticks = (secs + 11_644_473_600) as u64 * 10_000_000;
    let stamp = FILETIME {
        dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    // SAFETY: the handle is live for the call and the time structure is a
    // correctly sized out-parameter.
    unsafe {
        let _ = SetFileTime(HANDLE(handle.as_raw_handle()), None, None, Some(&stamp));
    }
}

#[test]
fn a_freshly_written_temporary_file_is_never_offered_as_safe() {
    // The whole path, not just the guard function: the walk measures it, the
    // headline counts it as careful, and the plan refuses to carry it in a
    // bulk action.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("live.tmp");
    write(&file, &filler(4096, 9));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);

    assert_eq!(
        scan.files[0].tier,
        SafetyTier::ReviewCarefully,
        "a temporary file written a moment ago must not be measured as safe"
    );
    assert_eq!(
        scan.safe_reclaimable_bytes, 0,
        "a file that is probably in use must not be counted as safe to remove"
    );
    assert!(
        scan.careful_reclaimable_bytes > 0,
        "it is still counted, as something that needs individual approval"
    );

    let bulk = build_plan(
        &scan,
        &[SelectionItem {
            path: file.clone(),
            expected_size: None,
            bulk: true,
        }],
    );
    assert!(
        bulk.items.is_empty(),
        "a bulk action must not carry a file that may be in use"
    );
    assert_eq!(bulk.refused.len(), 1);
    assert!(
        bulk.refused[0]
            .reasons
            .iter()
            .any(|r| r.contains("individual approval")),
        "the refusal must say why: {:?}",
        bulk.refused[0].reasons
    );
}

#[cfg(windows)]
#[test]
fn a_file_locked_after_the_plan_was_approved_is_refused_at_execution() {
    // The gap the guard exists to close: the file was old and idle when it
    // was measured and approved, and a program opened it in between.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("taken.tmp");
    write(&file, &filler(4096, 11));
    age_file(&file, 30);
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    assert_eq!(
        scan.files[0].tier,
        SafetyTier::SafeToReview,
        "an old, idle temporary file is a leftover and may be offered"
    );

    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));
    assert_eq!(
        plan.items.len(),
        1,
        "the plan must carry it before it is taken"
    );

    // Now a program opens it with no sharing at all.
    let handle = open_exclusive(&file);

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(
        receipt.deleted, 0,
        "a file another program is holding open must not be removed"
    );
    assert_eq!(receipt.refused, 1, "it must be refused, and said so");
    assert!(
        file.exists(),
        "the file must still be there: it was never removed"
    );
    assert!(
        receipt
            .refused_items
            .iter()
            .any(|r| r.reasons.iter().any(|x| x.contains("held open"))),
        "the refusal must say the file is in use: {:?}",
        receipt.refused_items
    );
    drop(handle);
}

#[cfg(windows)]
#[test]
fn a_file_already_locked_is_downgraded_in_the_plan() {
    // The preview stage: a file that is in use right now must not be
    // presented as a safe regenerable leftover.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("busy.tmp");
    write(&file, &filler(4096, 13));
    age_file(&file, 30);
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    assert_eq!(scan.files[0].tier, SafetyTier::SafeToReview);

    let handle = open_exclusive(&file);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));
    assert_eq!(plan.items.len(), 1, "it may still be removed one at a time");
    assert_eq!(
        plan.items[0].tier,
        SafetyTier::ReviewCarefully,
        "a file that is in use right now is not a safe regenerable leftover"
    );
    assert!(
        plan.items[0]
            .reasons
            .iter()
            .any(|r| r.contains("held open")),
        "the plan must say why: {:?}",
        plan.items[0].reasons
    );
    drop(handle);
}

#[cfg(windows)]
#[test]
fn a_locked_temporary_file_is_reported_as_in_use() {
    // The probe asks the kernel, not the clock: a file another process is
    // holding open is in use, and one that is not is not.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("held.tmp");
    write(&path, b"x");
    assert!(
        !fsx::is_in_use(&path),
        "a file nobody holds open must not be reported as in use"
    );
    let handle = open_exclusive(&path);
    assert!(
        fsx::is_in_use(&path),
        "a file held open must be reported as in use"
    );
    drop(handle);
    assert!(
        !fsx::is_in_use(&path),
        "once the handle is released the file is not in use"
    );
}

#[test]
fn an_unknown_hard_link_count_promises_no_bytes() {
    // A link count that could not be read is not evidence that the file has
    // only one name, so no reclaim may be promised for it.
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    write(&a, &filler(4096, 1));
    write(&b, &filler(4096, 1));
    let mk = |p: &Path| duplicates::Candidate {
        path: p.to_path_buf(),
        size_logical: 4096,
        modified_unix: Some(1),
        hard_link_count: None,
        class: StorageClass::UserTemp,
        tier: SafetyTier::SafeToReview,
    };
    let ctx = tiers::TierContext::synthetic(dir.path());
    let set = duplicates::build_set_for_test("a".repeat(64).as_str(), &[mk(&a), mk(&b)], &ctx);
    assert_eq!(set.reclaimable_bytes, 0, "no bytes may be promised");
    assert_eq!(set.uncertain_copies, 1);
    assert!(set.copies[1]
        .reasons
        .iter()
        .any(|r| r.contains("hard-link count could not be read")));
}

#[test]
fn the_volume_probe_reads_the_volume_root_not_a_file_path() {
    // `GetDiskFreeSpaceExW` takes a directory root. Handing it a file path
    // fails with ERROR_DIRECTORY, which is what made every receipt report an
    // unreadable volume.
    let root = fsx::volume_root(Path::new(r"C:\Users\b\Downloads\file.txt"));
    assert_eq!(root, Some(PathBuf::from("C:\\")));
    let space = fsx::volume_space(Path::new(r"C:\Users\b\Downloads\file.txt"));
    assert!(
        space.is_some(),
        "the volume of a file path must resolve through its volume root"
    );
}

// ---------------------------------------------------------------------------
// tiers
// ---------------------------------------------------------------------------

#[test]
fn windows_core_paths_are_protected() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    for p in [
        r"C:\Windows\System32\drivers\etc\hosts",
        r"C:\Windows\WinSxS\amd64_manifest\1.2.3.4",
        r"C:\Windows\Installer\abc.msi",
        r"C:\Windows\System32\Config\SAM",
        r"C:\Windows\assembly\thing.dll",
    ] {
        let c = tiers::classify(Path::new(p), StorageClass::Other, &ctx);
        assert_eq!(c.tier, SafetyTier::Protected, "{p} -> {:?}", c.reasons);
        assert!(
            c.reasons.iter().any(|r| r.contains("required by Windows")),
            "{p} lost its explanation: {:?}",
            c.reasons
        );
    }
}

#[test]
fn installed_apps_and_system_volumes_are_protected() {
    // The synthetic context roots every protected area under one base, so the
    // fixture paths must too — otherwise this would be testing the wrong tree.
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    for p in [
        r"C:\fixture\Program Files\App\app.exe",
        r"C:\fixture\Program Files (x86)\App\app.exe",
        r"C:\fixture\ProgramData\Vendor\data.bin",
        r"C:\fixture\System Volume Information\tracking.log",
        r"C:\fixture\Recovery\Backup.wbadmin",
    ] {
        let c = tiers::classify(Path::new(p), StorageClass::Other, &ctx);
        assert_eq!(c.tier, SafetyTier::Protected, "{p} -> {:?}", c.reasons);
    }
}

#[test]
fn a_program_data_cache_is_not_confused_with_shared_machine_state() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let cache = tiers::classify(
        Path::new(r"C:\fixture\ProgramData\Package Cache\installer.msi"),
        StorageClass::Other,
        &ctx,
    );
    // Even a recognised cache directory is not auto-deletable just for being
    // one: installers live here and removal breaks repair/uninstall.
    assert!(
        matches!(cache.tier, SafetyTier::Protected | SafetyTier::Unknown),
        "{:?}",
        cache.reasons
    );
}

#[test]
fn system_file_names_are_protected_regardless_of_folder() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    // Even inside a genuine temp folder, a pagefile is not a candidate.
    let c = tiers::classify(
        Path::new(r"C:\Users\bob\AppData\Local\Temp\pagefile.sys"),
        StorageClass::UserTemp,
        &ctx,
    );
    assert_eq!(c.tier, SafetyTier::Protected, "{:?}", c.reasons);
}

#[test]
fn recycle_bin_metadata_is_protected_but_payload_is_not() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let payload = tiers::classify(
        Path::new(r"C:\$Recycle.Bin\S-1-5-21-1\$RSELF7X.tmp"),
        StorageClass::RecycleBin,
        &ctx,
    );
    assert_eq!(payload.tier, SafetyTier::SafeToReview);

    // `$I` is the shell's index record, not the deleted bytes. It is
    // untouchable: removing it corrupts the Recycle Bin's own bookkeeping.
    let index = tiers::classify(
        Path::new(r"C:\$Recycle.Bin\S-1-5-21-1\$ISELF7X.tmp"),
        StorageClass::RecycleBin,
        &ctx,
    );
    assert_eq!(index.tier, SafetyTier::Protected, "{:?}", index.reasons);
}

#[test]
fn browser_cache_matches_only_cache_directories() {
    let cache = r"c:/users/bob/appdata/local/google/chrome/user data/default/cache";
    assert!(tiers::is_browser_cache_path(cache));
    assert!(tiers::is_browser_cache_path(
        r"c:/users/bob/appdata/local/google/chrome/user data/default/code cache"
    ));

    // Real user data that must never be swept away as "cache".
    for protected in [
        r"c:/users/bob/appdata/local/google/chrome/user data/default/cookies",
        r"c:/users/bob/appdata/local/google/chrome/user data/default/history",
        r"c:/users/bob/appdata/local/google/chrome/user data/default/bookmarks",
        r"c:/users/bob/appdata/local/google/chrome/user data/default/login data",
        r"c:/users/bob/appdata/local/google/chrome/user data/default/local storage",
        // A Firefox *profile* directory holds the user's real profile data.
        // The cache inside it is fine; the profile itself is not.
        r"c:/users/bob/appdata/roaming/mozilla/firefox/profiles/abc.default",
    ] {
        assert!(
            !tiers::is_browser_cache_path(protected),
            "{protected} must not be treated as cache"
        );
    }
}

#[test]
fn unrecognised_path_inside_a_profile_is_unknown_not_safe() {
    let ctx = ctx_for(Path::new(r"C:\fixture"));
    let c = tiers::classify(
        Path::new(r"C:\Users\bob\AppData\Local\Google\Chrome\User Data\Default\Sync Data"),
        StorageClass::BrowserCache,
        &ctx,
    );
    assert_eq!(c.tier, SafetyTier::Unknown, "{:?}", c.reasons);
}

#[test]
fn abandoned_data_is_never_recommended() {
    let ctx = ctx_for(Path::new(r"C:\fixture"));
    let c = tiers::classify(
        Path::new(r"C:\Users\bob\AppData\Local\SomeApp\data.bin"),
        StorageClass::AbandonedAppData,
        &ctx,
    );
    assert_eq!(c.tier, SafetyTier::Unknown);
    assert!(!c.tier.allows_deletion_request());
    assert!(!c.tier.allows_bulk_recommendation());
    assert!(c
        .reasons
        .iter()
        .any(|r| r.contains("age alone proves nothing")));
}

#[test]
fn only_safe_to_review_may_be_bulk_recommended() {
    assert!(SafetyTier::SafeToReview.allows_bulk_recommendation());
    assert!(!SafetyTier::ReviewCarefully.allows_bulk_recommendation());
    assert!(!SafetyTier::Protected.allows_bulk_recommendation());
    assert!(!SafetyTier::Unknown.allows_bulk_recommendation());

    // ReviewCarefully can be *requested*, but only item by item.
    assert!(SafetyTier::ReviewCarefully.allows_deletion_request());
    assert!(!SafetyTier::Protected.allows_deletion_request());
    assert!(!SafetyTier::Unknown.allows_deletion_request());
}

#[test]
fn component_matching_respects_directory_boundaries() {
    // A folder that merely starts with a protected name is not inside it.
    assert!(!tiers::is_under(
        Path::new(r"C:\Program FilesX\App"),
        Path::new(r"C:\Program Files")
    ));
    assert!(tiers::is_under(
        Path::new(r"C:\Program Files\App"),
        Path::new(r"C:\Program Files")
    ));
    assert!(tiers::is_under(
        Path::new(r"C:\Program Files"),
        Path::new(r"C:\Program Files")
    ));
    assert!(!tiers::is_under(Path::new(r"C:\Windows"), Path::new("")));
}

// ---------------------------------------------------------------------------
// walker
// ---------------------------------------------------------------------------

#[test]
fn walk_measures_every_file_and_never_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("cache");
    write(&root.join("a.bin"), &filler(100, 1));
    write(&root.join("sub/b.bin"), &filler(200, 2));
    write(&root.join("sub/deep/c.bin"), &filler(300, 3));

    let ctx = ctx_for(tmp.path());
    let (sink, stats) = walk_fixture(&root, StorageClass::UserTemp, &ctx);
    let names: Vec<String> = sink
        .inner
        .files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(stats.files_seen, 3);
    assert_eq!(stats.bytes_logical, 600);
    /* These files were written a moment ago, so the engine must not treat
    them as regenerable leftovers: something may still be using them.
    They are measured, but they need individual approval. */
    assert!(sink
        .inner
        .files
        .iter()
        .all(|f| f.tier == SafetyTier::ReviewCarefully));
    assert!(sink
        .inner
        .files
        .iter()
        .all(|f| f.reasons.iter().any(|r| r.contains("moments ago"))));
}

#[test]
fn a_temporary_file_written_long_ago_is_still_a_regenerable_leftover() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("cache");
    let file = root.join("old.bin");
    write(&file, &filler(100, 1));
    /* Age the file beyond the "in use" window. */
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
    let times = std::fs::FileTimes::new().set_modified(old);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(times)
        .unwrap();

    let ctx = ctx_for(tmp.path());
    let (sink, _stats) = walk_fixture(&root, StorageClass::UserTemp, &ctx);
    assert_eq!(sink.inner.files.len(), 1);
    assert_eq!(
        sink.inner.files[0].tier,
        SafetyTier::SafeToReview,
        "a leftover that is a day old is a regenerable leftover"
    );
}

#[cfg(windows)]
#[test]
fn walk_skips_links_so_they_cannot_escape_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("cache");
    let outside = tmp.path().join("outside");
    write(&root.join("inside.bin"), &filler(10, 1));
    write(&outside.join("secret.bin"), &filler(999, 9));

    if std::os::windows::fs::symlink_file(outside.join("secret.bin"), root.join("link.bin"))
        .is_err()
    {
        return; // No symlink privilege on this host; nothing to prove.
    }

    let ctx = ctx_for(tmp.path());
    let (sink, stats) = walk_fixture(&root, StorageClass::UserTemp, &ctx);
    assert_eq!(sink.inner.files.len(), 1, "link must not be followed");
    assert_eq!(stats.outcome.reparse_points_skipped, 1);
    /* A link that was deliberately not followed is not a read failure. It
    is recorded separately, so one harmless link cannot downgrade an
    otherwise complete measurement. */
    assert!(stats
        .skipped_links
        .iter()
        .any(|f| f.reason.contains("not followed")));
    assert!(!stats
        .failures
        .iter()
        .any(|f| f.reason.contains("not followed")));
    assert!(outside.join("secret.bin").exists(), "target must survive");
}

#[cfg(windows)]
#[test]
fn walk_survives_a_self_referential_link_without_looping() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    write(&root.join("a.bin"), &filler(10, 1));
    let inner = root.join("nested");
    fs::create_dir_all(&inner).unwrap();

    // A junction pointing at an ancestor is the classic infinite-walk trap.
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(inner.join("loop"))
        .arg(&root)
        .output();
    if made.as_ref().map(|o| !o.status.success()).unwrap_or(true) {
        return; // Junction creation unavailable; skip rather than pretend.
    }

    let ctx = ctx_for(tmp.path());
    let (sink, stats) = walk_fixture(&root, StorageClass::UserTemp, &ctx);
    assert!(
        sink.inner.files.len() <= 2,
        "walk followed a loop: {:?}",
        sink.inner
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
    );
    assert!(stats.outcome.reparse_points_skipped >= 1);
}

#[test]
fn walk_reports_instead_of_failing_when_a_directory_is_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    write(&root.join("readable.bin"), &filler(10, 1));
    let locked = root.join("locked");
    fs::create_dir_all(&locked).unwrap();
    write(&locked.join("hidden.bin"), &filler(20, 2));

    #[cfg(windows)]
    {
        // A directory opened with no sharing cannot be enumerated.
        use std::os::windows::fs::OpenOptionsExt;
        let _handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(0x02000000) // FILE_FLAG_BACKUP_SEMANTICS
            .open(&locked)
            .expect("fixture must be able to lock the directory");

        let ctx = ctx_for(tmp.path());
        let (sink, stats) = walk_fixture(&root, StorageClass::UserTemp, &ctx);
        assert!(
            sink.inner
                .files
                .iter()
                .all(|f| f.path.ends_with("readable.bin")),
            "walk stopped instead of continuing"
        );
        assert!(
            !stats.failures.is_empty(),
            "an unreadable directory must be reported, not silently skipped"
        );
        assert!(
            stats
                .failures
                .iter()
                .any(|f| f.reason.contains("locked") || f.reason.contains("denied")),
            "the OS reason must reach the coverage report: {:?}",
            stats.failures
        );
    }
    #[cfg(not(windows))]
    {
        let _ = locked;
        let ctx = ctx_for(tmp.path());
        let (sink, _) = walk_fixture(&root, StorageClass::UserTemp, &ctx);
        assert_eq!(sink.inner.files.len(), 2);
    }
}

#[test]
fn walk_honours_cancellation_immediately() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    for i in 0..200 {
        write(&root.join(format!("f{i}.bin")), &filler(10, i as u8));
    }
    let ctx = ctx_for(tmp.path());
    let cancel = Arc::new(AtomicBool::new(true));
    let mut sink = walk::WalkSink::new();
    let stats = walk::walk_root(
        walk::WalkRequest {
            root: &root,
            class: StorageClass::UserTemp,
            ctx: &ctx,
            limits: &walk::WalkLimits::default(),
            cancel: &cancel,
            scan_id: "cancel-test",
            root_label: "fixture",
        },
        &mut sink,
        &mut quiet_progress,
    );
    assert!(stats.outcome.cancelled);
    assert!(sink.files.is_empty(), "nothing should have been read");
}

#[test]
fn walk_reports_hitting_its_entry_limit_instead_of_truncating_silently() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    for i in 0..50 {
        write(&root.join(format!("f{i}.bin")), &filler(10, i as u8));
    }
    let ctx = ctx_for(tmp.path());
    let cancel = Arc::new(AtomicBool::new(false));
    let mut sink = walk::WalkSink::new();
    let stats = walk::walk_root(
        walk::WalkRequest {
            root: &root,
            class: StorageClass::UserTemp,
            ctx: &ctx,
            limits: &walk::WalkLimits {
                max_entries: 10,
                ..Default::default()
            },
            cancel: &cancel,
            scan_id: "limit-test",
            root_label: "fixture",
        },
        &mut sink,
        &mut quiet_progress,
    );
    assert!(stats.outcome.hit_entry_limit);
    assert_eq!(stats.outcome.limit_reason(), Some("entry limit reached"));
}

#[test]
fn a_deep_tree_does_not_overflow_the_stack() {
    // Depth is walked iteratively; 400 levels would blow a recursive walker.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("deep");
    let mut at = root.clone();
    for i in 0..400 {
        at = at.join(format!("d{i}"));
    }
    write(&at.join("leaf.bin"), &filler(16, 7));

    let ctx = ctx_for(tmp.path());
    let cancel = Arc::new(AtomicBool::new(false));
    let mut sink = walk::WalkSink::new();
    let stats = walk::walk_root(
        walk::WalkRequest {
            root: &root,
            class: StorageClass::UserTemp,
            ctx: &ctx,
            limits: &walk::WalkLimits::default(),
            cancel: &cancel,
            scan_id: "deep-test",
            root_label: "fixture",
        },
        &mut sink,
        &mut quiet_progress,
    );
    // Depth limit is reached and reported, not silently truncated.
    assert!(stats.outcome.hit_depth_limit);
    assert!(sink.files.iter().all(|f| f.path.ends_with("leaf.bin")));
}

#[test]
fn scanning_never_writes_to_the_tree_it_measures() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    write(&root.join("a.bin"), &filler(100, 1));
    write(&root.join("sub/b.bin"), &filler(200, 2));

    let snapshot = |p: &Path| -> Vec<(PathBuf, u64, Option<u64>)> {
        let mut v: Vec<_> = walkdir::WalkDir::new(p)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .map(|e| {
                let md = e.metadata().ok();
                (
                    e.path().to_path_buf(),
                    md.as_ref().map(|m| m.len()).unwrap_or(0),
                    md.as_ref().and_then(|m| fsx::unix_seconds(m.modified())),
                )
            })
            .collect();
        v.sort();
        v
    };

    let before = snapshot(&root);
    let ctx = ctx_for(tmp.path());
    let _ = walk_fixture(&root, StorageClass::UserTemp, &ctx);
    let after = snapshot(&root);
    assert_eq!(before, after, "a read-only scan mutated the tree");
}

// ---------------------------------------------------------------------------
// duplicates
// ---------------------------------------------------------------------------

fn candidates_from(paths: &[PathBuf]) -> Vec<duplicates::Candidate> {
    paths
        .iter()
        .map(|p| {
            let md = fs::symlink_metadata(p).unwrap();
            duplicates::Candidate {
                path: p.clone(),
                size_logical: md.len(),
                modified_unix: fsx::unix_seconds(md.modified()),
                hard_link_count: fsx::hard_link_count(p),
                class: StorageClass::UserTemp,
                tier: SafetyTier::SafeToReview,
            }
        })
        .collect()
}

fn find_dupes(paths: &[PathBuf], ctx: &tiers::TierContext) -> Vec<types::DuplicateSet> {
    let cancel = Arc::new(AtomicBool::new(false));
    let (sets, _) = duplicates::find_duplicates(
        &candidates_from(paths),
        ctx,
        &duplicates::DuplicateOptions::default(),
        &cancel,
        "dupe-test",
        |_| {},
    );
    sets
}

#[test]
fn identical_content_is_reported_as_verified_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let payload = filler(4096, 5);
    let a = tmp.path().join("a.bin");
    let b = tmp.path().join("b.bin");
    let c = tmp.path().join("c.bin");
    write(&a, &payload);
    write(&b, &payload);
    write(&c, &filler(4096, 6)); // same size, different content

    let ctx = ctx_for(tmp.path());
    let sets = find_dupes(&[a.clone(), b.clone(), c.clone()], &ctx);
    assert_eq!(
        sets.len(),
        1,
        "expected exactly one verified group: {sets:?}"
    );
    let set = &sets[0];
    assert_eq!(set.copies.len(), 2, "the differing file must not join");
    assert!(set.sha256.len() == 64);
    assert_eq!(
        set.copies.iter().filter(|x| x.is_kept_candidate).count(),
        1,
        "exactly one copy is kept"
    );
    assert!(set.reclaimable_bytes > 0);
}

#[test]
fn same_size_different_content_is_never_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for i in 0..6u8 {
        // Identical length, distinct leading byte: defeats the partial stage.
        let mut body = filler(8192, 0);
        body[0] = i;
        body[8191] = i;
        let p = tmp.path().join(format!("f{i}.bin"));
        write(&p, &body);
        paths.push(p);
    }
    let ctx = ctx_for(tmp.path());
    let sets = find_dupes(&paths, &ctx);
    assert!(sets.is_empty(), "size alone must never prove duplication");
}

#[test]
fn unique_files_produce_no_groups() {
    let tmp = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for i in 0..8u8 {
        let p = tmp.path().join(format!("f{i}.bin"));
        write(&p, &filler(2048, i));
        paths.push(p);
    }
    let ctx = ctx_for(tmp.path());
    assert!(find_dupes(&paths, &ctx).is_empty());
}

#[cfg(windows)]
#[test]
fn hardlinked_names_are_not_reported_as_reclaimable_duplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.bin");
    let b = tmp.path().join("b.bin");
    let payload = filler(4096, 3);
    write(&a, &payload);
    if fs::hard_link(&a, &b).is_err() {
        return;
    }
    let ctx = ctx_for(tmp.path());
    let sets = find_dupes(&[a, b], &ctx);
    assert!(
        sets.is_empty(),
        "removing a hard link frees nothing, so it must not be offered: {sets:?}"
    );
}

#[test]
fn a_file_that_changes_during_hashing_is_dropped() {
    // The snapshot-before/after check is what makes a hash meaningful.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("moving.bin");
    write(&path, &filler(64, 1));
    let before = duplicates::snapshot(&path);
    write(&path, &filler(128, 2));
    let after = duplicates::snapshot(&path);
    assert_ne!(before, after, "the change must be observable");
}

#[test]
fn the_kept_copy_is_the_one_outside_a_disposable_location() {
    let base = Path::new(r"C:\fixture");
    let ctx = ctx_for(base);
    let make = |path: &str, modified: u64, tier: SafetyTier| duplicates::Candidate {
        path: PathBuf::from(path),
        size_logical: 2048,
        modified_unix: Some(modified),
        hard_link_count: Some(1),
        class: StorageClass::UserTemp,
        tier,
    };
    // Older copy sits in a temp folder; newer copy in a user folder. The
    // newer, non-disposable one is the one to keep.
    let set = duplicates::build_set_for_test(
        "deadbeefdeadbeef",
        &[
            make(r"C:\fixture\Temp\old.bin", 100, SafetyTier::SafeToReview),
            make(
                r"C:\fixture\Documents\new.bin",
                900,
                SafetyTier::ReviewCarefully,
            ),
        ],
        &ctx,
    );
    let kept = set.copies.iter().find(|c| c.is_kept_candidate).unwrap();
    assert!(kept.path.ends_with("new.bin"), "kept {:?}", kept.path);
}

#[test]
fn duplicate_coverage_states_what_was_actually_checked() {
    let report = duplicates::DuplicateReport {
        files_considered: 1234,
        ..Default::default()
    };
    assert!(report.coverage_sentence().contains("1234"));

    let truncated = duplicates::DuplicateReport {
        files_considered: 10,
        truncated: true,
        reason: "cancelled".to_string(),
        ..Default::default()
    };
    assert!(truncated.coverage_sentence().starts_with("Partial"));
}

// ---------------------------------------------------------------------------
// plan and execute
// ---------------------------------------------------------------------------

#[test]
fn plan_refuses_protected_and_unknown_items() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let ctx = ctx_for(tmp.path());
    let ok_file = base.join("Temp").join("ok.bin");
    write(&ok_file, &filler(100, 1));
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);

    let plan = build_plan(&scan, &select(std::slice::from_ref(&ok_file)));
    assert_eq!(plan.items.len(), 1);
    assert!(plan.refused.is_empty());
    assert!(plan.estimated_bytes > 0);
    assert!(
        plan.consequence.contains("direct delete"),
        "{}",
        plan.consequence
    );
    assert!(plan.consequence.contains("no undo"), "{}", plan.consequence);
}

#[test]
fn plan_refuses_paths_outside_the_scanned_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let outside = tmp.path().join("outside.bin");
    write(&base.join("a.bin"), &filler(10, 1));
    write(&outside, &filler(10, 2));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);

    let plan = build_plan(&scan, &select(std::slice::from_ref(&outside)));
    assert!(plan.items.is_empty());
    assert_eq!(plan.refused.len(), 1);
}

#[test]
fn plan_marks_a_file_that_changed_size_as_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("a.bin");
    write(&file, &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);

    write(&file, &filler(999, 2)); // replaced after the scan
    let plan = build_plan(
        &scan,
        &[SelectionItem {
            path: file.clone(),
            expected_size: Some(100),
            bulk: false,
        }],
    );
    assert!(plan.items.is_empty());
    assert_eq!(plan.refused.len(), 1);
}

#[test]
fn execute_removes_only_validated_items_and_reports_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let a = base.join("a.bin");
    let b = base.join("b.bin");
    write(&a, &filler(4096, 1));
    write(&b, &filler(8192, 2));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(&[a.clone(), b.clone()]));
    assert_eq!(plan.items.len(), 2);

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(receipt.attempted, 2);
    assert_eq!(receipt.deleted, 2);
    assert_eq!(receipt.failed, 0);
    assert!(!a.exists() && !b.exists());
    assert!(receipt.bytes_removed_estimate > 0);
    assert!(receipt.bytes_removed_estimate <= receipt.bytes_estimated);
    assert!(receipt.note.contains("occupied"));
}

#[test]
fn execute_refuses_a_file_that_grew_after_the_plan_was_approved() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let a = base.join("a.bin");
    let victim = base.join("victim.bin");
    write(&a, &filler(100, 1));
    write(&victim, &filler(200, 2));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(&[a.clone(), victim.clone()]));
    assert_eq!(plan.items.len(), 2);

    // The target is replaced with something bigger between plan and execute.
    write(&victim, &filler(9999, 3));
    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));

    assert!(victim.exists(), "a changed file must survive");
    assert!(!a.exists(), "the untouched item should still be removed");
    assert_eq!(receipt.deleted, 1);
    assert_eq!(receipt.refused_items.len(), 1);
    assert!(receipt.refused_items[0]
        .reasons
        .iter()
        .any(|r| r.contains("changed size")));
}

#[test]
fn execute_refuses_a_target_that_became_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let target = base.join("target.bin");
    let link = base.join("planned.bin");
    write(&target, &filler(100, 1));
    write(&link, &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&link)));
    assert_eq!(plan.items.len(), 1);

    fs::remove_file(&link).unwrap();
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(&target, &link).is_err() {
            return;
        }
        let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
        assert!(target.exists(), "the link target must survive");
        assert_eq!(receipt.deleted, 0);
        assert!(receipt
            .refused_items
            .iter()
            .any(|i| i.reasons.iter().any(|r| r.contains("became a link"))));
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
        assert!(target.exists());
        assert_eq!(receipt.deleted, 0);
    }
}

#[test]
fn execute_refuses_a_target_that_is_no_longer_under_an_authorised_root() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("a.bin");
    write(&file, &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));
    assert_eq!(plan.items.len(), 1);

    // The file is untouched and still deletable, but the scan no longer
    // authorises its location. Root confinement must refuse it anyway.
    let mut moved = scan.clone();
    moved.roots = vec![types::ScanRoot {
        path: base.join("somewhere-else"),
        class: StorageClass::UserTemp,
        is_catalogue: false,
        label: "moved".to_string(),
    }];
    let receipt = execute_plan(&moved, &plan, &Arc::new(AtomicBool::new(false)));
    assert!(file.exists(), "unauthorised target must survive");
    assert_eq!(receipt.deleted, 0);
    assert!(receipt.refused_items[0]
        .reasons
        .iter()
        .any(|r| r.contains("authorised scan root")));
}

#[test]
fn execute_refuses_a_file_that_was_re_parented_into_protected_space() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("Temp").join("a.bin");
    write(&file, &filler(100, 1));
    // A context whose Windows directory is `fixture\Windows`.
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));
    assert_eq!(plan.items.len(), 1);

    // Replace the file in place with a protected one of identical size.
    fs::remove_file(&file).unwrap();
    fs::create_dir_all(base.join("Windows")).unwrap();
    let planted = base.join("Windows").join("pagefile.sys");
    write(&planted, &filler(100, 1));

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    // The original path no longer exists, so it is reported as a failure and
    // the protected file is untouched.
    assert!(planted.exists());
    assert_eq!(receipt.deleted, 0);
}

#[cfg(windows)]
#[test]
fn execute_leaves_read_only_files_alone() {
    use std::os::windows::ffi::OsStrExt;
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("a.bin");
    write(&file, &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));

    let mut attrs = fsx::file_attributes(&fs::symlink_metadata(&file).unwrap()).unwrap();
    attrs |= fsx::attrs::READONLY;
    let wide: Vec<u16> = file
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and lives for the duration of the call.
    let ok = unsafe {
        windows::Win32::Storage::FileSystem::SetFileAttributesW(
            windows::core::PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(attrs),
        )
    };
    assert!(ok.is_ok());

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert!(file.exists(), "read-only files must be left for the user");
    assert_eq!(receipt.deleted, 0);
    assert!(receipt
        .failures
        .iter()
        .any(|f| f.reason.contains("read-only")));
}

#[test]
fn execute_honours_cancellation_and_deletes_nothing_afterwards() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let mut paths = Vec::new();
    for i in 0..10 {
        let p = base.join(format!("f{i}.bin"));
        write(&p, &filler(1024, i as u8));
        paths.push(p);
    }
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(&paths));
    assert_eq!(plan.items.len(), 10);

    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(true)));
    assert_eq!(receipt.deleted, 0);
    assert!(paths.iter().all(|p| p.exists()));
    assert_eq!(receipt.refused_items.len(), 10);
}

#[test]
fn execute_reports_a_missing_target_instead_of_pretending_to_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("a.bin");
    write(&file, &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));

    fs::remove_file(&file).unwrap();
    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(receipt.deleted, 0);
    assert_eq!(receipt.failed, 1);
    assert_eq!(receipt.bytes_removed_estimate, 0);
}

#[test]
fn a_plan_built_from_a_different_scan_cannot_be_mixed_in() {
    // The plan carries its scan id; execution reports which scan it ran under.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    let file = base.join("a.bin");
    write(&file, &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let mut scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);
    let plan = build_plan(&scan, &select(std::slice::from_ref(&file)));
    scan.scan_id = "a-different-scan".to_string();
    let receipt = execute_plan(&scan, &plan, &Arc::new(AtomicBool::new(false)));
    assert_eq!(receipt.scan_id, "a-different-scan");
}

// ---------------------------------------------------------------------------
// aggregation and reporting
// ---------------------------------------------------------------------------

#[test]
fn a_category_summary_reports_its_most_cautious_member() {
    let files = vec![
        FileRecord {
            path: PathBuf::from("a"),
            size_logical: 10,
            size_allocated: Some(10),
            modified_unix: None,
            accessed_unix: None,
            created_unix: None,
            extension: String::new(),
            hard_link_count: None,
            is_reparse_point: false,
            is_read_only: false,
            is_hidden: false,
            is_system: false,
            class: StorageClass::BrowserCache,
            tier: SafetyTier::SafeToReview,
            reasons: vec![],
        },
        FileRecord {
            path: PathBuf::from("b"),
            size_logical: 30,
            size_allocated: Some(30),
            modified_unix: None,
            accessed_unix: None,
            created_unix: None,
            extension: String::new(),
            hard_link_count: None,
            is_reparse_point: false,
            is_read_only: false,
            is_hidden: false,
            is_system: false,
            class: StorageClass::BrowserCache,
            tier: SafetyTier::Unknown,
            reasons: vec![],
        },
    ];
    let summaries = summarize(&files);
    let row = summaries
        .iter()
        .find(|s| s.class == StorageClass::BrowserCache)
        .unwrap();
    assert_eq!(row.tier, SafetyTier::Unknown, "worst member must win");
    assert_eq!(row.item_count, 2);
    assert_eq!(row.bytes_allocated, 40);
}

#[test]
fn every_class_is_listed_even_when_empty() {
    let summaries = summarize(&[]);
    assert_eq!(summaries.len(), StorageClass::ALL.len());
    assert!(summaries.iter().all(|s| s.item_count == 0));
}

#[test]
fn headline_reports_a_floor_when_the_scan_was_incomplete() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("fixture");
    write(&base.join("a.bin"), &filler(100, 1));
    let ctx = ctx_for(tmp.path());
    let mut scan = synthetic_scan(&base, &ctx, StorageClass::UserTemp);

    assert_eq!(
        scan.headline().certainty,
        Certainty::Measured,
        "a clean fixture must measure cleanly"
    );

    scan.coverage.permission_denied_dirs = 3;
    let h = scan.headline();
    assert_eq!(h.certainty, Certainty::LowerBound);
    assert!(h.incomplete);

    scan.coverage.cancelled = true;
    assert_eq!(scan.headline().certainty, Certainty::Partial);
}

#[test]
fn reclaim_uses_allocated_size_not_apparent_size() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("sparse.bin");
    write(&file, &filler(4096, 1));
    let md = fs::symlink_metadata(&file).unwrap();
    let logical = md.len();
    let allocated = fsx::allocated_size(&file, logical).bytes;

    let record = FileRecord {
        path: file,
        size_logical: logical,
        size_allocated: Some(allocated),
        modified_unix: None,
        accessed_unix: None,
        created_unix: None,
        extension: "bin".into(),
        hard_link_count: None,
        is_reparse_point: false,
        is_read_only: false,
        is_hidden: false,
        is_system: false,
        class: StorageClass::UserTemp,
        tier: SafetyTier::SafeToReview,
        reasons: vec![],
    };
    assert_eq!(record.reclaimable_bytes(), allocated);
    assert!(record.reclaimable_bytes() >= logical);
}

#[test]
fn progress_reports_no_eta_until_two_roots_have_finished() {
    assert_eq!(eta_for(&[], 5), None);
    assert_eq!(eta_for(&[100], 5), None);
    assert_eq!(eta_for(&[100, 200], 4), Some(600));
    assert_eq!(eta_for(&[100, 200], 0), Some(0));
}

#[test]
fn byte_formatting_does_not_invent_precision() {
    assert_eq!(human_bytes(512), "512 B");
    assert_eq!(human_bytes(2048), "2 KB");
    assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
}

#[test]
fn duplicate_set_ids_are_stable_and_content_addressed() {
    let a = types::duplicate_set_id("abcdef0123456789abcdef", 100, 2);
    let b = types::duplicate_set_id("abcdef0123456789abcdef", 100, 2);
    let c = types::duplicate_set_id("abcdef0123456789abcdef", 200, 2);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn a_root_nested_inside_another_is_measured_once() {
    // Adding `C:\Users\bob\AppData\Local` as an extra location overlaps the
    // catalogue's own `%LOCALAPPDATA%` entries. Walking both would double the
    // file count and every byte total derived from it.
    let mut roots = vec![
        types::ScanRoot {
            path: PathBuf::from(r"C:\Users\bob\AppData\Local"),
            class: StorageClass::UserSelected,
            is_catalogue: false,
            label: "outer".to_string(),
        },
        types::ScanRoot {
            path: PathBuf::from(r"C:\Users\bob\AppData\Local\Temp"),
            class: StorageClass::UserTemp,
            is_catalogue: true,
            label: "inner".to_string(),
        },
    ];
    let dropped = catalog::drop_nested_roots(&mut roots);
    assert_eq!(dropped, 1);
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].label, "outer", "the broader root is the one kept");
}

#[test]
fn sibling_roots_are_all_kept() {
    let mut roots = vec![
        types::ScanRoot {
            path: PathBuf::from(r"C:\Users\bob\AppData\Local\Temp"),
            class: StorageClass::UserTemp,
            is_catalogue: true,
            label: "temp".to_string(),
        },
        types::ScanRoot {
            path: PathBuf::from(r"C:\Users\bob\AppData\Local\Cache"),
            class: StorageClass::UserTemp,
            is_catalogue: true,
            label: "cache".to_string(),
        },
    ];
    assert_eq!(catalog::drop_nested_roots(&mut roots), 0);
    assert_eq!(roots.len(), 2);
}

#[test]
fn a_root_that_only_shares_a_name_prefix_is_kept() {
    // `C:\Data` must not swallow `C:\Database`.
    let mut roots = vec![
        types::ScanRoot {
            path: PathBuf::from(r"C:\Data"),
            class: StorageClass::UserSelected,
            is_catalogue: false,
            label: "data".to_string(),
        },
        types::ScanRoot {
            path: PathBuf::from(r"C:\Database"),
            class: StorageClass::UserSelected,
            is_catalogue: false,
            label: "database".to_string(),
        },
    ];
    assert_eq!(catalog::drop_nested_roots(&mut roots), 0);
    assert_eq!(roots.len(), 2);
}

#[test]
fn cancelling_from_another_thread_stops_a_walk_in_flight() {
    // The Cancel button has to work while a real walk is running. This builds
    // a tree large enough that the walk cannot finish instantly, trips the
    // flag from another thread, and asserts the walk returns having read only
    // a fraction of it.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("big");
    for d in 0..40 {
        for f in 0..250 {
            write(
                &root.join(format!("d{d}")).join(format!("f{f}.bin")),
                &filler(64, (f % 7) as u8),
            );
        }
    }
    let ctx = ctx_for(tmp.path());
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);

    let handle = std::thread::spawn(move || {
        let mut sink = walk::WalkSink::new();
        walk::walk_root(
            walk::WalkRequest {
                root: &root,
                class: StorageClass::UserTemp,
                ctx: &ctx,
                limits: &walk::WalkLimits {
                    max_duration_ms: 60_000,
                    ..Default::default()
                },
                cancel: &flag,
                scan_id: "cancel-live",
                root_label: "fixture",
            },
            &mut sink,
            &mut quiet_progress,
        )
    });

    std::thread::sleep(std::time::Duration::from_millis(25));
    cancel.store(true, Ordering::Relaxed);

    // The walk's own duration limit bounds this, so a regression that ignored
    // the flag fails by taking far too long rather than hanging forever.
    let stats = handle.join().expect("walk thread panicked");
    assert!(stats.outcome.cancelled, "walk ignored the cancel flag");
    assert!(
        stats.files_seen < 10_000,
        "walk read {} files after being cancelled",
        stats.files_seen
    );
}
