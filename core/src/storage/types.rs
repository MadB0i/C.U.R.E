//! Shared vocabulary for the storage intelligence engine.
//!
//! Everything here is a plain serialisable DTO. The engine reports
//! measurements and *evidence*, never verdicts about the user's data: a tier
//! says how much scrutiny a path needs, never "this is junk".

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::safedelete;

/// How much scrutiny a path deserves before anything is removed.
///
/// The ordering matters and is deliberately pessimistic: `Unknown` is worse
/// than `Protected`, because "we could not tell" is not the same as "we know
/// this is critical", and an unknown path must never receive a one-click
/// deletion recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyTier {
    /// Regenerable by definition: temp caches, dumps, browser caches.
    /// Still needs a preview — "safe" describes the *class*, not a licence.
    SafeToReview,
    /// Real user data or a guess about intent. Every item is confirmed
    /// individually and there is no bulk action.
    ReviewCarefully,
    /// Operating system, drivers, recovery, restore points, installed
    /// applications. Never deleted by this engine, ever.
    Protected,
    /// We could not classify this path. Never auto-recommended.
    Unknown,
}

impl SafetyTier {
    pub fn key(self) -> &'static str {
        match self {
            SafetyTier::SafeToReview => "safe_to_review",
            SafetyTier::ReviewCarefully => "review_carefully",
            SafetyTier::Protected => "protected",
            SafetyTier::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SafetyTier::SafeToReview => "Safe to review",
            SafetyTier::ReviewCarefully => "Review carefully",
            SafetyTier::Protected => "Protected",
            SafetyTier::Unknown => "Unknown",
        }
    }

    /// One line explaining what the tier means, shown in the UI.
    pub fn explanation(self) -> &'static str {
        match self {
            SafetyTier::SafeToReview => {
                "Regenerable by the application that made it. Removed files come back on next use."
            }
            SafetyTier::ReviewCarefully => {
                "Real files you or an application created. Nothing here is removed without you \
                 choosing each item."
            }
            SafetyTier::Protected => {
                "Operating system, drivers, recovery or installed applications. This engine never \
                 proposes removing these."
            }
            SafetyTier::Unknown => {
                "Could not be classified with confidence. Treated as off-limits; no deletion is \
                 offered."
            }
        }
    }

    /// Only `SafeToReview` may ever appear in a bulk action list.
    ///
    /// `ReviewCarefully` requires per-item confirmation, and
    /// `Protected`/`Unknown` are refused outright.
    pub fn allows_bulk_recommendation(self) -> bool {
        matches!(self, SafetyTier::SafeToReview)
    }

    /// `false` for anything the engine will not delete even when asked.
    pub fn allows_deletion_request(self) -> bool {
        matches!(self, SafetyTier::SafeToReview | SafetyTier::ReviewCarefully)
    }

    pub const ALL: [SafetyTier; 4] = [
        SafetyTier::SafeToReview,
        SafetyTier::ReviewCarefully,
        SafetyTier::Protected,
        SafetyTier::Unknown,
    ];
}

/// What kind of storage an item belongs to. Categories describe *provenance*,
/// which is the main input to the tier decision — they are never on their own
/// a reason to delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageClass {
    UserTemp,
    WindowsTemp,
    BrowserCache,
    RecycleBin,
    WindowsOld,
    WindowsUpdateCache,
    CrashDumps,
    DiagnosticLogs,
    DevCache,
    OldInstaller,
    OldDownload,
    AbandonedAppData,
    UserSelected,
    Other,
}

