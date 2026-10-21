//! Verified duplicate detection.
//!
//! The pipeline is deliberately staged, and nothing is reported until the
//! last stage completes:
//!
//! 1. **Size bucket** — files of different lengths cannot be identical. This
//!    is the only stage that touches metadata.
//! 2. **Partial hash** — first and last 64 KiB, so a bucket of large media
//!    files is separated without reading them end to end.
//! 3. **Full SHA-256** — every member of a surviving group is read in full
//!    and re-checked for modification afterwards.
//!
//! A group that survives all three is byte-identical. A group that fails any
//! of them is discarded, never reported. There is no "probably a duplicate"
//! tier, because there is no safe action to attach to one.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest, Sha256};

use super::fsx;
use super::tiers::{self, TierContext};
use super::types::{DuplicateCopy, DuplicateSet, SafetyTier, ScanProgress, StorageClass};

/// Files at or above this size are sampled by the partial stage but still
/// fully verified before any group is reported; the cap only limits how much
/// is read when a group turns out not to be duplicates.
pub const PARTIAL_SAMPLE_BYTES: u64 = 64 * 1024;

/// Groups smaller than this cannot produce a worthwhile recommendation.
pub const MIN_GROUP_SIZE: u64 = 1024;

/// Hard ceiling on any single file's contribution to a duplicate scan.
/// Anything larger is still inventoried and still reported as a large file —
/// it just never joins a duplicate group, because reading a 60 GB file to
/// prove it is unique buys nothing.
pub const MAX_DUPLICATE_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Tuning for [`find_duplicates`].
#[derive(Debug, Clone)]
pub struct DuplicateOptions {
    pub max_files: usize,
    pub max_bytes_to_read: u64,
    pub max_duration_ms: u64,
}

impl Default for DuplicateOptions {
    fn default() -> Self {
        DuplicateOptions {
            max_files: 400_000,
            max_bytes_to_read: 12 * 1024 * 1024 * 1024,
            max_duration_ms: 180_000,
        }
    }
}

/// Why a file did not join a group. Surfaced in the coverage report so an
/// absent result is never mistaken for "there are no duplicates".
#[derive(Debug, Clone, Default)]
pub struct DuplicateReport {
    pub files_considered: usize,
    /// Files whose entire content was read and hashed.
    pub fully_hashed: usize,
    pub size_buckets: usize,
    pub partial_hash_groups: usize,
    pub full_hash_groups: usize,
    pub verified_sets: usize,
    pub skipped_oversize: usize,
    pub skipped_hardlinked: usize,
    pub skipped_changed_during_scan: usize,
    pub skipped_unreadable: usize,
    pub bytes_hashed: u64,
    pub truncated: bool,
    pub reason: String,
}

impl DuplicateReport {
    /// One sentence the UI can show verbatim.
    ///
    /// The sentence must never claim more than was done. "Checked N files in
    /// full" was only ever true for the files that survived into a
    /// full-hash group; the rest were sampled. The report therefore counts
    /// the two stages separately.
    pub fn coverage_sentence(&self) -> String {
        if self.truncated {
            return format!(
                "Partial — stopped after {} files ({})",
                self.files_considered, self.reason
            );
        }
        if self.skipped_oversize > 0 || self.skipped_hardlinked > 0 {
            return format!(
                "Considered {} files, read {} of them in full; {} above the size cap and {} \
                 hard-linked files were not grouped",
                self.files_considered,
                self.fully_hashed,
                self.skipped_oversize,
                self.skipped_hardlinked
            );
        }
        format!(
            "Considered {} files and read {} of them in full",
            self.files_considered, self.fully_hashed
        )
    }
}

/// A file staged for duplicate analysis.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub path: PathBuf,
    pub size_logical: u64,
    pub modified_unix: Option<u64>,
    pub hard_link_count: Option<u32>,
    pub class: StorageClass,
    pub tier: SafetyTier,
}

