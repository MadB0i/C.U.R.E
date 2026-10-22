//! The investigation model.
//!
//! A scan produces *measurements*. This module turns those measurements into
//! **leads**: ranked, evidence-backed observations an operator can interrogate.
//!
//! Two rules govern everything in this file:
//!
//! 1. **A lead is a reading of a measurement, never a second measurement.**
//!    Nothing here touches the filesystem. Every figure comes from a
//!    [`StorageScan`] the engine already produced.
//! 2. **A lead never says more than the evidence supports.** Where the
//!    measurement does not identify what wrote a file, the lead says the
//!    origin is unknown. Where a figure is a floor rather than a total, the
//!    lead says so.
//!
//! The board is deliberately layered so that overlapping views can never be
//! added together by accident:
//!
//! * the **storage map** is built from the engine's per-class summaries,
//!   which partition the measurement, so its slices add up to exactly what
//!   was measured;
//! * the **leads** are individual observations. A lead that sits *inside*
//!   another lead says so, and carries the identifier of the lead it belongs
//!   to.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::types::{
    Certainty, DuplicateSet, FileRecord, FolderRecord, SafetyTier, StorageClass, StorageScan,
};
use super::human_bytes;

// ---------------------------------------------------------------------------
// Thresholds — the whole significance rule, in one place
// ---------------------------------------------------------------------------

/// A lead must be at least this large *and* this large a share of what was
/// measured before it counts as notable.
///
/// Both conditions are required on purpose. A 900 MB file on a 2 TB disk is
/// large; it is not a concentration. A 40 MB folder on a 500 MB disk is a
/// large share; it is not a concentration either.
pub const NOTABLE_MIN_BYTES: u64 = 64 * 1024 * 1024;
/// 5% of the measured total.
pub const NOTABLE_MIN_SHARE: f64 = 0.05;

/// The threshold for the single most concentrated lead in a measurement.
pub const MAJOR_MIN_BYTES: u64 = 512 * 1024 * 1024;
/// 15% of the measured total.
pub const MAJOR_MIN_SHARE: f64 = 0.15;

/// How many location leads the board carries at once.
///
/// The board is a reading aid, not an inventory. The full measurement stays
/// in the workstation.
pub const MAX_LOCATION_LEADS: usize = 24;
/// How many duplicate leads the board carries.
pub const MAX_DUPLICATE_LEADS: usize = 8;
/// How many dormant-application leads the board carries.
pub const MAX_DORMANT_LEADS: usize = 6;

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// What kind of evidence a lead is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeadKind {
    /// One storage category, measured across every root.
    Category,
    /// One directory, measured on disk.
    Location,
    /// One verified byte-identical set.
    DuplicateSet,
    /// One dormant application-data directory.
    DormantApp,
}

/// How much of the measurement a lead accounts for.
///
/// This is a statement about *size*, never about risk. A lead can be major
/// and protected at the same time, and the board shows both facts side by
/// side rather than letting one colour imply the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Significance {
    /// Measured, listed, and worth reading.
    Ordinary,
    /// Big enough and concentrated enough to change the picture.
    Notable,
    /// The largest concentration in this measurement.
    Major,
}

impl Significance {
    pub fn label(self) -> &'static str {
        match self {
            Significance::Ordinary => "Ordinary",
            Significance::Notable => "Notable",
            Significance::Major => "Major",
        }
    }
}

/// How the engine treats the paths behind a lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeadRisk {
    /// Regenerable. The only band that may appear in a bulk action.
    Safe,
    /// Real data. Every item is confirmed on its own.
    ReviewRequired,
    /// Off-limits, positively identified.
    Protected,
    /// Off-limits, not classified.
    Unknown,
}

impl LeadRisk {
    pub fn from_tier(tier: SafetyTier) -> Self {
        match tier {
            SafetyTier::SafeToReview => LeadRisk::Safe,
            SafetyTier::ReviewCarefully => LeadRisk::ReviewRequired,
            SafetyTier::Protected => LeadRisk::Protected,
            SafetyTier::Unknown => LeadRisk::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LeadRisk::Safe => "Safe to review",
            LeadRisk::ReviewRequired => "Review required",
            LeadRisk::Protected => "Protected",
            LeadRisk::Unknown => "Unknown",
        }
    }
}

/// What the operator may do with a lead.
///
/// The board never removes anything. Even the strongest action hands the
/// lead to the removal workflow, which previews every target and requires an
/// explicit typed confirmation before a single byte is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeadActionKind {
    /// The lead can be inspected item by item.
    Inspect,
    /// The lead can be handed to the removal workflow for review.
    ReviewAndQueue,
    /// Nothing is offered. The engine will not touch these paths.
    NotOffered,
}

/// How sure the engine is about what produced the storage behind a lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginConfidence {
    /// The engine identified the producer directly.
    Confirmed,
    /// The engine inferred the producer from the path and the file type.
    Probable,
    /// The measurement does not identify the producer.
    Unknown,
}

impl OriginConfidence {
    pub fn label(self) -> &'static str {
        match self {
            OriginConfidence::Confirmed => "Origin identified",
            OriginConfidence::Probable => "Likely origin",
            OriginConfidence::Unknown => "Origin unknown",
        }
    }
}

/// Where the storage behind a lead came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Origin {
    pub label: String,
    pub confidence: OriginConfidence,
    /// The engine's own words about why it believes this.
    pub evidence: String,
}

/// One lead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lead {
    /// Stable within one investigation.
    pub id: String,
    pub kind: LeadKind,
    /// **What** was discovered.
    pub title: String,
    /// **Where** it was found. Shown inside the application only; it is never
    /// part of anything the application exports.
    pub location: String,
    /// A short, human-readable form of [`Lead::location`].
    pub location_label: String,
    /// **Why** it matters.
    pub significance_note: String,
    /// **Origin**, with its confidence stated.
    pub origin: Origin,
    /// **Size**: the bytes it occupies on the volume, and the bytes it
    /// appears to be. Both are shown, because for a sparse or compressed file
    /// they are different numbers and only the first one is real.
    pub bytes_allocated: u64,
    pub bytes_logical: u64,
    pub bytes_allocated_label: String,
    pub bytes_logical_label: String,
    pub item_count: u64,
    /// **Risk**.
    pub risk: LeadRisk,
    pub risk_note: String,
    /// **Action**.
    pub action_kind: LeadActionKind,
    pub action_label: String,
    /// **Consequence**.
    pub consequence: String,
    /// How much of the measurement this lead accounts for.
    pub significance: Significance,
    /// The same figure as a fraction of the measured total, 0.0 to 1.0.
    pub share: f64,
    /// Concrete facts backing the lead.
    pub evidence: Vec<String>,
    /// The storage class behind the lead, when it has one.
    pub class: Option<StorageClass>,
    pub tier: SafetyTier,
    /// When this lead sits inside another one, the identifier of that lead.
    pub within: Option<String>,
    /// A note about the scope of this lead, so overlapping figures are never
    /// added together by accident.
    pub scope_note: String,
}