impl StorageClass {
    pub fn key(self) -> &'static str {
        match self {
            StorageClass::UserTemp => "user_temp",
            StorageClass::WindowsTemp => "windows_temp",
            StorageClass::BrowserCache => "browser_cache",
            StorageClass::RecycleBin => "recycle_bin",
            StorageClass::WindowsOld => "windows_old",
            StorageClass::WindowsUpdateCache => "windows_update_cache",
            StorageClass::CrashDumps => "crash_dumps",
            StorageClass::DiagnosticLogs => "diagnostic_logs",
            StorageClass::DevCache => "dev_cache",
            StorageClass::OldInstaller => "old_installer",
            StorageClass::OldDownload => "old_download",
            StorageClass::AbandonedAppData => "abandoned_app_data",
            StorageClass::UserSelected => "user_selected",
            StorageClass::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StorageClass::UserTemp => "User temp files",
            StorageClass::WindowsTemp => "Windows temp files",
            StorageClass::BrowserCache => "Browser caches",
            StorageClass::RecycleBin => "Recycle Bin",
            StorageClass::WindowsOld => "Previous Windows install",
            StorageClass::WindowsUpdateCache => "Windows Update cache",
            StorageClass::CrashDumps => "Crash dumps",
            StorageClass::DiagnosticLogs => "Diagnostic logs",
            StorageClass::DevCache => "Developer build caches",
            StorageClass::OldInstaller => "Old installers",
            StorageClass::OldDownload => "Old downloads",
            StorageClass::AbandonedAppData => "Possibly unused app data",
            StorageClass::UserSelected => "Selected location",
            StorageClass::Other => "Other",
        }
    }

    /// Provenance alone cannot promote a path to `SafeToReview`.
    ///
    /// This is the *floor* tier for the class. Real judgement happens in
    /// [`crate::storage::tiers::classify`], which can only ever raise it
    /// (towards Protected/Unknown), never lower it below this.
    pub fn floor_tier(self) -> SafetyTier {
        match self {
            StorageClass::UserTemp
            | StorageClass::WindowsTemp
            | StorageClass::BrowserCache
            | StorageClass::RecycleBin
            | StorageClass::CrashDumps
            | StorageClass::DiagnosticLogs
            | StorageClass::WindowsUpdateCache => SafetyTier::SafeToReview,

            StorageClass::WindowsOld => SafetyTier::ReviewCarefully,

            StorageClass::DevCache => SafetyTier::ReviewCarefully,
            StorageClass::OldInstaller => SafetyTier::ReviewCarefully,
            StorageClass::OldDownload => SafetyTier::ReviewCarefully,
            StorageClass::AbandonedAppData => SafetyTier::Unknown,
            StorageClass::UserSelected => SafetyTier::ReviewCarefully,
            StorageClass::Other => SafetyTier::Unknown,
        }
    }

    pub const ALL: [StorageClass; 14] = [
        StorageClass::UserTemp,
        StorageClass::WindowsTemp,
        StorageClass::BrowserCache,
        StorageClass::RecycleBin,
        StorageClass::WindowsOld,
        StorageClass::WindowsUpdateCache,
        StorageClass::CrashDumps,
        StorageClass::DiagnosticLogs,
        StorageClass::DevCache,
        StorageClass::OldInstaller,
        StorageClass::OldDownload,
        StorageClass::AbandonedAppData,
        StorageClass::UserSelected,
        StorageClass::Other,
    ];
}

/// A single measured file.
///
/// `size_logical` is the apparent length; `size_allocated` is the space the
/// bytes actually occupy on the volume. They differ for sparse and compressed
/// files, and reclaim estimates must use the allocated figure or they lie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: PathBuf,
    pub size_logical: u64,
    /// `None` when the platform would not report it (non-Windows, or the
    /// file vanished between enumeration and the query).
    pub size_allocated: Option<u64>,
    pub modified_unix: Option<u64>,
    /// Windows frequently disables last-access updates, so `None` and
    /// "recently touched" are indistinguishable. Never treated as proof.
    pub accessed_unix: Option<u64>,
    pub created_unix: Option<u64>,
    pub extension: String,
    pub hard_link_count: Option<u32>,
    pub is_reparse_point: bool,
    pub is_read_only: bool,
    pub is_hidden: bool,
    pub is_system: bool,
    pub class: StorageClass,
    pub tier: SafetyTier,
    /// Human-readable justification, one clause per decision input.
    pub reasons: Vec<String>,
}