/// Find byte-identical groups among `candidates`.
///
/// Cancellation is honoured at each stage boundary and between files, so a
/// long duplicate pass stays interruptible.
pub fn find_duplicates(
    candidates: &[Candidate],
    ctx: &TierContext,
    options: &DuplicateOptions,
    cancel: &Arc<AtomicBool>,
    scan_id: &str,
    mut progress: impl FnMut(ScanProgress),
) -> (Vec<DuplicateSet>, DuplicateReport) {
    let started = Instant::now();
    let mut report = DuplicateReport::default();
    let mut sets: Vec<DuplicateSet> = Vec::new();

    // ---- stage 1: size buckets -----------------------------------------
    let mut buckets: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    let mut pool: Vec<Candidate> = Vec::new();

    for c in candidates {
        if c.size_logical < MIN_GROUP_SIZE {
            continue;
        }
        if c.size_logical > MAX_DUPLICATE_FILE_BYTES {
            report.skipped_oversize += 1;
            continue;
        }
        if matches!(c.hard_link_count, Some(n) if n > 1) {
            // Two names for one inode: a duplicate in name only. Reporting
            // it would promise bytes that deleting a link never returns.
            report.skipped_hardlinked += 1;
            continue;
        }
        if buckets.len() >= options.max_files || pool.len() >= options.max_files {
            report.truncated = true;
            report.reason = "file cap reached".to_string();
            break;
        }
        buckets.entry(c.size_logical).or_default().push(pool.len());
        pool.push(c.clone());
    }

    report.files_considered = pool.len();
    report.size_buckets = buckets.iter().filter(|(_, v)| v.len() > 1).count();

    let mut start_progress = |phase: &str, done: usize, bytes: u64, phase_total: usize| {
        progress(ScanProgress {
            scan_id: scan_id.to_string(),
            phase: phase.to_string(),
            root_label: "Duplicates".to_string(),
            current_path: String::new(),
            dirs_seen: 0,
            files_seen: done as u64,
            bytes_seen: bytes,
            elapsed_ms: started.elapsed().as_millis() as u64,
            eta_ms: None,
        });
        let _ = phase_total;
    };

    start_progress("hashing", 0, 0, 0);

    // ---- stage 2: partial hash ------------------------------------------
    let mut partial_groups: Vec<Vec<usize>> = Vec::new();
    let mut hashed_bytes: u64 = 0;
    let mut partial_done = 0usize;
    let mut truncated_partial = false;

    for members in buckets.values() {
        if members.len() < 2 {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            report.truncated = true;
            report.reason = "cancelled".to_string();
            break;
        }
        let mut by_partial: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for idx in members {
            if should_stop(started, options, hashed_bytes) {
                truncated_partial = true;
                break;
            }
            let c = &pool[*idx];
            let sampled = partial_hash(&c.path, c.size_logical);
            let read = partial_sample_bytes(c.size_logical);
            hashed_bytes += read;
            report.bytes_hashed += read;
            partial_done += 1;
            match sampled {
                Some(h) => {
                    by_partial.entry(h).or_default().push(*idx);
                }
                None => {
                    report.skipped_unreadable += 1;
                }
            }
            if partial_done.is_multiple_of(256) {
                start_progress("hashing", partial_done, hashed_bytes, 0);
            }
        }
        if truncated_partial {
            report.truncated = true;
            report.reason = "read budget reached".to_string();
            break;
        }
        partial_groups.extend(by_partial.into_values().filter(|v| v.len() > 1));
    }

    report.partial_hash_groups = partial_groups.len();

    // ---- stage 3: full SHA-256 -----------------------------------------
    let mut full_done = 0usize;
    'groups: for members in partial_groups {
        if cancel.load(Ordering::Relaxed) {
            report.truncated = true;
            report.reason = "cancelled".to_string();
            break;
        }
        let mut by_full: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for idx in members {
            if should_stop(started, options, hashed_bytes) {
                report.truncated = true;
                report.reason = "read budget reached".to_string();
                break 'groups;
            }
            let c = &pool[idx];
            let before = snapshot(&c.path);
            match full_hash(&c.path) {
                Some(digest) => {
                    let after = snapshot(&c.path);
                    if before != after {
                        // The file changed underneath us. A hash of a moving
                        // target proves nothing, so it leaves the group.
                        report.skipped_changed_during_scan += 1;
                        continue;
                    }
                    hashed_bytes += c.size_logical;
                    report.bytes_hashed += c.size_logical;
                    report.fully_hashed += 1;
                    by_full.entry(digest).or_default().push(idx);
                }
                None => {
                    report.skipped_unreadable += 1;
                }
            }
            full_done += 1;
            if full_done.is_multiple_of(32) {
                start_progress("hashing", full_done, hashed_bytes, 0);
            }
        }
        for (digest, mut idxs) in by_full {
            if idxs.len() < 2 {
                continue;
            }
            report.full_hash_groups += 1;
            report.verified_sets += 1;
            idxs.sort_by(|a, b| compare_for_keep(&pool[*a], &pool[*b]));
            sets.push(build_set(&digest, &idxs, &pool, ctx));
        }
    }

    // Largest reclaim first, with the id as a stable tie-break.
    sets.sort_by(|a, b| {
        b.reclaimable_bytes
            .cmp(&a.reclaimable_bytes)
            .then_with(|| a.id.cmp(&b.id))
    });
    (sets, report)
}

fn should_stop(started: Instant, options: &DuplicateOptions, hashed: u64) -> bool {
    hashed >= options.max_bytes_to_read
        || started.elapsed().as_millis() as u64 > options.max_duration_ms
}

/// `(size, mtime)` used to detect a file changing between hash and report.
pub(crate) fn snapshot(path: &Path) -> (u64, Option<u64>) {
    match std::fs::metadata(path) {
        Ok(md) => (md.len(), fsx::unix_seconds(md.modified())),
        Err(_) => (u64::MAX, None),
    }
}

/// Order copies so the least valuable one is kept.
///
/// Prefer the newest file (most likely the deliberate copy), then the one in
/// the least disposable location, then path order so the result is stable
/// across runs and testable.
fn compare_for_keep(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.modified_unix, b.modified_unix) {
        (Some(x), Some(y)) if x != y => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => {
            let a_temp = is_disposable(a.tier);
            let b_temp = is_disposable(b.tier);
            match a_temp.cmp(&b_temp) {
                Ordering::Equal => a.path.cmp(&b.path),
                other => other,
            }
        }
    }
}

