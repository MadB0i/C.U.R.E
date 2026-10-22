//! The shareable case file.
//!
//! This module builds the only artefact C.U.R.E ever produces for sharing, and
//! it is built from an **allowlist**: the struct below has no field that can
//! hold a path, a user name, a machine name, a file name or a volume label.
//! There is nowhere for private data to go, because there is no slot for it.
//!
//! Everything that reaches the exported image is either:
//!
//! * a number the engine measured (bytes, counts, durations);
//! * a label from the engine's own fixed vocabulary (`StorageClass::label`,
//!   `SafetyTier::label`); or
//! * a short note the operator typed, which is checked for anything that
//!   looks like a path or an address before it is allowed through.
//!
//! The exported image is a rendering of this struct. It is never a
//! screenshot of the application, so it cannot pick up a window title, a
//! tooltip or a neighbouring window by accident.

use serde::{Deserialize, Serialize};

use super::detective::{Investigation, LeadKind, Significance};
use super::human_bytes;
use super::types::CleanupReceipt;

/// How much detail the exported image carries.
///
/// The safest setting is the default. A more detailed case file is useful and
/// still private, but the operator has to ask for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    /// Headline figures and category names. Nothing else.
    Minimal,
    /// Adds item counts and the coverage note.
    Detailed,
}

impl PrivacyLevel {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "detailed" => PrivacyLevel::Detailed,
            _ => PrivacyLevel::Minimal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PrivacyLevel::Minimal => "Minimal",
            PrivacyLevel::Detailed => "Detailed",
        }
    }
}

/// Where the case stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    /// The investigation finished. Nothing is waiting on the operator.
    InvestigationComplete,
    /// There is storage the operator could still review and decide about.
    ReviewPending,
    /// Files were removed **and** the volume was observed to have more free
    /// space afterwards.
    ///
    /// This status exists only when a receipt carries an observed free-space
    /// movement. It is never used for a requested, queued, interrupted,
    /// inconclusive or failed cleanup.
    RecoveryVerified,
}

impl CaseStatus {
    pub fn label(self) -> &'static str {
        match self {
            CaseStatus::InvestigationComplete => "Investigation complete",
            CaseStatus::ReviewPending => "Review pending",
            CaseStatus::RecoveryVerified => "Recovery verified",
        }
    }
}

/// One category on the case file.
///
/// The label comes from the engine's fixed class vocabulary. There is no path,
/// no file name and no folder name in this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseCategory {
    pub label: String,
    pub band_label: String,
    pub bytes: u64,
    pub bytes_label: String,
    /// Share of the measured total, 0.0 to 1.0.
    pub share: f64,
    /// Only present at the detailed privacy level.
    #[serde(default)]
    pub item_count: Option<u64>,
}

/// The shareable case file.
///
/// Every field here is safe to render onto an image that leaves the machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseFile {
    /// A short, non-reversible identifier. It carries no part of the scan id,
    /// no path and no machine identity.
    pub case_id: String,
    pub title: String,
    pub status: CaseStatus,
    pub status_label: String,
    pub generated_unix: u64,
    pub measured_bytes: u64,
    pub measured_label: String,
    /// Bytes the engine could offer for removal after review.
    pub candidate_bytes: u64,
    pub candidate_label: String,
    /// Files the engine confirmed it removed. `None` when nothing was removed.
    pub files_removed: Option<u32>,
    /// The size those files occupied before they were removed.
    pub bytes_removed_estimate: Option<u64>,
    pub bytes_removed_label: Option<String>,
    /// Free space the volume was **observed** to have gained.
    ///
    /// `None` unless a receipt carries a real free-space reading. This is the
    /// only figure the case file calls "verified", because it is the only one
    /// that was observed rather than estimated.
    pub verified_recovered_bytes: Option<u64>,
    pub verified_recovered_label: Option<String>,
    pub categories: Vec<CaseCategory>,
    /// The notable discoveries, reduced to descriptions that cannot carry a
    /// file name or a folder name.
    pub notable_leads: Vec<String>,
    pub lead_count: usize,
    pub notable_count: usize,
    /// Whether the measurement could see everything it set out to see.
    pub completeness: String,
    /// A short note the operator wrote. Only present when they asked for it
    /// and the text passed the privacy check.
    pub summary: Option<String>,
    /// `true` when the note was dropped because it looked like it carried
    /// something private.
    pub summary_withheld: bool,
    pub privacy: PrivacyLevel,
    /// `true` for a synthetic demonstration. A demo case file always says so.
    pub demo: bool,
}