impl FileRecord {
    /// Bytes that deleting this file would actually return to the volume.
    pub fn reclaimable_bytes(&self) -> u64 {
        self.size_allocated.unwrap_or(self.size_logical)
    }
}

/// A measured directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderRecord {
    pub path: PathBuf,
    pub depth: usize,
    pub size_logical: u64,
    pub size_allocated: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub class: StorageClass,
    pub tier: SafetyTier,
    /// `true` when part of this tree could not be read. A partial number is
    /// always reported as partial; never rounded up into a clean total.
    pub incomplete: bool,
}

/// A root the user or the catalogue asked the engine to look at.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRoot {
    pub path: PathBuf,
    pub class: StorageClass,
    /// `true` for roots the engine resolved itself from known-safe
    /// locations, `false` for a drive/folder the user picked.
    pub is_catalogue: bool,
    pub label: String,
}

/// Per-root outcome: what was measured and what could not be.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCoverage {
    pub path: PathBuf,
    pub label: String,
    pub class: StorageClass,
    pub resolved: bool,
    pub reason: String,
    pub files_seen: u64,
    pub dirs_seen: u64,
    pub bytes_logical: u64,
    pub bytes_allocated: u64,
    /// True when any entry under this root was unreadable or skipped.
    pub incomplete: bool,
}

/// A location the engine refused to read, with the OS reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFailure {
    pub path: PathBuf,
    pub reason: String,
    pub is_permission: bool,
}

/// Honest accounting of what the scan could and could not see.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanCoverage {
    pub roots: Vec<RootCoverage>,
    pub failures: Vec<ScanFailure>,
    /// Links and junctions that were deliberately not followed.
    ///
    /// These are **not** failures. Skipping a link is the policy working,
    /// and counting it as a read failure would downgrade an otherwise
    /// complete measurement for no reason.
    pub skipped_links: Vec<ScanFailure>,
    pub permission_denied_dirs: u64,
    pub permission_denied_files: u64,
    pub reparse_points_skipped: u64,
    pub locked_or_vanished: u64,
    /// Roots dropped because they sit inside another root already being
    /// walked. Reported so the file count is never quietly smaller than the
    /// operator asked for.
    pub overlapping_roots_dropped: u64,
    pub files_seen: u64,
    pub dirs_seen: u64,
    pub duration_ms: u64,
    pub cancelled: bool,
    /// Free space on the volume the primary root lives on, measured by the
    /// OS — never estimated.
    pub volume_total_bytes: Option<u64>,
    pub volume_free_bytes: Option<u64>,
    pub volume_label: Option<String>,
}

impl ScanCoverage {
    /// One line stating whether the numbers above can be trusted wholesale.
    pub fn completeness(&self) -> &'static str {
        if self.cancelled {
            "Partial — scan was cancelled before it finished"
        } else if !self.failures.is_empty() {
            "Partial — some locations could not be read"
        } else {
            "Complete for the roots scanned"
        }
    }

    /// Extra context the coverage panel shows when it is relevant.
    pub fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.overlapping_roots_dropped > 0 {
            out.push(format!(
                "{} requested location(s) sat inside another scanned root and were measured once \
                 rather than twice.",
                self.overlapping_roots_dropped
            ));
        }
        if self.reparse_points_skipped > 0 {
            out.push(format!(
                "{} link(s) or reparse point(s) were skipped and never followed.",
                self.reparse_points_skipped
            ));
        }
        out
    }
}

/// Live scan progress. Emitted so the UI can show what is really happening
/// rather than an invented progress bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: String,
    /// `walking`, `hashing`, `aggregating`, `finished`, `cancelled`.
    pub phase: String,
    pub root_label: String,
    pub current_path: String,
    pub dirs_seen: u64,
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub elapsed_ms: u64,
    /// `None` until enough samples exist to be worth anything. An invented
    /// ETA is worse than none.
    pub eta_ms: Option<u64>,
}