/// One band of the storage map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapBand {
    /// Regenerable storage: safe to review, and the only bulk-eligible band.
    Regenerable,
    /// Real data that needs an individual decision.
    Review,
    /// Off-limits, positively identified.
    Protected,
    /// Off-limits, not classified.
    Unknown,
}

impl MapBand {
    pub fn from_tier(tier: SafetyTier) -> Self {
        match tier {
            SafetyTier::SafeToReview => MapBand::Regenerable,
            SafetyTier::ReviewCarefully => MapBand::Review,
            SafetyTier::Protected => MapBand::Protected,
            SafetyTier::Unknown => MapBand::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MapBand::Regenerable => "Regenerable",
            MapBand::Review => "Needs review",
            MapBand::Protected => "Protected",
            MapBand::Unknown => "Unknown",
        }
    }
}

/// One slice of the storage map.
///
/// The slices partition the measurement: every measured file belongs to
/// exactly one class, and every class appears at most once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapSlice {
    pub key: String,
    pub label: String,
    pub tier: SafetyTier,
    pub tier_label: String,
    pub band: MapBand,
    pub bytes_allocated: u64,
    pub bytes_logical: u64,
    pub bytes_allocated_label: String,
    pub item_count: u64,
    /// Share of the measured total, 0.0 to 1.0.
    pub share: f64,
    /// The category lead that explains this slice, when there is one.
    pub lead_id: Option<String>,
}

/// The storage map: what was measured, and where it went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMap {
    /// Everything the scan measured, allocated bytes.
    pub measured_bytes: u64,
    pub measured_bytes_label: String,
    /// Everything the scan measured, logical bytes.
    pub measured_logical_bytes: u64,
    pub measured_logical_label: String,
    /// Bytes the engine could offer for removal, after review.
    pub candidate_bytes: u64,
    pub candidate_label: String,
    /// The regenerable part of [`Self::candidate_bytes`].
    pub safe_candidate_bytes: u64,
    pub safe_candidate_label: String,
    /// The part of [`Self::candidate_bytes`] that needs an individual
    /// decision.
    pub careful_candidate_bytes: u64,
    pub careful_candidate_label: String,
    /// Bytes in paths the engine positively protects.
    pub protected_bytes: u64,
    pub protected_label: String,
    /// Bytes in paths the engine could not classify.
    pub unknown_bytes: u64,
    pub unknown_label: String,
    pub slices: Vec<MapSlice>,
    pub certainty: Certainty,
    /// Whether the totals above can be trusted wholesale.
    pub completeness: String,
}

/// The single number the board leads with, with its uncertainty attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationHeadline {
    pub measured_bytes: u64,
    pub measured_label: String,
    pub candidate_bytes: u64,
    pub candidate_label: String,
    /// A plain sentence saying what the candidate figure is and is not.
    pub candidate_note: String,
    pub lead_count: usize,
    pub notable_count: usize,
    pub completeness: String,
}

/// How much evidence the investigation rests on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceCounts {
    pub files_measured: u64,
    pub folders_measured: u64,
    pub duplicate_sets_verified: usize,
    pub duplicate_coverage_note: String,
    pub dormant_apps: usize,
    pub permission_denied: u64,
    pub links_skipped: u64,
    pub duration_ms: u64,
}

/// The whole investigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Investigation {
    pub scan_id: String,
    pub generated_unix: u64,
    pub map: StorageMap,
    pub headline: InvestigationHeadline,
    pub leads: Vec<Lead>,
    /// Identifiers of the leads that cleared the notable threshold.
    pub notable_leads: Vec<String>,
    /// The identifier of the single most concentrated lead, when one exists.
    pub major_lead: Option<String>,
    pub evidence: EvidenceCounts,
    /// Extra lines about what the scan could not see.
    pub coverage_notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// The significance rule
// ---------------------------------------------------------------------------

/// Decide how significant a lead is.
///
/// Deterministic and documented: a lead must clear **both** an absolute floor
/// and a share of the measured total. The function is pure, so the same
/// measurement always produces the same verdict, and a test can state the
/// rule as an equation.
pub fn significance_of(bytes_allocated: u64, measured_bytes: u64) -> Significance {
    if measured_bytes == 0 || bytes_allocated == 0 {
        return Significance::Ordinary;
    }
    let share = bytes_allocated as f64 / measured_bytes as f64;
    if bytes_allocated >= MAJOR_MIN_BYTES && share >= MAJOR_MIN_SHARE {
        return Significance::Major;
    }
    if bytes_allocated >= NOTABLE_MIN_BYTES && share >= NOTABLE_MIN_SHARE {
        return Significance::Notable;
    }
    Significance::Ordinary
}

// ---------------------------------------------------------------------------
// Origin
// ---------------------------------------------------------------------------

/// What the engine believes produced the storage in a class.
///
/// The label comes from the engine's own class, which is derived from the
/// path and the file type. The evidence line is the engine's own reasoning,
/// taken verbatim from the measurement rather than rewritten here.
fn origin_for_class(class: StorageClass, sample_reasons: &[String]) -> Origin {
    let (confidence, evidence) = match class {
        StorageClass::RecycleBin => (
            OriginConfidence::Confirmed,
            "measured inside the Recycle Bin".to_string(),
        ),
        StorageClass::WindowsOld => (
            OriginConfidence::Confirmed,
            "measured inside the previous Windows installation".to_string(),
        ),
        StorageClass::Other => (
            OriginConfidence::Unknown,
            "the measurement does not identify what wrote this".to_string(),
        ),
        StorageClass::UserSelected => (
            OriginConfidence::Unknown,
            "measured in a location the operator chose; the measurement does not \
             identify what wrote it"
                .to_string(),
        ),
        _ => (
            OriginConfidence::Probable,
            "inferred from the location and file type".to_string(),
        ),
    };

    let mut evidence = evidence;
    if let Some(reason) = sample_reasons.first() {
        if !reason.trim().is_empty() {
            evidence.push_str(": ");
            evidence.push_str(reason.trim());
        }
    }

    Origin {
        label: class.label().to_string(),
        confidence,
        evidence,
    }
}

// ---------------------------------------------------------------------------
// Action and consequence
// ---------------------------------------------------------------------------

