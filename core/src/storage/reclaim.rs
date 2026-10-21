//! Reclaim accounting computed from the **union** of file identities.
//!
//! The engine measures files, groups them into categories, and separately
//! verifies byte-identical duplicates. Those views overlap: a duplicate copy
//! sitting in `%TEMP%` is simultaneously a "user temp" file and a "duplicate
//! copy". Adding the two views together counts the same bytes twice, and the
//! headline then promises more space than the disk can give back.
//!
//! This module collapses the views into one table keyed by file identity.
//! Every path contributes its bytes exactly once, so the headline is the sum
//! of distinct removable files and can never exceed the truth.

use std::collections::HashMap;

use super::tiers;
use super::types::{DuplicateSet, FileRecord, SafetyTier};

/// One file identity and the tier that governs it.
#[derive(Debug, Clone)]
struct Entry {
    bytes: u64,
    tier: SafetyTier,
    /// `true` when a verified duplicate group also lists this path.
    duplicate: bool,
}

/// The collapsed view of everything the scan measured.
#[derive(Debug, Clone, Default)]
pub struct ReclaimAccounting {
    /// Union of bytes held by bulk-eligible files.
    pub safe_bytes: u64,
    /// Union of bytes held by files that need individual approval.
    pub careful_bytes: u64,
    /// Union of bytes held by files that will not be touched.
    pub protected_bytes: u64,
    pub safe_file_count: u64,
    pub careful_file_count: u64,
    pub protected_file_count: u64,
    /// Distinct file identities behind every figure above.
    pub distinct_file_count: u64,
    /// Verified duplicate bytes that the engine would offer to remove.
    /// A **subset** of the buckets above.
    pub duplicate_bytes: u64,
    /// The bulk-eligible part of [`Self::duplicate_bytes`].
    pub duplicate_bulk_bytes: u64,
    /// How many duplicate copies could not be promised, because the hard-link
    /// count could not be read.
    pub uncertain_duplicate_copies: u64,
}

/// Collapse the scan's overlapping views into one identity table.
///
/// `files` is the measured set. `duplicates` is the verified-duplicate view.
/// A path that appears in both contributes its bytes once, and the more
/// cautious of the two tiers wins: the engine never becomes *more*
/// permissive after a second look.
pub fn compute(files: &[FileRecord], duplicates: &[DuplicateSet]) -> ReclaimAccounting {
    let mut table: HashMap<String, Entry> = HashMap::with_capacity(files.len());

    for f in files {
        let key = tiers::normalize(&f.path);
        let bytes = f.reclaimable_bytes();
        match table.get_mut(&key) {
            Some(existing) => {
                // The same identity measured twice (overlapping roots, a
                // retried root). Keep the larger figure and the more
                // cautious tier; never add.
                existing.bytes = existing.bytes.max(bytes);
                if f.tier > existing.tier {
                    existing.tier = f.tier;
                }
            }
            None => {
                table.insert(
                    key,
                    Entry {
                        bytes,
                        tier: f.tier,
                        duplicate: false,
                    },
                );
            }
        }
    }

    // The duplicate pass re-derives each copy's tier from the live
    // filesystem, which is more recent than the walk. Fold that in.
    let mut duplicate_bytes = 0u64;
    let mut duplicate_bulk_bytes = 0u64;
    let mut uncertain_duplicate_copies = 0u64;

    for set in duplicates {
        for copy in &set.copies {
            let key = tiers::normalize(&copy.path);
            let entry = table.entry(key).or_insert_with(|| Entry {
                bytes: copy.size_allocated,
                tier: copy.tier,
                duplicate: true,
            });
            if copy.tier > entry.tier {
                entry.tier = copy.tier;
            }
            if copy.size_allocated > entry.bytes {
                entry.bytes = copy.size_allocated;
            }
            entry.duplicate = true;

            if copy.is_kept_candidate {
                continue;
            }
            if !copy.tier.allows_deletion_request() {
                continue;
            }
            if !copy.reclaim_is_certain {
                uncertain_duplicate_copies += 1;
                continue;
            }
            duplicate_bytes = duplicate_bytes.saturating_add(copy.size_allocated);
            if copy.tier.allows_bulk_recommendation() {
                duplicate_bulk_bytes = duplicate_bulk_bytes.saturating_add(copy.size_allocated);
            }
        }
    }

    let mut out = ReclaimAccounting {
        duplicate_bytes,
        duplicate_bulk_bytes,
        uncertain_duplicate_copies,
        ..Default::default()
    };

    for entry in table.values() {
        match entry.tier {
            SafetyTier::SafeToReview => {
                out.safe_bytes = out.safe_bytes.saturating_add(entry.bytes);
                out.safe_file_count += 1;
            }
            SafetyTier::ReviewCarefully => {
                out.careful_bytes = out.careful_bytes.saturating_add(entry.bytes);
                out.careful_file_count += 1;
            }
            SafetyTier::Protected | SafetyTier::Unknown => {
                out.protected_bytes = out.protected_bytes.saturating_add(entry.bytes);
                out.protected_file_count += 1;
            }
        }
    }
    out.distinct_file_count = table.len() as u64;
    out
}