/// One measured copy of a byte-identical file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCopy {
    pub path: PathBuf,
    pub size_logical: u64,
    /// Bytes the copy actually occupies on the volume. This is the figure
    /// every reclaim estimate uses, because a 4 GiB sparse file does not free
    /// 4 GiB when it is deleted.
    pub size_allocated: u64,
    pub modified_unix: Option<u64>,
    /// `None` means the link count could not be read. That is *not* the same
    /// as "one link", and the engine never treats it as such.
    pub hard_link_count: Option<u32>,
    pub tier: SafetyTier,
    pub class: StorageClass,
    /// The copy the engine suggests keeping: the most recent one, or the one
    /// outside a disposable location. Removing anything else is a guess;
    /// removing this one is the conventional "keep one" rule, and it is still
    /// only a suggestion the user confirms.
    pub is_kept_candidate: bool,
    /// `true` when the engine is *certain* that removing this copy frees
    /// `size_allocated` bytes. `false` when the copy is eligible in principle
    /// but the reclaim cannot be promised — an unknown hard-link count, for
    /// example.
    pub reclaim_is_certain: bool,
    pub reasons: Vec<String>,
}

/// A verified set of byte-identical files.
///
/// `sha256` is only populated after a full read of every member. Groups that
/// merely share a size never appear here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateSet {
    pub id: String,
    pub size_logical: u64,
    pub size_allocated: u64,
    pub sha256: String,
    pub copies: Vec<DuplicateCopy>,
    /// Bytes that would actually be freed if every non-kept, deletable copy
    /// were removed.
    pub reclaimable_bytes: u64,
    /// The part of [`Self::reclaimable_bytes`] that sits in a bulk-eligible
    /// location. The rest needs individual approval, so it must never be
    /// added to a bulk recommendation.
    pub bulk_reclaimable_bytes: u64,
    /// Copies whose bytes cannot be *promised*: the hard-link count could not
    /// be read, so removing the name might free nothing.
    pub uncertain_copies: usize,
    /// Copies this engine refuses to touch no matter what.
    pub protected_copies: usize,
}

/// An abandoned-looking application data directory.
///
/// **Never a deletion recommendation.** The whole point of the record is to
/// be honest that "not modified in 400 days" is a weak signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbandonedAppRecord {
    pub path: PathBuf,
    pub app_name: String,
    pub size_logical: u64,
    pub size_allocated: u64,
    pub file_count: u64,
    pub last_modified_unix: Option<u64>,
    pub days_since_change: Option<u64>,
    pub class: StorageClass,
    pub tier: SafetyTier,
    pub reasons: Vec<String>,
    /// What this record can and cannot tell you, stated plainly.
    pub caveat: String,
}

/// Aggregate view of one class, for the overview and category filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassSummary {
    pub class: StorageClass,
    pub tier: SafetyTier,
    pub item_count: u64,
    pub bytes_logical: u64,
    pub bytes_allocated: u64,
}

/// What the duplicate pass managed to check, stated so that "no duplicates
/// found" is never confused with "the check did not run".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuplicateCoverage {
    pub files_considered: usize,
    /// How many of those files were read end to end.
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
    /// One sentence, safe to show verbatim.
    pub summary: String,
}