/// Build the case file for an investigation.
///
/// `receipt` is the cleanup receipt, when one exists. Without it the case
/// file can never claim that anything was recovered.
///
/// A receipt is only honoured when it belongs to the same scan the
/// investigation came from. A receipt from a different measurement is
/// ignored rather than reported.
pub fn build(
    investigation: &Investigation,
    receipt: Option<&CleanupReceipt>,
    summary: Option<&str>,
    include_summary: bool,
    privacy: PrivacyLevel,
) -> CaseFile {
    let receipt = receipt.filter(|r| r.scan_id == investigation.scan_id);
    let (summary_text, summary_withheld) = match (include_summary, summary) {
        (true, Some(text)) => match sanitize_summary(text) {
            Some(clean) => (Some(clean), false),
            None => (None, true),
        },
        _ => (None, false),
    };

    let mut categories: Vec<CaseCategory> = investigation
        .map
        .slices
        .iter()
        .map(|slice| CaseCategory {
            label: slice.label.clone(),
            band_label: slice.band.label().to_string(),
            bytes: slice.bytes_allocated,
            bytes_label: slice.bytes_allocated_label.clone(),
            share: slice.share,
            item_count: match privacy {
                PrivacyLevel::Detailed => Some(slice.item_count),
                PrivacyLevel::Minimal => None,
            },
        })
        .collect();
    categories.sort_by_key(|c| std::cmp::Reverse(c.bytes));

    // Verified recovery needs an observation, not a request.
    let observed_gain: Option<u64> = receipt.and_then(|r| {
        let gain: i64 = r
            .volumes
            .iter()
            .filter_map(|v| v.delta)
            .filter(|d| *d > 0)
            .sum();
        if gain > 0 {
            Some(gain as u64)
        } else {
            None
        }
    });

    let files_removed = receipt.filter(|r| r.deleted > 0).map(|r| r.deleted as u32);
    let bytes_removed_estimate = receipt
        .filter(|r| r.deleted > 0)
        .map(|r| r.bytes_removed_estimate);

    let status = if observed_gain.is_some() {
        CaseStatus::RecoveryVerified
    } else if investigation.map.candidate_bytes > 0 {
        CaseStatus::ReviewPending
    } else {
        CaseStatus::InvestigationComplete
    };

    // A short identifier derived from the scan id. It identifies this case
    // inside one investigation and nothing else.
    let case_id = short_id(&investigation.scan_id);

    CaseFile {
        case_id,
        title: "THE MISSING STORAGE CASE".to_string(),
        status,
        status_label: status.label().to_string(),
        generated_unix: investigation.generated_unix,
        measured_bytes: investigation.map.measured_bytes,
        measured_label: investigation.map.measured_bytes_label.clone(),
        candidate_bytes: investigation.map.candidate_bytes,
        candidate_label: investigation.map.candidate_label.clone(),
        files_removed,
        bytes_removed_estimate,
        bytes_removed_label: bytes_removed_estimate.map(human_bytes),
        verified_recovered_bytes: observed_gain,
        verified_recovered_label: observed_gain.map(human_bytes),
        categories,
        notable_leads: public_lead_labels(investigation),
        lead_count: investigation.leads.len(),
        notable_count: investigation
            .leads
            .iter()
            .filter(|l| l.significance != Significance::Ordinary)
            .count(),
        completeness: investigation.map.completeness.clone(),
        summary: summary_text,
        summary_withheld,
        privacy,
        demo: false,
    }
}