fn is_disposable(tier: SafetyTier) -> bool {
    matches!(tier, SafetyTier::SafeToReview)
}

/// Build a reported group, with reclaim computed from real link counts.
fn build_set(digest: &str, idxs: &[usize], pool: &[Candidate], ctx: &TierContext) -> DuplicateSet {
    let size = pool[idxs[0]].size_logical;
    let mut copies: Vec<DuplicateCopy> = Vec::with_capacity(idxs.len());
    let mut reclaimable: u64 = 0;
    let mut bulk_reclaimable: u64 = 0;
    let mut protected = 0usize;
    let mut uncertain = 0usize;

    for (position, idx) in idxs.iter().enumerate() {
        let c = &pool[*idx];
        let allocated = fsx::allocated_size(&c.path, c.size_logical).bytes;
        let is_kept = position == 0;

        // Re-check the live tier: a path may have moved under a protected
        // root between the walk and now.
        let live = tiers::classify(&c.path, c.class, ctx);
        let tier = live.tier;

        // A link count that could not be read is *unknown*, not "one link".
        // Removing a name whose only other link is somewhere else frees
        // nothing, so an unknown count must never be promised as reclaim.
        let link_count_known = matches!(c.hard_link_count, Some(1));
        let link_count_many = matches!(c.hard_link_count, Some(n) if n > 1);

        let eligible = !is_kept && tier.allows_deletion_request() && allocated > 0;
        let deletable = eligible && link_count_known;
        if deletable {
            reclaimable += allocated;
            if tier.allows_bulk_recommendation() {
                bulk_reclaimable += allocated;
            }
        }
        if eligible && !link_count_known {
            uncertain += 1;
        }
        if !tier.allows_deletion_request() {
            protected += 1;
        }

        let mut reasons = live.reasons;
        if is_kept {
            reasons.push(
                "kept: the most recent copy, or the one outside a disposable location".to_string(),
            );
        } else if !tier.allows_deletion_request() {
            reasons.push("this engine will not propose removing this copy".to_string());
        } else if link_count_many {
            reasons.push(
                "hard-linked elsewhere: removing this name would not free any bytes".to_string(),
            );
        } else if !link_count_known {
            reasons.push(
                "the hard-link count could not be read, so removing this name may not free \
                 any bytes; no reclaim is promised"
                    .to_string(),
            );
        }

        copies.push(DuplicateCopy {
            path: c.path.clone(),
            size_logical: c.size_logical,
            size_allocated: allocated,
            modified_unix: c.modified_unix,
            hard_link_count: c.hard_link_count,
            tier,
            class: c.class,
            is_kept_candidate: is_kept,
            reclaim_is_certain: deletable,
            reasons,
        });
    }

    DuplicateSet {
        id: super::types::duplicate_set_id(digest, size, copies.len()),
        size_logical: size,
        size_allocated: fsx::allocated_size(&pool[idxs[0]].path, size).bytes,
        sha256: digest.to_string(),
        copies,
        reclaimable_bytes: reclaimable,
        bulk_reclaimable_bytes: bulk_reclaimable,
        uncertain_copies: uncertain,
        protected_copies: protected,
    }
}

/// Build a reported group straight from candidate values.
///
/// Exposed for tests so the keep/deletable/reclaim rules can be checked
/// without constructing an on-disk tree.
#[cfg(test)]
pub(crate) fn build_set_for_test(
    digest: &str,
    candidates: &[Candidate],
    ctx: &TierContext,
) -> DuplicateSet {
    // Same ordering the pipeline applies, so the test exercises the real
    // keep-selection rule rather than the order it happened to pass in.
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|a, b| compare_for_keep(&candidates[*a], &candidates[*b]));
    build_set(digest, &order, candidates, ctx)
}

/// Hash the first and last [`PARTIAL_SAMPLE_BYTES`] of a file.
fn partial_hash(path: &Path, size: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let take = PARTIAL_SAMPLE_BYTES.min(size);
    if take > 0 {
        let mut buf = vec![0u8; take as usize];
        f.read_exact(&mut buf).ok()?;
        hasher.update(&buf);
    }
    if size > take {
        f.seek(SeekFrom::End(-(take as i64))).ok()?;
        let mut buf = vec![0u8; take as usize];
        f.read_exact(&mut buf).ok()?;
        hasher.update(&buf);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// How many bytes [`partial_hash`] actually reads for a file of `size`.
///
/// The stage samples the head *and* the tail, so the honest cost is up to
/// twice the sample window. Reporting one window understates the read budget
/// the engine actually spends.
pub fn partial_sample_bytes(size: u64) -> u64 {
    let take = PARTIAL_SAMPLE_BYTES.min(size);
    if size > take {
        take + take
    } else {
        take
    }
}

/// Full-content SHA-256, streamed so memory stays flat regardless of size.
pub fn full_hash(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}