/// The complete result of one storage scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageScan {
    pub scan_id: String,
    pub started_unix: u64,
    pub finished_unix: u64,
    pub roots: Vec<ScanRoot>,
    pub files: Vec<FileRecord>,
    pub folders: Vec<FolderRecord>,
    pub summaries: Vec<ClassSummary>,
    pub coverage: ScanCoverage,
    pub duplicate_coverage: DuplicateCoverage,
    /// Largest files, already sorted descending by allocated size.
    pub large_files: Vec<FileRecord>,
    /// The folders that occupy the most space, largest first. Every folder
    /// in the tree is measured; only the biggest are carried forward, so a
    /// scan of half a million directories does not become half a million
    /// rows of UI.
    pub large_folders: Vec<FolderRecord>,
    /// Verified duplicate sets, largest reclaimable first.
    pub duplicates: Vec<DuplicateSet>,
    pub abandoned: Vec<AbandonedAppRecord>,
    /// Sum of `reclaimable_bytes` over `SafeToReview` items only. Protected,
    /// Unknown and ReviewCarefully bytes are excluded, and separately
    /// reported, so the headline number can never over-promise.
    pub safe_reclaimable_bytes: u64,
    /// Sum over `ReviewCarefully` items. Requires individual confirmation.
    pub careful_reclaimable_bytes: u64,
    /// Bytes in paths that will not be touched at all.
    pub protected_bytes: u64,
    /// Bytes held by verified duplicate copies that the engine would offer to
    /// remove. This is a **subset** of `safe_reclaimable_bytes` and
    /// `careful_reclaimable_bytes` — never an addition to them. It is reported
    /// separately so the UI can say "of which N GB are duplicated copies"
    /// without adding the same bytes to the headline twice.
    pub duplicate_reclaimable_bytes: u64,
    /// The part of [`Self::duplicate_reclaimable_bytes`] that is bulk-eligible.
    pub duplicate_bulk_reclaimable_bytes: u64,
    /// How many distinct files the safe figure covers.
    pub safe_file_count: u64,
    /// How many distinct files the careful figure covers.
    pub careful_file_count: u64,
    /// How many distinct files are off-limits.
    pub protected_file_count: u64,
    /// How many file identities were folded into the totals. Compared against
    /// the number of measured files, this shows how much overlap was
    /// collapsed instead of added.
    pub distinct_file_count: u64,
}

impl StorageScan {
    /// The number the UI is allowed to lead with, plus its uncertainty.
    ///
    /// Always derived from allocated bytes, never from the apparent length,
    /// and always computed from the **union** of file identities: a file that
    /// is counted in two categories contributes its bytes once. The headline
    /// can therefore never exceed the bytes that are actually removable.
    pub fn headline(&self) -> Headline {
        let certainty = Certainty::from_coverage(&self.coverage);
        Headline {
            safe_bytes: self.safe_reclaimable_bytes,
            careful_bytes: self.careful_reclaimable_bytes,
            duplicate_bytes: self.duplicate_reclaimable_bytes,
            duplicate_bulk_bytes: self.duplicate_bulk_reclaimable_bytes,
            certainty,
            // Derived from the same rule as `certainty`, so the number and
            // the "at least"/"measured" wording can never disagree.
            incomplete: certainty != Certainty::Measured,
        }
    }
}

/// The single estimate the UI presents, with its uncertainty stated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Headline {
    /// Lower bound: the union of every bulk-eligible file's allocated bytes.
    pub safe_bytes: u64,
    /// Everything that would also need individual confirmation.
    pub careful_bytes: u64,
    /// A **subset** of the two figures above: the part held by verified
    /// duplicate copies. Shown for context, never added to them.
    pub duplicate_bytes: u64,
    /// The bulk-eligible part of [`Self::duplicate_bytes`].
    pub duplicate_bulk_bytes: u64,
    pub certainty: Certainty,
    pub incomplete: bool,
}

/// How much the numbers can be trusted, stated rather than implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    /// Every root read end to end.
    Measured,
    /// Some locations unreadable; totals are a floor, not a total.
    LowerBound,
    /// Cancelled before finishing.
    Partial,
}

impl Certainty {
    pub fn label(self) -> &'static str {
        match self {
            Certainty::Measured => "Measured",
            Certainty::LowerBound => "At least",
            Certainty::Partial => "Partial",
        }
    }

    pub fn from_coverage(coverage: &ScanCoverage) -> Certainty {
        if coverage.cancelled {
            Certainty::Partial
        } else if coverage.permission_denied_dirs > 0
            || coverage.permission_denied_files > 0
            || !coverage.failures.is_empty()
        {
            Certainty::LowerBound
        } else {
            Certainty::Measured
        }
    }
}