/// The notable leads, reduced to the facts a case file may show.
///
/// A lead's title is either an engine class label, a folder name or an
/// application name. The first is safe; the other two are not, so they are
/// replaced by a description of the *kind* of lead instead.
pub fn public_lead_labels(investigation: &Investigation) -> Vec<String> {
    let mut labels = Vec::new();
    for lead in investigation
        .leads
        .iter()
        .filter(|l| l.significance != Significance::Ordinary)
    {
        let label = match lead.kind {
            LeadKind::Category => lead.title.clone(),
            LeadKind::Location => "A large measured folder".to_string(),
            LeadKind::DuplicateSet => "Verified duplicate copies".to_string(),
            LeadKind::DormantApp => "Dormant application data".to_string(),
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

/// Keep an operator's note only when it cannot carry anything private.
///
/// The check is deliberately blunt: a note that contains anything resembling a
/// Windows path, a UNC path or an address is dropped rather than published.
/// The operator can always retype it without the private part.
pub fn sanitize_summary(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > 160 {
        return None;
    }
    // Control characters are never part of a note a human meant to write.
    if trimmed.chars().any(|c| c.is_control()) {
        return None;
    }
    let lower = trimmed.to_lowercase();

    // A drive letter, a UNC path, or a file scheme.
    let has_drive = lower
        .chars()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|w| w[0].is_ascii_alphabetic() && w[1] == ':');
    if has_drive {
        return None;
    }
    if trimmed.contains('\\') {
        return None;
    }
    if lower.contains("file://") || lower.contains("\\\\") {
        return None;
    }
    // A `Users\someone` segment, or an `@` that turns it into an address.
    if lower.contains("users\\") || lower.contains("/users/") {
        return None;
    }
    if trimmed.contains('@') {
        return None;
    }
    // Collapse whitespace so the rendered line cannot be used to hide text.
    let collapsed: Vec<&str> = trimmed.split_whitespace().collect();
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.join(" "))
}

/// A short, non-reversible identifier for a case.
///
/// FNV-1a over the scan id. It is not a security primitive; it exists so the
/// exported image can carry a case number that does not echo anything about
/// the machine it came from.
fn short_id(seed: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", hash)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::detective::investigate;
    use crate::storage::types::{
        CleanupReceipt, FileRecord, FolderRecord, SafetyTier, ScanCoverage, ScanRoot, StorageClass,
        StorageScan, VolumeDelta,
    };

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    /// A scan whose every string is private: a user name, a file name, a
    /// machine name and a volume label.
    fn private_scan() -> StorageScan {
        let files = vec![
            FileRecord {
                path: std::path::PathBuf::from(
                    "C:\\Users\\janedoe\\AppData\\Local\\Temp\\Salary_2024.xlsx",
                ),
                size_logical: 4 * GIB,
                size_allocated: Some(4 * GIB),
                modified_unix: Some(1_700_000_000),
                accessed_unix: None,
                created_unix: None,
                extension: "xlsx".to_string(),
                hard_link_count: Some(1),
                is_reparse_point: false,
                is_read_only: false,
                is_hidden: false,
                is_system: false,
                class: StorageClass::UserTemp,
                tier: SafetyTier::SafeToReview,
                reasons: vec!["in a location the operating system treats as temporary".to_string()],
            },
            FileRecord {
                path: std::path::PathBuf::from(
                    "C:\\Users\\janedoe\\Projects\\SecretProject\\target\\build.bin",
                ),
                size_logical: 2 * GIB,
                size_allocated: Some(2 * GIB),
                modified_unix: Some(1_700_000_000),
                accessed_unix: None,
                created_unix: None,
                extension: "bin".to_string(),
                hard_link_count: Some(1),
                is_reparse_point: false,
                is_read_only: false,
                is_hidden: false,
                is_system: false,
                class: StorageClass::DevCache,
                tier: SafetyTier::ReviewCarefully,
                reasons: vec!["developer build output — regenerated by the toolchain".to_string()],
            },
        ];
        let summaries = crate::storage::summarize(&files);
        StorageScan {
            scan_id: "scan-abc123".to_string(),
            started_unix: 0,
            finished_unix: 1_700_000_000,
            roots: vec![ScanRoot {
                path: std::path::PathBuf::from("C:\\Users\\janedoe\\AppData\\Local\\Temp"),
                class: StorageClass::UserTemp,
                is_catalogue: true,
                label: "User temp files".to_string(),
            }],
            files,
            folders: vec![FolderRecord {
                path: std::path::PathBuf::from(
                    "C:\\Users\\janedoe\\Projects\\SecretProject\\target",
                ),
                depth: 1,
                size_logical: 2 * GIB,
                size_allocated: 2 * GIB,
                file_count: 10,
                dir_count: 1,
                class: StorageClass::DevCache,
                tier: SafetyTier::ReviewCarefully,
                incomplete: false,
            }],
            summaries,
            coverage: ScanCoverage {
                files_seen: 2,
                dirs_seen: 2,
                volume_label: Some("JANEDOE-BACKUP".to_string()),
                volume_free_bytes: Some(10 * GIB),
                volume_total_bytes: Some(500 * GIB),
                ..Default::default()
            },
            duplicate_coverage: Default::default(),
            large_files: vec![],
            large_folders: vec![],
            duplicates: vec![],
            abandoned: vec![],
            safe_reclaimable_bytes: 4 * GIB,
            careful_reclaimable_bytes: 2 * GIB,
            protected_bytes: 0,
            protected_only_bytes: 0,
            unknown_bytes: 0,
            duplicate_reclaimable_bytes: 0,
            duplicate_bulk_reclaimable_bytes: 0,
            safe_file_count: 1,
            careful_file_count: 1,
            protected_file_count: 0,
            distinct_file_count: 2,
        }
    }

    fn receipt_with_delta(delta: i64) -> CleanupReceipt {
        CleanupReceipt {
            scan_id: "scan-abc123".to_string(),
            planned: 1,
            attempted: 1,
            deleted: 1,
            refused: 0,
            failed: 0,
            bytes_removed_estimate: 4 * GIB,
            bytes_uncertain: 0,
            bytes_estimated: 4 * GIB,
            failures: vec![],
            refused_items: vec![],
            finished_unix: 1_700_000_100,
            volumes: vec![VolumeDelta {
                root: "C:\\".to_string(),
                free_before: Some(10 * GIB),
                free_after: Some(10 * GIB + 4 * GIB),
                delta: Some(delta),
                files_removed: 1,
            }],
            note: "Estimated figures are pre-removal sizes.".to_string(),
        }
    }

    // -- privacy --------------------------------------------------------------

    #[test]
    fn the_case_file_carries_no_path_no_user_name_and_no_file_name() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Detailed);
        let json = serde_json::to_string(&case).expect("the case file serialises");

        for secret in [
            "janedoe",
            "Salary_2024",
            "SecretProject",
            "xlsx",
            "Users",
            "AppData",
            "C:\\",
            "JANEDOE-BACKUP",
            "scan-abc123",
        ] {
            assert!(
                !json.contains(secret),
                "the exported case file leaked {secret:?}: {json}"
            );
        }
    }

    #[test]
    fn the_case_file_carries_no_file_system_separator_at_all() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        let json = serde_json::to_string(&case).expect("the case file serialises");
        assert!(!json.contains('\\'), "a backslash in the case file: {json}");
    }

    #[test]
    fn the_case_id_does_not_echo_the_scan_id() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        assert_ne!(case.case_id, scan.scan_id);
        assert!(!case.case_id.contains("scan-abc123"));
        assert_eq!(case.case_id.len(), 16);
    }

    #[test]
    fn the_case_file_only_shows_engine_vocabulary_for_categories() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Detailed);
        let allowed: Vec<&str> = StorageClass::ALL.iter().map(|c| c.label()).collect();
        for category in &case.categories {
            assert!(
                allowed.contains(&category.label.as_str()),
                "unexpected category label {:?}",
                category.label
            );
        }
    }

    #[test]
    fn a_lead_title_is_never_used_verbatim_in_the_case_file() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Detailed);
        let json = serde_json::to_string(&case).expect("the case file serialises");
        assert!(
            !json.contains("SecretProject"),
            "a folder name reached the case file: {json}"
        );
        assert!(
            !json.contains("target"),
            "a folder name reached the case file: {json}"
        );
    }

    #[test]
    fn a_notable_lead_is_described_without_naming_its_folder() {
        let mut scan = private_scan();
        scan.large_folders = vec![FolderRecord {
            path: std::path::PathBuf::from("C:\\Users\\janedoe\\Projects\\SecretProject\\target"),
            depth: 1,
            size_logical: 2 * GIB,
            size_allocated: 2 * GIB,
            file_count: 10,
            dir_count: 1,
            class: StorageClass::DevCache,
            tier: SafetyTier::ReviewCarefully,
            incomplete: false,
        }];
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        let json = serde_json::to_string(&case).expect("the case file serialises");
        assert!(
            !json.contains("target"),
            "a folder name reached the case file: {json}"
        );
        assert!(
            case.notable_leads
                .iter()
                .any(|l| l == "A large measured folder"),
            "a location lead is described, not named: {:?}",
            case.notable_leads
        );
    }

    #[test]
    fn repeated_location_leads_are_summarised_once() {
        let mut scan = private_scan();
        scan.large_folders = vec![
            FolderRecord {
                path: std::path::PathBuf::from(
                    "C:\\Users\\janedoe\\Projects\\SecretProject\\target",
                ),
                depth: 1,
                size_logical: 2 * GIB,
                size_allocated: 2 * GIB,
                file_count: 10,
                dir_count: 1,
                class: StorageClass::DevCache,
                tier: SafetyTier::ReviewCarefully,
                incomplete: false,
            },
            FolderRecord {
                path: std::path::PathBuf::from("C:\\Users\\janedoe\\Projects\\OtherProject\\dist"),
                depth: 1,
                size_logical: 2 * GIB,
                size_allocated: 2 * GIB,
                file_count: 10,
                dir_count: 1,
                class: StorageClass::DevCache,
                tier: SafetyTier::ReviewCarefully,
                incomplete: false,
            },
        ];
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        assert_eq!(
            case.notable_leads
                .iter()
                .filter(|l| l.as_str() == "A large measured folder")
                .count(),
            1,
            "location leads are summarised once: {:?}",
            case.notable_leads
        );
    }

    // -- summaries ------------------------------------------------------------

    #[test]
    fn an_operator_note_is_kept_when_it_is_harmless() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(
            &investigation,
            None,
            Some("My build caches had grown for months."),
            true,
            PrivacyLevel::Minimal,
        );
        assert_eq!(
            case.summary.as_deref(),
            Some("My build caches had grown for months.")
        );
        assert!(!case.summary_withheld);
    }

    #[test]
    fn an_operator_note_that_looks_like_a_path_is_withheld() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        for note in [
            "It was in C:\\Users\\janedoe\\Downloads",
            "see \\\\fileserver\\share",
            "mail me at janedoe@example.com",
            "file:///C:/secret",
            "Users\\janedoe",
        ] {
            let case = build(
                &investigation,
                None,
                Some(note),
                true,
                PrivacyLevel::Minimal,
            );
            assert!(case.summary.is_none(), "a private note survived: {note}");
            assert!(case.summary_withheld);
        }
    }

    #[test]
    fn a_note_is_not_included_unless_it_was_asked_for() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(
            &investigation,
            None,
            Some("a harmless note"),
            false,
            PrivacyLevel::Minimal,
        );
        assert!(case.summary.is_none());
        assert!(
            !case.summary_withheld,
            "nothing was withheld; it was never asked for"
        );
    }

    // -- status ---------------------------------------------------------------

    #[test]
    fn without_a_receipt_nothing_can_be_verified() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        assert!(case.verified_recovered_bytes.is_none());
        assert!(case.files_removed.is_none());
        assert_eq!(case.status, CaseStatus::ReviewPending);
    }

    #[test]
    fn an_observed_free_space_gain_is_the_only_thing_that_verifies_recovery() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(
            &investigation,
            Some(&receipt_with_delta(4 * GIB as i64)),
            None,
            false,
            PrivacyLevel::Minimal,
        );
        assert_eq!(case.status, CaseStatus::RecoveryVerified);
        assert_eq!(case.verified_recovered_bytes, Some(4 * GIB));
        assert_eq!(case.files_removed, Some(1));
    }

    #[test]
    fn a_removal_without_an_observed_gain_is_not_reported_as_verified() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let case = build(
            &investigation,
            Some(&receipt_with_delta(0)),
            None,
            false,
            PrivacyLevel::Minimal,
        );
        assert_ne!(
            case.status,
            CaseStatus::RecoveryVerified,
            "a request is not a verification"
        );
        assert!(case.verified_recovered_bytes.is_none());
        assert_eq!(
            case.bytes_removed_estimate,
            Some(4 * GIB),
            "the files were removed, and their pre-removal size is still reported"
        );
    }

    #[test]
    fn a_failed_cleanup_does_not_claim_recovery() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let mut receipt = receipt_with_delta(0);
        receipt.deleted = 0;
        receipt.failed = 1;
        let case = build(
            &investigation,
            Some(&receipt),
            None,
            false,
            PrivacyLevel::Minimal,
        );
        assert_ne!(case.status, CaseStatus::RecoveryVerified);
        assert!(case.files_removed.is_none());
    }

    #[test]
    fn a_receipt_from_a_different_scan_is_ignored() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let mut receipt = receipt_with_delta(4 * GIB as i64);
        receipt.scan_id = "scan-a-completely-different-measurement".to_string();
        let case = build(
            &investigation,
            Some(&receipt),
            None,
            false,
            PrivacyLevel::Minimal,
        );
        assert!(
            case.verified_recovered_bytes.is_none(),
            "a receipt from another measurement must not verify this one"
        );
        assert_ne!(case.status, CaseStatus::RecoveryVerified);
    }

    #[test]
    fn an_investigation_with_nothing_to_review_is_complete() {
        let mut scan = private_scan();
        scan.safe_reclaimable_bytes = 0;
        scan.careful_reclaimable_bytes = 0;
        let investigation = investigate(&scan);
        let case = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        assert_eq!(case.status, CaseStatus::InvestigationComplete);
    }

    // -- privacy levels -------------------------------------------------------

    #[test]
    fn the_default_privacy_level_shows_the_least() {
        let scan = private_scan();
        let investigation = investigate(&scan);
        let minimal = build(&investigation, None, None, false, PrivacyLevel::Minimal);
        let detailed = build(&investigation, None, None, false, PrivacyLevel::Detailed);
        assert!(minimal.categories.iter().all(|c| c.item_count.is_none()));
        assert!(detailed.categories.iter().all(|c| c.item_count.is_some()));
    }

    #[test]
    fn an_unknown_privacy_level_falls_back_to_the_safest_setting() {
        assert_eq!(PrivacyLevel::parse("detailed"), PrivacyLevel::Detailed);
        assert_eq!(PrivacyLevel::parse("DETAILED"), PrivacyLevel::Detailed);
        assert_eq!(PrivacyLevel::parse("anything else"), PrivacyLevel::Minimal);
        assert_eq!(PrivacyLevel::parse(""), PrivacyLevel::Minimal);
    }
}