fn action_for(risk: LeadRisk) -> (LeadActionKind, String) {
    match risk {
        LeadRisk::Safe => (
            LeadActionKind::ReviewAndQueue,
            "Review and queue".to_string(),
        ),
        LeadRisk::ReviewRequired => (
            LeadActionKind::ReviewAndQueue,
            "Review each item".to_string(),
        ),
        LeadRisk::Protected => (
            LeadActionKind::NotOffered,
            "No removal offered".to_string(),
        ),
        LeadRisk::Unknown => (LeadActionKind::NotOffered, "No removal offered".to_string()),
    }
}

fn consequence_for(risk: LeadRisk) -> String {
    match risk {
        LeadRisk::Safe => {
            "These are regenerable. Removing them frees space that the application rebuilds on \
             next use, and nothing unique is lost. Every target is still previewed and \
             confirmed before it is removed."
                .to_string()
        }
        LeadRisk::ReviewRequired => {
            "These are real files. Nothing here is removed unless you choose it, and the engine \
             re-checks every target against the live filesystem immediately before it goes."
                .to_string()
        }
        LeadRisk::Protected => {
            "Operating system, driver or installed application data. This engine never removes \
             these, so nothing happens if you act on this lead."
                .to_string()
        }
        LeadRisk::Unknown => {
            "The engine could not classify this with confidence, so no removal is offered. \
             Investigating it is safe; deleting it is not something this tool will help with."
                .to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// The investigation
// ---------------------------------------------------------------------------

/// Turn a completed scan into an investigation.
///
/// The scan is the only input. Nothing is re-measured, nothing is guessed,
/// and no lead claims more than the scan's own figures support.
pub fn investigate(scan: &StorageScan) -> Investigation {
    let measured_bytes: u64 = scan
        .summaries
        .iter()
        .map(|s| s.bytes_allocated)
        .fold(0u64, |a, b| a.saturating_add(b));
    let measured_logical_bytes: u64 = scan
        .summaries
        .iter()
        .map(|s| s.bytes_logical)
        .fold(0u64, |a, b| a.saturating_add(b));

    let mut leads: Vec<Lead> = Vec::new();
    let mut slices: Vec<MapSlice> = Vec::new();

    // The engine's own reasoning for the first file of each class, gathered
    // in one pass over the measurement.
    let sample_reasons = sample_reasons_by_class(scan);

    // ---- category leads and the map's slices -----------------------------
    //
    // The engine's summaries partition the measurement, so these slices add
    // up to exactly what was measured.
    let mut summaries: Vec<&super::types::ClassSummary> = scan.summaries.iter().collect();
    summaries.sort_by_key(|s| std::cmp::Reverse(s.bytes_allocated));

    for summary in &summaries {
        if summary.bytes_allocated == 0 {
            continue;
        }
        let class = summary.class;
        let risk = LeadRisk::from_tier(summary.tier);
        let share = share_of(summary.bytes_allocated, measured_bytes);
        let lead_id = format!("category:{}", class.key());

        let sample_reasons = sample_reasons
            .get(&class)
            .cloned()
            .unwrap_or_default();
        let origin = origin_for_class(class, &sample_reasons);
        let (action_kind, action_label) = action_for(risk);

        let mut evidence = Vec::new();
        evidence.push(format!(
            "{} measured in this category",
            plural(summary.item_count, "item", "items")
        ));
        evidence.push(format!(
            "{} occupies {} on the volume",
            human_bytes(summary.bytes_allocated),
            if summary.bytes_logical > summary.bytes_allocated {
                format!(" ({} of apparent length)", human_bytes(summary.bytes_logical))
            } else {
                "the volume".to_string()
            }
        ));
        if let Some(reason) = sample_reasons.first() {
            evidence.push(format!("Engine note: {}", reason.trim()));
        }
        evidence.push(format!("Category tier: {}", summary.tier.label()));

        let location = location_for_class(scan, class);
        let location_label = location_label_for_class(scan, class);

        let lead = Lead {
            id: lead_id.clone(),
            kind: LeadKind::Category,
            title: class.label().to_string(),
            location,
            location_label,
            significance_note: format!(
                "{} of the storage this scan measured, across {}.",
                share_percent(share),
                plural(summary.item_count, "item", "items")
            ),
            origin,
            bytes_allocated: summary.bytes_allocated,
            bytes_logical: summary.bytes_logical,
            bytes_allocated_label: human_bytes(summary.bytes_allocated),
            bytes_logical_label: human_bytes(summary.bytes_logical),
            item_count: summary.item_count,
            risk,
            risk_note: risk_note(risk, summary.tier),
            action_kind,
            action_label,
            consequence: consequence_for(risk),
            significance: significance_of(summary.bytes_allocated, measured_bytes),
            share,
            evidence,
            class: Some(class),
            tier: summary.tier,
            within: None,
            scope_note: "One slice of the measured storage. Slices do not overlap.".to_string(),
        };
        leads.push(lead);

        slices.push(MapSlice {
            key: class.key().to_string(),
            label: class.label().to_string(),
            tier: summary.tier,
            tier_label: summary.tier.label().to_string(),
            band: MapBand::from_tier(summary.tier),
            bytes_allocated: summary.bytes_allocated,
            bytes_logical: summary.bytes_logical,
            bytes_allocated_label: human_bytes(summary.bytes_allocated),
            item_count: summary.item_count,
            share,
            lead_id: Some(lead_id),
        });
    }

    // ---- location leads ---------------------------------------------------
    //
    // Only the outermost measured folder of each branch is used, so two
    // location leads never describe the same bytes twice.
    let mut locations = location_leads(scan, measured_bytes);
    locations.sort_by_key(|l| std::cmp::Reverse(l.bytes_allocated));
    locations.truncate(MAX_LOCATION_LEADS);
    for lead in locations.iter_mut() {
        lead.scope_note = format!(
            "Measured inside {}. This figure is part of that category's total, not an \
             addition to it.",
            lead.scope_note
        );
        leads.push(lead.clone());
    }

    // ---- verified duplicate leads ----------------------------------------
    let mut duplicates: Vec<&DuplicateSet> = scan.duplicates.iter().collect();
    duplicates.sort_by_key(|d| std::cmp::Reverse(d.reclaimable_bytes));
    for set in duplicates.iter().take(MAX_DUPLICATE_LEADS) {
        leads.push(duplicate_lead(set, measured_bytes));
    }

    // ---- dormant application leads ---------------------------------------
    let mut dormant: Vec<&super::types::AbandonedAppRecord> = scan.abandoned.iter().collect();
    dormant.sort_by_key(|r| std::cmp::Reverse(r.size_allocated));
    for record in dormant.iter().take(MAX_DORMANT_LEADS) {
        leads.push(dormant_lead(record, measured_bytes));
    }

    // ---- rank -------------------------------------------------------------
    //
    // The list is ranked by the bytes each lead accounts for. Leads that
    // overlap say so through `within`, and the board groups them by kind so
    // two different scopes are never added together.
    leads.sort_by(|a, b| {
        b.significance
            .cmp(&a.significance)
            .then(b.bytes_allocated.cmp(&a.bytes_allocated))
            .then(a.id.cmp(&b.id))
    });

    let notable_leads: Vec<String> = leads
        .iter()
        .filter(|l| l.significance != Significance::Ordinary)
        .map(|l| l.id.clone())
        .collect();
    let major_lead = leads
        .iter()
        .find(|l| l.significance == Significance::Major)
        .map(|l| l.id.clone());

    let candidate_bytes = scan
        .safe_reclaimable_bytes
        .saturating_add(scan.careful_reclaimable_bytes);
    let certainty = scan.headline().certainty;

    let map = StorageMap {
        measured_bytes,
        measured_bytes_label: human_bytes(measured_bytes),
        measured_logical_bytes,
        measured_logical_label: human_bytes(measured_logical_bytes),
        candidate_bytes,
        candidate_label: human_bytes(candidate_bytes),
        safe_candidate_bytes: scan.safe_reclaimable_bytes,
        safe_candidate_label: human_bytes(scan.safe_reclaimable_bytes),
        careful_candidate_bytes: scan.careful_reclaimable_bytes,
        careful_candidate_label: human_bytes(scan.careful_reclaimable_bytes),
        protected_bytes: scan.protected_only_bytes,
        protected_label: human_bytes(scan.protected_only_bytes),
        unknown_bytes: scan.unknown_bytes,
        unknown_label: human_bytes(scan.unknown_bytes),
        slices,
        certainty,
        completeness: scan.coverage.completeness().to_string(),
    };

    let headline = InvestigationHeadline {
        measured_bytes,
        measured_label: human_bytes(measured_bytes),
        candidate_bytes,
        candidate_label: human_bytes(candidate_bytes),
        candidate_note: if scan.safe_reclaimable_bytes > 0 {
            format!(
                "Of the storage this scan measured, {} is regenerable and {} needs an \
                 individual decision. Neither figure is a promise: every target is previewed \
                 and confirmed first.",
                human_bytes(scan.safe_reclaimable_bytes),
                human_bytes(scan.careful_reclaimable_bytes)
            )
        } else {
            "This scan found no storage it could offer for removal. Everything it measured is \
             either protected, unclassified, or would need an individual decision."
                .to_string()
        },
        lead_count: leads.len(),
        notable_count: notable_leads.len(),
        completeness: scan.coverage.completeness().to_string(),
    };

    let duplicate_coverage = &scan.duplicate_coverage;

    Investigation {
        scan_id: scan.scan_id.clone(),
        generated_unix: scan.finished_unix,
        map,
        headline,
        leads,
        notable_leads,
        major_lead,
        evidence: EvidenceCounts {
            files_measured: scan.coverage.files_seen,
            folders_measured: scan.coverage.dirs_seen,
            duplicate_sets_verified: duplicate_coverage.verified_sets,
            duplicate_coverage_note: duplicate_coverage.summary.clone(),
            dormant_apps: scan.abandoned.len(),
            permission_denied: scan
                .coverage
                .permission_denied_dirs
                .saturating_add(scan.coverage.permission_denied_files),
            links_skipped: scan.coverage.reparse_points_skipped,
            duration_ms: scan.coverage.duration_ms,
        },
        coverage_notes: scan.coverage.notes(),
    }
}

// ---------------------------------------------------------------------------
// Lead builders
// ---------------------------------------------------------------------------

/// Build the location leads from the engine's measured folders.
///
/// The engine records every folder it walked and carries forward the largest.
/// A folder that sits inside another measured folder describes the same bytes
/// from a nearer vantage point, so only the outermost one of each branch is
/// used. The result never double-counts.
fn location_leads(scan: &StorageScan, measured_bytes: u64) -> Vec<Lead> {
    let mut folders: Vec<&FolderRecord> = scan.large_folders.iter().collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.size_allocated));

    let mut chosen: Vec<&FolderRecord> = Vec::new();
    for folder in folders {
        if folder.size_allocated == 0 {
            continue;
        }
        let path = normalize_path(&folder.path);
        let nested = chosen.iter().any(|kept| {
            let kept_path = normalize_path(&kept.path);
            path.len() > kept_path.len() && path.starts_with(&kept_path)
        });
        if nested {
            continue;
        }
        chosen.push(folder);
        if chosen.len() >= MAX_LOCATION_LEADS * 3 {
            break;
        }
    }

    chosen
        .into_iter()
        .map(|folder| {
            let class = folder.class;
            let risk = LeadRisk::from_tier(folder.tier);
            let share = share_of(folder.size_allocated, measured_bytes);
            let (action_kind, action_label) = action_for(risk);
            let name = folder
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| folder.path.to_string_lossy().into_owned());

            let mut evidence = Vec::new();
            evidence.push(format!(
                "{} in this folder",
                plural(folder.file_count, "file", "files")
            ));
            evidence.push(format!(
                "{} occupies the volume",
                human_bytes(folder.size_allocated)
            ));
            if folder.size_logical > folder.size_allocated {
                evidence.push(format!(
                    "{} of apparent length, so part of it is sparse or compressed",
                    human_bytes(folder.size_logical)
                ));
            }
            if folder.incomplete {
                evidence.push(
                    "part of this folder could not be read, so its total is a floor".to_string(),
                );
            }
            evidence.push(format!("Category: {}", class.label()));

            Lead {
                id: format!("location:{}", normalize_path(&folder.path)),
                kind: LeadKind::Location,
                title: name,
                location: folder.path.to_string_lossy().into_owned(),
                location_label: class.label().to_string(),
                significance_note: format!(
                    "{} of the storage this scan measured, in one folder.",
                    share_percent(share)
                ),
                origin: origin_for_class(class, &[]),
                bytes_allocated: folder.size_allocated,
                bytes_logical: folder.size_logical,
                bytes_allocated_label: human_bytes(folder.size_allocated),
                bytes_logical_label: human_bytes(folder.size_logical),
                item_count: folder.file_count,
                risk,
                risk_note: risk_note(risk, folder.tier),
                action_kind,
                action_label,
                consequence: consequence_for(risk),
                significance: significance_of(folder.size_allocated, measured_bytes),
                share,
                evidence,
                class: Some(class),
                tier: folder.tier,
                within: Some(format!("category:{}", class.key())),
                scope_note: class.label().to_string(),
            }
        })
        .collect()
}

