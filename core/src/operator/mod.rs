//! C.U.R.E Operator: Real Action-Oriented Security & Recovery Engine.
//!
//! Replaces decorative companions with a functional, deterministically auditable
//! operator that provides:
//! - Evidence-first system posture & recommendations
//! - Risk assessments and action approvals
//! - Non-destructive storage rescue & verified cross-volume relocation
//! - Active threat response and process tree containment
//! - Takeover defense, PID race prevention, and audit journaling

pub mod defense;
pub mod journal;
pub mod response;

pub use defense::{
    is_critical_system_process, sanitize_untrusted_data, PathBoundaryGuard, PidVerificationResult,
    ProcessVerificationAnchor, CRITICAL_SYSTEM_PROCESSES,
};
pub use journal::{
    audit_log, get_audit_log, init_global_journal, verify_global_journal_integrity,
    OperatorJournalEntry,
};
pub use response::{
    contain_file_with_defender, find_defender_cli, get_process_descendants,
    scan_file_with_defender, terminate_process_guarded, DefenderScanResult,
    ProcessContainmentReceipt, ResponseLevel, ThreatCategory, ThreatFinding,
};

use serde::{Deserialize, Serialize};

/// High-level actionable recommendation surfaced by the C.U.R.E Operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorRecommendation {
    pub id: String,
    pub level: ResponseLevel,
    pub title: String,
    pub description: String,
    pub risk_assessment: String,
    pub action_type: String,
    pub action_label: String,
    pub target: String,
    pub required_confirmation_token: Option<String>,
}

/// Overall system posture evaluated by the C.U.R.E Operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorPosture {
    pub timestamp_unix: u64,
    pub highest_threat_level: ResponseLevel,
    pub c_drive_congested: bool,
    pub c_drive_pressure_pct: f64,
    pub c_drive_free_bytes: u64,
    pub recommended_storage_relief_bytes: u64,
    pub active_agent_count: usize,
    pub unverified_agent_count: usize,
    pub threat_findings: Vec<ThreatFinding>,
    pub recommendations: Vec<OperatorRecommendation>,
}