/// One item the engine proposes to remove, with the evidence attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub path: PathBuf,
    pub size_logical: u64,
    pub size_allocated: u64,
    pub tier: SafetyTier,
    pub class: StorageClass,
    pub reasons: Vec<String>,
    /// `true` when the path is no longer what the scan saw (it moved, was
    /// replaced, or became a link). Such items are dropped from the plan.
    pub stale: bool,
    /// The identity of the file at plan time. Execution re-checks it through
    /// the handle it is about to delete, so a file that was swapped in
    /// between approval and execution is refused rather than removed.
    #[serde(default)]
    pub file_id: Option<safedelete::FileIdentity>,
    /// The size the operator was shown, carried forward so the receipt can
    /// show what was promised against what was removed.
    #[serde(default)]
    pub approved_size: u64,
}

/// The full preview shown before anything is removed.
///
/// The plan is what the user approves. Execution re-validates every entry
/// against the live filesystem and refuses anything that changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPlan {
    pub scan_id: String,
    /// A digest of the approved items. Execution recomputes it and refuses a
    /// plan whose contents no longer match the digest the operator approved.
    pub plan_id: String,
    pub items: Vec<PlanItem>,
    pub refused: Vec<PlanItem>,
    pub estimated_bytes: u64,
    pub estimated_certainty: Certainty,
    /// Plain-language statement of what happens, shown above the confirm.
    pub consequence: String,
    /// The exact word the operator had to type to approve this plan.
    pub confirmation_phrase: String,
}

/// The phrase an operator must type to approve a cleanup plan.
pub const CONFIRM_PHRASE: &str = "REMOVE";

/// Result of an execution, with figures separated by what they actually are.
///
/// The distinction matters. A byte count taken *before* a file is deleted is
/// an estimate of what that file occupied. A free-space reading taken *after*
/// the batch is an observation of the whole volume, and unrelated system
/// writes can move it. Neither is a per-file guarantee, and this receipt
/// never presents one as the other.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReceipt {
    pub scan_id: String,
    /// Items the operator approved.
    pub planned: usize,
    /// Items the engine actually attempted to remove.
    pub attempted: usize,
    /// Files confirmed gone: the delete succeeded *and* the path was
    /// re-checked afterwards.
    pub deleted: usize,
    /// Items the engine refused at execution time.
    pub refused: usize,
    /// Deletes that failed.
    pub failed: usize,
    /// Sum of the allocated size of every **confirmed-deleted** file, taken
    /// before the delete. This is what those files occupied — not a
    /// measurement of what the volume gave back.
    pub bytes_removed_estimate: u64,
    /// Bytes belonging to deleted files whose reclaim could not be promised,
    /// because the hard-link count could not be read.
    pub bytes_uncertain: u64,
    /// The pre-delete estimate for every item that reached the delete stage,
    /// including the ones that failed. Divergence is shown, not hidden.
    pub bytes_estimated: u64,
    pub failures: Vec<ScanFailure>,
    pub refused_items: Vec<PlanItem>,
    pub finished_unix: u64,
    /// Observed free-space change per volume. An observation of the whole
    /// volume, never a per-file guarantee.
    pub volumes: Vec<VolumeDelta>,
    /// One sentence stating exactly what the numbers above mean.
    pub note: String,
}

/// Free-space movement on one volume across the batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeDelta {
    /// The volume root, for example `C:\`.
    pub root: String,
    pub free_before: Option<u64>,
    pub free_after: Option<u64>,
    /// `after - before`. Positive means the volume reported more free space.
    /// Negative means it reported less, which happens when other software
    /// writes while the batch runs.
    pub delta: Option<i64>,
    /// Files removed from this volume.
    pub files_removed: usize,
}

/// Stable identifier for a duplicate group: hash + size + member count.
pub fn duplicate_set_id(sha256: &str, size: u64, copies: usize) -> String {
    format!("{}:{}:{}", &sha256[..sha256.len().min(16)], size, copies)
}