/// Build a lead for one verified byte-identical set.
fn duplicate_lead(set: &DuplicateSet, measured_bytes: u64) -> Lead {
    let share = share_of(set.reclaimable_bytes, measured_bytes);
    let tiers: BTreeSet<&str> = set.copies.iter().map(|c| c.tier.key()).collect();
    let risk = if tiers.contains("protected") {
        LeadRisk::Protected
    } else if tiers.contains("unknown") {
        LeadRisk::Unknown
    } else if tiers.contains("review_carefully") {
        LeadRisk::ReviewRequired
    } else {
        LeadRisk::Safe
    };
    let (action_kind, action_label) = if set.bulk_reclaimable_bytes > 0 {
        (LeadActionKind::ReviewAndQueue, "Review the extra copies")
    } else {
        (LeadActionKind::Inspect, "Compare the copies")
    };

    let mut evidence = Vec::new();
    evidence.push(format!(
        "{} copies of one file, verified by a full SHA-256 read of every copy",
        set.copies.len()
    ));
    evidence.push(format!(
        "SHA-256 {}",
        if set.sha256.len() >= 16 {
            format!("{}…", &set.sha256[..16])
        } else {
            set.sha256.clone()
        }
    ));
    evidence.push(format!(
        "{} would be freed if the extra copies went",
        human_bytes(set.reclaimable_bytes)
    ));
    if set.uncertain_copies > 0 {
        evidence.push(format!(
            "{} copy or copies could not be promised: the hard-link count could not be read",
            set.uncertain_copies
        ));
    }
    if set.protected_copies > 0 {
        evidence.push(format!(
            "{} protected copy or copies are in the set and are never touched",
            set.protected_copies
        ));
    }

    Lead {
        id: format!("duplicate:{}", set.id),
        kind: LeadKind::DuplicateSet,
        title: format!("{} identical copies of one file", set.copies.len()),
        location: set
            .copies
            .first()
            .map(|c| {
                c.path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| c.path.to_string_lossy().into_owned())
            })
            .unwrap_or_default(),
        location_label: format!("{} copies", set.copies.len()),
        significance_note: format!(
            "{} of the storage this scan measured is held by repeated copies of the same \
             content.",
            share_percent(share)
        ),
        origin: Origin {
            label: "Repeated copies of the same content".to_string(),
            confidence: OriginConfidence::Probable,
            evidence: "every copy hashed to the same SHA-256, so the content is identical; the \
                       measurement does not record which application wrote it"
                .to_string(),
        },
        bytes_allocated: set.reclaimable_bytes,
        bytes_logical: set.size_logical,
        bytes_allocated_label: human_bytes(set.reclaimable_bytes),
        bytes_logical_label: human_bytes(set.size_logical),
        item_count: set.copies.len() as u64,
        risk,
        risk_note: risk_note(risk, SafetyTier::Unknown),
        action_kind,
        action_label: action_label.to_string(),
        consequence: "The engine keeps the most recent copy and treats the rest as \
                      candidates. Removing them frees their space; the content itself still \
                      exists in the copy that was kept."
            .to_string(),
        significance: significance_of(set.reclaimable_bytes, measured_bytes),
        share,
        evidence,
        class: None,
        tier: SafetyTier::Unknown,
        within: None,
        scope_note: "Counted once, as verified duplicates. It is already part of the category \
                     totals above."
            .to_string(),
    }
}

/// Build a lead for one dormant application-data directory.
fn dormant_lead(record: &super::types::AbandonedAppRecord, measured_bytes: u64) -> Lead {
    let share = share_of(record.size_allocated, measured_bytes);
    let risk = LeadRisk::from_tier(record.tier);

    let mut evidence = Vec::new();
    evidence.push(format!(
        "{} in {} files",
        human_bytes(record.size_allocated),
        record.file_count
    ));
    if let Some(days) = record.days_since_change {
        evidence.push(format!("no change recorded for {} days", days));
    }
    evidence.push(record.caveat.clone());

    Lead {
        id: format!("dormant:{}", normalize_path(&record.path)),
        kind: LeadKind::DormantApp,
        title: record.app_name.clone(),
        location: record.path.to_string_lossy().into_owned(),
        location_label: "application data".to_string(),
        significance_note: format!(
            "{} of the storage this scan measured, from an application that has not written \
             here in a long time.",
            share_percent(share)
        ),
        origin: Origin {
            label: "Application data".to_string(),
            confidence: OriginConfidence::Probable,
            evidence: "measured inside a named application data folder".to_string(),
        },
        bytes_allocated: record.size_allocated,
        bytes_logical: record.size_logical,
        bytes_allocated_label: human_bytes(record.size_allocated),
        bytes_logical_label: human_bytes(record.size_logical),
        item_count: record.file_count,
        risk,
        risk_note: risk_note(risk, record.tier),
        action_kind: LeadActionKind::NotOffered,
        action_label: "No removal offered".to_string(),
        consequence: "Age is a weak signal. An application can keep data it never reads \
                      again, and a file that was never touched can still be the only copy. \
                      This engine never offers to remove application data."
            .to_string(),
        significance: significance_of(record.size_allocated, measured_bytes),
        share,
        evidence,
        class: Some(record.class),
        tier: record.tier,
        within: Some(format!("category:{}", record.class.key())),
        scope_note: record.class.label().to_string(),
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn share_of(bytes: u64, measured: u64) -> f64 {
    if measured == 0 {
        0.0
    } else {
        (bytes as f64 / measured as f64).clamp(0.0, 1.0)
    }
}

fn share_percent(share: f64) -> String {
    if share < 0.01 {
        "Less than 1%".to_string()
    } else {
        format!("{:.1}%", share * 100.0)
    }
}

fn plural(count: u64, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{} {}", count, one)
    } else {
        format!("{} {}", count, many)
    }
}

fn risk_note(risk: LeadRisk, tier: SafetyTier) -> String {
    match risk {
        LeadRisk::Safe => format!("Regenerable by the application that made it. {}.", tier.explanation()),
        LeadRisk::ReviewRequired => format!(
            "Real data that needs an individual decision. {}.",
            tier.explanation()
        ),
        LeadRisk::Protected => format!("Off-limits. {}.", tier.explanation()),
        LeadRisk::Unknown => format!("Unclassified. {}.", tier.explanation()),
    }
}

/// The engine's own reasoning for the first file of each class.
///
/// One pass over the measurement, so the cost does not grow with the number
/// of classes.
fn sample_reasons_by_class(scan: &StorageScan) -> BTreeMap<StorageClass, Vec<String>> {
    let mut out: BTreeMap<StorageClass, Vec<String>> = BTreeMap::new();
    for file in &scan.files {
        out.entry(file.class)
            .or_insert_with(|| file.reasons.clone());
    }
    out
}

/// Where a class was measured. Shown inside the application only.
fn location_for_class(scan: &StorageScan, class: StorageClass) -> String {
    let roots: Vec<String> = scan
        .roots
        .iter()
        .filter(|r| r.class == class)
        .map(|r| r.path.to_string_lossy().into_owned())
        .collect();
    if roots.is_empty() {
        "Measured across the scanned locations".to_string()
    } else {
        roots.join("\n")
    }
}

fn location_label_for_class(scan: &StorageScan, class: StorageClass) -> String {
    let count = scan.roots.iter().filter(|r| r.class == class).count();
    match count {
        0 => "Scanned locations".to_string(),
        1 => "1 scanned location".to_string(),
        n => format!("{} scanned locations", n),
    }
}

/// Lower-case, forward-slash form of a path, for comparisons and identifiers.
fn normalize_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// The largest measured file in a class, used as supporting evidence.
pub fn largest_file_in(scan: &StorageScan, class: StorageClass) -> Option<&FileRecord> {
    scan.large_files.iter().find(|f| f.class == class)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::{
        ClassSummary, DuplicateCopy, FileRecord, FolderRecord, ScanCoverage, ScanRoot,
    };

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    fn file(
        path: &str,
        logical: u64,
        allocated: u64,
        class: StorageClass,
        tier: SafetyTier,
    ) -> FileRecord {
        FileRecord {
            path: std::path::PathBuf::from(path),
            size_logical: logical,
            size_allocated: Some(allocated),
            modified_unix: Some(1_700_000_000),
            accessed_unix: None,
            created_unix: None,
            extension: String::new(),
            hard_link_count: Some(1),
            is_reparse_point: false,
            is_read_only: false,
            is_hidden: false,
            is_system: false,
            class,
            tier,
            reasons: vec!["classified as safe to review".to_string()],
        }
    }

    fn folder(
        path: &str,
        logical: u64,
        allocated: u64,
        files: u64,
        class: StorageClass,
        tier: SafetyTier,
    ) -> FolderRecord {
        FolderRecord {
            path: std::path::PathBuf::from(path),
            depth: 1,
            size_logical: logical,
            size_allocated: allocated,
            file_count: files,
            dir_count: 1,
            class,
            tier,
            incomplete: false,
        }
    }

    fn scan_from(files: Vec<FileRecord>) -> StorageScan {
        let summaries = super::super::summarize(&files);
        let accounting = crate::storage::reclaim::compute(&files, &[]);
        StorageScan {
            scan_id: "scan-test".to_string(),
            started_unix: 0,
            finished_unix: 1_700_000_000,
            roots: vec![ScanRoot {
                path: std::path::PathBuf::from("C:\\Users\\tester\\AppData\\Local\\Temp"),
                class: StorageClass::UserTemp,
                is_catalogue: true,
                label: "User temp files".to_string(),
            }],
            files,
            folders: vec![],
            summaries,
            coverage: ScanCoverage {
                files_seen: 1,
                dirs_seen: 1,
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

    // -- the significance rule ------------------------------------------------

    #[test]
    fn significance_needs_both_a_floor_and_a_share() {
        // Big in absolute terms, tiny as a share: not notable.
        assert_eq!(
            significance_of(900 * MIB, 40 * GIB),
            Significance::Ordinary,
            "a 900 MB lead on a 40 GB measurement is not a concentration"
        );
        // A big share, but small in absolute terms: not notable.
        assert_eq!(
            significance_of(40 * MIB, 100 * MIB),
            Significance::Ordinary,
            "a 40% share of a 100 MB measurement is not a concentration"
        );
        // Clears both.
        assert_eq!(
            significance_of(2 * GIB, 20 * GIB),
            Significance::Notable,
            "2 GB of a 20 GB measurement clears both thresholds"
        );
        assert_eq!(
            significance_of(5 * GIB, 20 * GIB),
            Significance::Major,
            "5 GB of a 20 GB measurement is a major concentration"
        );
    }

    #[test]
    fn significance_is_deterministic() {
        for _ in 0..8 {
            assert_eq!(
                significance_of(1234 * MIB, 9 * GIB),
                significance_of(1234 * MIB, 9 * GIB)
            );
        }
    }

    #[test]
    fn significance_of_nothing_is_ordinary() {
        assert_eq!(significance_of(0, 0), Significance::Ordinary);
        assert_eq!(significance_of(10 * GIB, 0), Significance::Ordinary);
    }

    // -- classification and aggregation --------------------------------------

    #[test]
    fn empty_scan_produces_an_investigation_with_no_leads() {
        let scan = scan_from(vec![]);
        let investigation = investigate(&scan);
        assert!(investigation.leads.is_empty(), "an empty scan has no leads");
        assert_eq!(investigation.map.measured_bytes, 0);
        assert_eq!(investigation.headline.candidate_bytes, 0);
        assert!(investigation.map.slices.is_empty());
        assert!(investigation.major_lead.is_none());
        assert!(investigation.notable_leads.is_empty());
    }

    #[test]
    fn categories_are_grouped_and_the_map_adds_up() {
        let scan = scan_from(vec![
            file("C:\\Temp\\a.tmp", 4 * GIB, 4 * GIB, StorageClass::UserTemp, SafetyTier::SafeToReview),
            file("C:\\Temp\\b.tmp", GIB, GIB, StorageClass::UserTemp, SafetyTier::SafeToReview),
            file("C:\\Win\\System32\\drivers\\x.sys", 3 * GIB, 3 * GIB, StorageClass::Other, SafetyTier::Protected),
            file("D:\\Build\\target\\big.bin", 2 * GIB, 2 * GIB, StorageClass::DevCache, SafetyTier::ReviewCarefully),
        ]);
        let investigation = investigate(&scan);

        assert_eq!(investigation.map.slices.len(), 3, "three classes carry bytes");
        let summed: u64 = investigation.map.slices.iter().map(|s| s.bytes_allocated).sum();
        assert_eq!(
            summed, investigation.map.measured_bytes,
            "the map partitions the measurement: its slices add up to what was measured"
        );
        assert_eq!(investigation.map.measured_bytes, 10 * GIB);

        let temp = investigation
            .map
            .slices
            .iter()
            .find(|s| s.key == "user_temp")
            .expect("the temp category is on the map");
        assert_eq!(temp.bytes_allocated, 5 * GIB);
        assert_eq!(temp.item_count, 2);
        assert_eq!(temp.band, MapBand::Regenerable);
    }

    #[test]
    fn logical_and_allocated_sizes_are_both_preserved() {
        // A sparse file: 8 GB of apparent length, 1 GB actually occupied.
        let scan = scan_from(vec![file(
            "C:\\Temp\\sparse.vhd",
            8 * GIB,
            GIB,
            StorageClass::UserTemp,
            SafetyTier::SafeToReview,
        )]);
        let investigation = investigate(&scan);
        let lead = investigation
            .leads
            .iter()
            .find(|l| l.kind == LeadKind::Category)
            .expect("a category lead");
        assert_eq!(lead.bytes_allocated, GIB, "allocated bytes are the real figure");
        assert_eq!(lead.bytes_logical, 8 * GIB, "the apparent length is kept beside it");
        assert!(lead
            .evidence
            .iter()
            .any(|e| e.contains("apparent length")));
    }

    // -- origin attribution ---------------------------------------------------

    #[test]
    fn origin_is_known_for_a_class_the_engine_identified() {
        let scan = scan_from(vec![file(
            "C:\\$Recycle.Bin\\S-1-5-21\\x.bin",
            GIB,
            GIB,
            StorageClass::RecycleBin,
            SafetyTier::SafeToReview,
        )]);
        let investigation = investigate(&scan);
        let lead = &investigation.leads[0];
        assert_eq!(lead.origin.confidence, OriginConfidence::Confirmed);
        assert!(lead.origin.evidence.contains("Recycle Bin"));
    }

    #[test]
    fn origin_is_unknown_when_the_measurement_says_nothing() {
        let scan = scan_from(vec![file(
            "D:\\Things\\mystery.dat",
            GIB,
            GIB,
            StorageClass::Other,
            SafetyTier::Unknown,
        )]);
        let investigation = investigate(&scan);
        let lead = &investigation.leads[0];
        assert_eq!(
            lead.origin.confidence,
            OriginConfidence::Unknown,
            "an unclassified path must not be given an origin"
        );
        assert!(lead.origin.evidence.contains("does not identify"));
    }

    #[test]
    fn origin_carries_the_engines_own_reasoning() {
        let mut f = file(
            "D:\\Build\\target\\x.bin",
            GIB,
            GIB,
            StorageClass::DevCache,
            SafetyTier::ReviewCarefully,
        );
        f.reasons = vec!["developer build output — regenerated by the toolchain".to_string()];
        let scan = scan_from(vec![f]);
        let investigation = investigate(&scan);
        let lead = &investigation.leads[0];
        assert_eq!(lead.origin.confidence, OriginConfidence::Probable);
        assert!(lead
            .origin
            .evidence
            .contains("developer build output — regenerated by the toolchain"));
    }

    // -- risk, action and consequence ----------------------------------------

    #[test]
    fn protected_storage_offers_no_removal() {
        let scan = scan_from(vec![file(
            "C:\\Windows\\System32\\drivers\\x.sys",
            2 * GIB,
            2 * GIB,
            StorageClass::Other,
            SafetyTier::Protected,
        )]);
        let investigation = investigate(&scan);
        let lead = &investigation.leads[0];
        assert_eq!(lead.risk, LeadRisk::Protected);
        assert_eq!(lead.action_kind, LeadActionKind::NotOffered);
        assert!(lead.consequence.contains("never removes"));
        assert_eq!(investigation.map.protected_bytes, 2 * GIB);
    }

    #[test]
    fn unknown_storage_is_reported_apart_from_protected_storage() {
        let scan = scan_from(vec![
            file(
                "C:\\Windows\\System32\\drivers\\x.sys",
                2 * GIB,
                2 * GIB,
                StorageClass::Other,
                SafetyTier::Protected,
            ),
            file(
                "D:\\Things\\mystery.dat",
                3 * GIB,
                3 * GIB,
                StorageClass::Other,
                SafetyTier::Unknown,
            ),
        ]);
        let investigation = investigate(&scan);
        assert_eq!(investigation.map.protected_bytes, 2 * GIB);
        assert_eq!(investigation.map.unknown_bytes, 3 * GIB);
        assert_ne!(
            investigation.map.protected_bytes, investigation.map.unknown_bytes,
            "we could not tell is not the same claim as we know this is critical"
        );
    }

    #[test]
    fn review_required_storage_is_never_bulk_actionable() {
        let scan = scan_from(vec![file(
            "D:\\Downloads\\setup.exe",
            2 * GIB,
            2 * GIB,
            StorageClass::OldInstaller,
            SafetyTier::ReviewCarefully,
        )]);
        let investigation = investigate(&scan);
        let lead = &investigation.leads[0];
        assert_eq!(lead.risk, LeadRisk::ReviewRequired);
        assert_ne!(
            lead.action_kind,
            LeadActionKind::NotOffered,
            "review-required storage is still offered for review"
        );
        assert_eq!(
            investigation.map.candidate_bytes,
            2 * GIB,
            "review-required bytes are candidates, but only for individual decisions"
        );
    }

    // -- location leads ------------------------------------------------------

    #[test]
    fn nested_folders_do_not_produce_two_leads_for_the_same_bytes() {
        let mut scan = scan_from(vec![file(
            "D:\\Build\\target\\x.bin",
            2 * GIB,
            2 * GIB,
            StorageClass::DevCache,
            SafetyTier::ReviewCarefully,
        )]);
        scan.large_folders = vec![
            folder(
                "D:\\Build\\target",
                2 * GIB,
                2 * GIB,
                10,
                StorageClass::DevCache,
                SafetyTier::ReviewCarefully,
            ),
            folder(
                "D:\\Build\\target\\debug",
                GIB,
                GIB,
                5,
                StorageClass::DevCache,
                SafetyTier::ReviewCarefully,
            ),
        ];
        let investigation = investigate(&scan);
        let locations: Vec<&Lead> = investigation
            .leads
            .iter()
            .filter(|l| l.kind == LeadKind::Location)
            .collect();
        assert_eq!(
            locations.len(),
            1,
            "the inner folder describes the same bytes and is dropped"
        );
        assert_eq!(locations[0].bytes_allocated, 2 * GIB);
        assert!(locations[0]
            .within
            .as_deref()
            .is_some_and(|w| w.starts_with("category:")));
        assert!(locations[0].scope_note.contains("not an addition"));
    }

    // -- duplicates ----------------------------------------------------------

    #[test]
    fn a_verified_duplicate_set_becomes_a_lead() {
        let mut scan = scan_from(vec![file(
            "C:\\Temp\\copy.bin",
            2 * GIB,
            2 * GIB,
            StorageClass::UserTemp,
            SafetyTier::SafeToReview,
        )]);
        scan.duplicates = vec![DuplicateSet {
            id: "abc:123:2".to_string(),
            size_logical: 2 * GIB,
            size_allocated: 2 * GIB,
            sha256: "0123456789abcdef0123456789abcdef".to_string(),
            copies: vec![
                DuplicateCopy {
                    path: std::path::PathBuf::from("C:\\Temp\\copy.bin"),
                    size_logical: 2 * GIB,
                    size_allocated: 2 * GIB,
                    modified_unix: None,
                    hard_link_count: Some(1),
                    tier: SafetyTier::SafeToReview,
                    class: StorageClass::UserTemp,
                    is_kept_candidate: false,
                    reclaim_is_certain: true,
                    reasons: vec![],
                },
                DuplicateCopy {
                    path: std::path::PathBuf::from("D:\\Backup\\copy.bin"),
                    size_logical: 2 * GIB,
                    size_allocated: 2 * GIB,
                    modified_unix: None,
                    hard_link_count: Some(1),
                    tier: SafetyTier::SafeToReview,
                    class: StorageClass::UserTemp,
                    is_kept_candidate: true,
                    reclaim_is_certain: true,
                    reasons: vec![],
                },
            ],
            reclaimable_bytes: 2 * GIB,
            bulk_reclaimable_bytes: 2 * GIB,
            uncertain_copies: 0,
            protected_copies: 0,
        }];
        let investigation = investigate(&scan);
        let lead = investigation
            .leads
            .iter()
            .find(|l| l.kind == LeadKind::DuplicateSet)
            .expect("a duplicate lead");
        assert_eq!(lead.bytes_allocated, 2 * GIB);
        assert!(lead
            .evidence
            .iter()
            .any(|e| e.contains("full SHA-256 read")));
        assert!(lead.origin.evidence.contains("does not record which application"));
    }

    // -- dormant applications ------------------------------------------------

    #[test]
    fn dormant_application_data_is_never_offered_for_removal() {
        let mut scan = scan_from(vec![]);
        scan.abandoned = vec![super::super::types::AbandonedAppRecord {
            path: std::path::PathBuf::from("C:\\Users\\tester\\AppData\\Roaming\\OldApp"),
            app_name: "Old App".to_string(),
            size_logical: 2 * GIB,
            size_allocated: 2 * GIB,
            file_count: 400,
            last_modified_unix: Some(1_500_000_000),
            days_since_change: Some(400),
            class: StorageClass::AbandonedAppData,
            tier: SafetyTier::Unknown,
            reasons: vec![],
            caveat: "Age is a weak signal.".to_string(),
        }];
        let investigation = investigate(&scan);
        let lead = investigation
            .leads
            .iter()
            .find(|l| l.kind == LeadKind::DormantApp)
            .expect("a dormant lead");
        assert_eq!(lead.action_kind, LeadActionKind::NotOffered);
        assert_eq!(lead.risk, LeadRisk::Unknown);
    }

    // -- significance in context --------------------------------------------

    #[test]
    fn a_dominant_category_is_flagged_and_promoted() {
        let scan = scan_from(vec![
            file("C:\\Temp\\a.tmp", 9 * GIB, 9 * GIB, StorageClass::UserTemp, SafetyTier::SafeToReview),
            file("D:\\Build\\x.bin", GIB, GIB, StorageClass::DevCache, SafetyTier::ReviewCarefully),
        ]);
        let investigation = investigate(&scan);
        assert_eq!(investigation.map.measured_bytes, 10 * GIB);
        // 9 GB of 10 GB is a concentration; 1 GB of 10 GB is also a share of
        // 10%, which clears the notable threshold but not the major one.
        assert_eq!(investigation.notable_leads.len(), 2);
        let major = investigation.major_lead.as_deref().expect("a major lead");
        assert_eq!(major, "category:user_temp");
        let lead = investigation
            .leads
            .iter()
            .find(|l| l.id == major)
            .expect("the major lead is in the list");
        assert_eq!(lead.significance, Significance::Major);
        let minor = investigation
            .leads
            .iter()
            .find(|l| l.id == "category:dev_cache")
            .expect("the minor lead is in the list");
        assert_eq!(minor.significance, Significance::Notable);
        assert_ne!(
            minor.significance, lead.significance,
            "the two leads are ranked differently because their sizes differ"
        );
    }

    #[test]
    fn the_lead_count_and_the_notable_count_are_reported_honestly() {
        let scan = scan_from(vec![file(
            "C:\\Temp\\a.tmp",
            100 * MIB,
            100 * MIB,
            StorageClass::UserTemp,
            SafetyTier::SafeToReview,
        )]);
        let investigation = investigate(&scan);
        assert_eq!(investigation.headline.lead_count, investigation.leads.len());
        assert_eq!(
            investigation.headline.notable_count,
            investigation.notable_leads.len()
        );
    }

    #[test]
    fn a_cancelled_scan_says_so() {
        let mut scan = scan_from(vec![file(
            "C:\\Temp\\a.tmp",
            GIB,
            GIB,
            StorageClass::UserTemp,
            SafetyTier::SafeToReview,
        )]);
        scan.coverage = ScanCoverage {
            files_seen: 1,
            cancelled: true,
            ..Default::default()
        };
        let investigation = investigate(&scan);
        assert_eq!(investigation.map.certainty, Certainty::Partial);
        assert!(investigation.headline.completeness.contains("cancelled"));
    }

    #[test]
    fn permission_failures_lower_the_certainty() {
        let mut scan = scan_from(vec![file(
            "C:\\Temp\\a.tmp",
            GIB,
            GIB,
            StorageClass::UserTemp,
            SafetyTier::SafeToReview,
        )]);
        scan.coverage = ScanCoverage {
            files_seen: 1,
            permission_denied_dirs: 2,
            ..Default::default()
        };
        let investigation = investigate(&scan);
        assert_eq!(investigation.map.certainty, Certainty::LowerBound);
        assert_eq!(investigation.evidence.permission_denied, 2);
    }

    #[test]
    fn the_class_summary_the_engine_produced_is_the_one_that_is_used() {
        // The investigation must read the engine's own aggregation rather
        // than making a second one.
        let files = vec![file(
            "C:\\Temp\\a.tmp",
            GIB,
            GIB,
            StorageClass::UserTemp,
            SafetyTier::SafeToReview,
        )];
        let scan = scan_from(files);
        let expected: Vec<ClassSummary> = scan.summaries.clone();
        let investigation = investigate(&scan);
        let engine_total: u64 = expected.iter().map(|s| s.bytes_allocated).sum();
        assert_eq!(investigation.map.measured_bytes, engine_total);
    }
}
