//! Security report export (JSON / human-readable TXT).
//!
//! An explicit, user-invoked snapshot: what was checked, what was found,
//! and what was NOT checked. The report never prints "clean" for an
//! unperformed check — every coverage area is labeled Checked, Not checked,
//! or Unavailable with the reason.
//!
//! Cost note: assembling a report re-verifies signatures for listed
//! findings and hashes HighRisk binaries. That is deliberate (explicit
//! export action) and bounded to findings, never the whole disk.

use serde::{Deserialize, Serialize};

use crate::hash_intel::ThreatIntelProvider;
use crate::model::{RiskLevel, ScoredEntry};
use crate::process_scan::{ProcessInfo, ProcessScore};
use crate::ransom_detect::RansomFinding;

pub const CURE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Filesystem-safe UTC stamp for report filenames (`20260916-123456`).
pub fn utc_stamp() -> String {
    chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoverageState {
    Checked,
    /// Enumerated with skips (e.g. unreadable task files).
    Partial {
        skipped: usize,
    },
    NotChecked,
    Unavailable,
    CheckFailed,
    AccessDenied,
}

impl CoverageState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Checked => "checked",
            Self::Partial { .. } => "partial",
            Self::NotChecked => "not checked",
            Self::Unavailable => "unavailable",
            Self::CheckFailed => "check failed",
            Self::AccessDenied => "access denied",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageRow {
    pub area: String,
    pub state: CoverageState,
    pub detail: String,
}

impl CoverageRow {
    pub fn checked(area: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            area: area.into(),
            state: CoverageState::Checked,
            detail: detail.into(),
        }
    }
    pub fn not_checked(area: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            area: area.into(),
            state: CoverageState::NotChecked,
            detail: reason.into(),
        }
    }
    pub fn unavailable(area: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            area: area.into(),
            state: CoverageState::Unavailable,
            detail: reason.into(),
        }
    }
}

/// Coverage row from a scanner access state. Non-OK states surface the
/// scanner's own reason so a failed check never renders as a clean zero.
/// Shared by the CLI report, GUI export, and scan summary.
pub fn source_state_row(
    area: &str,
    status: &crate::elevation::SourceStatus,
    ok_detail: String,
) -> CoverageRow {
    use crate::elevation::SourceStatus;
    let state = match status {
        SourceStatus::Available => CoverageState::Checked,
        SourceStatus::Partial { skipped, .. } => CoverageState::Partial { skipped: *skipped },
        SourceStatus::Unavailable { .. } => CoverageState::Unavailable,
        SourceStatus::CheckFailed { .. } => CoverageState::CheckFailed,
        SourceStatus::AccessDenied { .. } => CoverageState::AccessDenied,
    };
    let detail = match status {
        SourceStatus::Available => ok_detail,
        SourceStatus::Partial { reason, .. } => format!("{ok_detail} ({reason})"),
        SourceStatus::Unavailable { reason }
        | SourceStatus::CheckFailed { reason }
        | SourceStatus::AccessDenied { reason } => reason.clone(),
    };
    CoverageRow {
        area: area.to_string(),
        state,
        detail,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingReport {
    pub kind: String,
    pub name: String,
    pub source: String,
    pub command: String,
    pub location: String,
    pub score: i32,
    pub risk: String,
    pub reasons: Vec<String>,
    pub attack_id: String,
    pub attack_name: String,
    /// VALID / INVALID / UNSIGNED / UNKNOWN, or NOT ASSESSED.
    pub signature: String,
    pub publisher: Option<String>,
    /// SHA-256 hex — computed for HighRisk findings with a resolvable
    /// binary only (hashing everything per export would be pure cost).
    pub sha256: Option<String>,
    /// Static guidance for the risk level. Never an automatic verdict.
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub generated_at: String,
    pub cure_version: String,
    pub windows_version: String,
    pub coverage: Vec<CoverageRow>,
    pub findings: Vec<FindingReport>,
    pub safe_persistence: usize,
    pub safe_processes: usize,
    pub intel_provider: String,
    pub redacted: bool,
    /// Present only in incident exports (`cure incident` / incident view).
    pub incident: Option<IncidentReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentCorrelationRow {
    pub pid: u32,
    pub process: String,
    pub finding: String,
    pub source: String,
    pub level: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentTimelineRow {
    pub t_ms: u64,
    pub wall_time: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentReport {
    pub investigation_id: String,
    pub started_at: String,
    pub duration_secs: u64,
    pub elevated: bool,
    pub verdict: String,
    pub verdict_explanation: String,
    pub process_observation: String,
    pub window_observation: String,
    pub process_count: usize,
    pub window_count: usize,
    pub transient_count: usize,
    pub correlations: Vec<IncidentCorrelationRow>,
    pub timeline: Vec<IncidentTimelineRow>,
}

/// Build the exportable incident section from an observation result,
/// applying redaction to paths, titles, and command lines when asked.
/// Redaction is about machine identity — correlation structure stays.
pub fn incident_section(
    result: &crate::incident::ObservationResult,
    redact: bool,
) -> IncidentReport {
    incident_section_with_identity(
        result,
        redact,
        home_dir().as_deref(),
        &std::env::var("USERNAME").unwrap_or_default(),
    )
}

/// Same as [`incident_section`] with an explicit identity (tests use a
/// synthetic one; production passes the process environment).
pub fn incident_section_with_identity(
    result: &crate::incident::ObservationResult,
    redact: bool,
    home: Option<&std::path::Path>,
    username: &str,
) -> IncidentReport {
    let redact_it = |s: &str| {
        if redact {
            redact_text(s, home, username)
        } else {
            s.to_string()
        }
    };
    IncidentReport {
        investigation_id: result.investigation_id.clone(),
        started_at: result.started_at.clone(),
        duration_secs: result.duration_secs,
        elevated: result.elevated,
        verdict: result.verdict.label().to_string(),
        verdict_explanation: result.verdict.explanation().to_string(),
        process_observation: format!("{:?}", result.process_observation),
        window_observation: format!("{:?}", result.window_observation),
        process_count: result.processes.len(),
        window_count: result.windows.len(),
        transient_count: result.windows.iter().filter(|w| w.is_transient()).count(),
        correlations: result
            .correlations
            .iter()
            .map(|c| IncidentCorrelationRow {
                pid: c.process_pid,
                process: redact_it(&c.process_name),
                finding: redact_it(&c.finding_name),
                source: c.finding_source.clone(),
                level: c.level.label().to_string(),
                evidence: c.evidence.iter().map(|e| redact_it(e)).collect(),
            })
            .collect(),
        timeline: result
            .timeline
            .iter()
            .map(|e| IncidentTimelineRow {
                t_ms: e.t_ms,
                wall_time: e.wall_time.clone(),
                kind: format!("{:?}", e.kind),
                text: redact_it(&e.text),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReportOptions {
    pub redact: bool,
}

/// Everything the report renders. Callers run the scans; this module only
/// assembles, enriches (bounded), and renders.
pub struct ScanInput {
    pub persistence: Vec<ScoredEntry>,
    pub processes: Vec<(ProcessInfo, ProcessScore)>,
    pub ransom: Vec<RansomFinding>,
    pub canary_note: String,
}

pub fn recommended_action_persistence(scored: &ScoredEntry) -> String {
    match scored.risk {
        RiskLevel::HighRisk if scored.entry.source.is_file_backed() => {
            format!("Quarantine for review (`cure quarantine {}`), then investigate before any deletion.", scored.entry.id)
        }
        RiskLevel::HighRisk => {
            "Investigate before disabling. Back up the location first; removal is manual and must be reversible.".to_string()
        }
        RiskLevel::Suspicious => "Investigate before disabling.".to_string(),
        RiskLevel::Safe => "No action needed.".to_string(),
    }
}

pub fn assemble(input: ScanInput, coverage: Vec<CoverageRow>, opts: &ReportOptions) -> Report {
    assemble_with_identity(
        input,
        coverage,
        opts,
        home_dir().as_deref(),
        &std::env::var("USERNAME").unwrap_or_default(),
    )
}

/// Same as [`assemble`] with an explicit identity (tests use a synthetic
/// one; production passes the process environment).
pub fn assemble_with_identity(
    input: ScanInput,
    coverage: Vec<CoverageRow>,
    opts: &ReportOptions,
    home: Option<&std::path::Path>,
    username: &str,
) -> Report {
    let redact = |s: &str| {
        if opts.redact {
            redact_text(s, home, username)
        } else {
            s.to_string()
        }
    };

    let mut findings = Vec::new();
    let mut safe_persistence = 0usize;
    for scored in &input.persistence {
        if scored.risk == RiskLevel::Safe {
            safe_persistence += 1;
            continue;
        }
        let exe = crate::signature::resolve_executable_path(&scored.entry.command);
        // Publisher for every listed finding (small lists only); SHA-256
        // additionally for HighRisk with a resolvable binary.
        let (signature, publisher) = match exe.as_deref() {
            Some(path) => {
                let detail = crate::signature::signature_detail(path);
                (detail.status.label().to_string(), detail.publisher)
            }
            None => ("UNKNOWN".to_string(), None),
        };
        let sha256 = if scored.risk == RiskLevel::HighRisk {
            exe.as_deref().and_then(crate::hash_intel::sha256_file_hex)
        } else {
            None
        };
        findings.push(FindingReport {
            kind: "persistence".to_string(),
            name: redact(&scored.entry.name),
            source: scored.entry.source.to_string(),
            command: redact(&scored.entry.command),
            location: redact(&scored.entry.location),
            score: scored.score,
            risk: format!("{:?}", scored.risk),
            reasons: scored.reasons.clone(),
            attack_id: scored.attack.id.clone(),
            attack_name: scored.attack.name.clone(),
            signature,
            publisher: publisher.map(|p| redact(&p)),
            sha256,
            recommended_action: recommended_action_persistence(scored),
        });
    }

    let mut safe_processes = 0usize;
    for (info, ps) in &input.processes {
        if ps.risk == RiskLevel::Safe {
            safe_processes += 1;
            continue;
        }
        findings.push(FindingReport {
            kind: "process".to_string(),
            name: redact(&info.name),
            source: format!("pid {}", info.pid),
            command: redact(&info.exe_path),
            location: redact(&info.exe_path),
            score: ps.score,
            risk: format!("{:?}", ps.risk),
            reasons: ps.reasons.clone(),
            attack_id: String::new(),
            attack_name: String::new(),
            signature: "NOT ASSESSED".to_string(),
            publisher: None,
            sha256: None,
            recommended_action: "Confirm the process is unwanted, then terminate from Process Sentinel (the PID is re-validated before termination).".to_string(),
        });
    }

    for finding in &input.ransom {
        let (kind, path, detail) = match finding {
            RansomFinding::Note(note) => (
                "ransom-note",
                note.path.to_string_lossy().to_string(),
                format!("Matched pattern: {}", note.matched_stem),
            ),
            RansomFinding::BulkEncryption(cluster) => (
                "bulk-encryption",
                cluster.folder.to_string_lossy().to_string(),
                format!(
                    "{} files with unusual extension \".{}\" (avg age {} days)",
                    cluster.file_count, cluster.extension, cluster.avg_age_days
                ),
            ),
        };
        findings.push(FindingReport {
            kind: kind.to_string(),
            name: redact(&path),
            source: "ransom-indicators".to_string(),
            command: String::new(),
            location: redact(&path),
            score: 100,
            risk: "HighRisk".to_string(),
            reasons: vec![redact(&detail)],
            attack_id: "T1486".to_string(),
            attack_name: "Data Encrypted for Impact".to_string(),
            signature: "NOT ASSESSED".to_string(),
            publisher: None,
            sha256: None,
            recommended_action: "Isolate the machine from the network; check nomoreransom.org for a known decryptor. C.U.R.E cannot decrypt files.".to_string(),
        });
    }

    Report {
        generated_at: chrono::Utc::now().to_rfc3339(),
        cure_version: CURE_VERSION.to_string(),
        windows_version: windows_version(),
        coverage,
        findings,
        safe_persistence,
        safe_processes,
        intel_provider: crate::hash_intel::fixture_provider()
            .provider_label()
            .to_string(),
        redacted: opts.redact,
        incident: None,
    }
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

/// Redact a home directory and username from free text (case-insensitive,
/// longest match first). Display-only: scan logic never sees redacted text.
pub fn redact_text(text: &str, home: Option<&std::path::Path>, username: &str) -> String {
    let mut out = text.to_string();
    let mut needles: Vec<(String, &str)> = Vec::new();
    if let Some(home) = home {
        let h = home.to_string_lossy().into_owned();
        if !h.is_empty() {
            needles.push((h, "%USERPROFILE%"));
        }
    }
    if !username.is_empty() {
        needles.push((username.to_string(), "<user>"));
    }
    needles.sort_by_key(|n| std::cmp::Reverse(n.0.len()));
    for (needle, replacement) in needles {
        out = replace_case_insensitive(&out, &needle, replacement);
    }
    out
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let lower_hay = haystack.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut rest = 0usize;
    while let Some(pos) = lower_hay[rest..].find(&lower_needle) {
        let start = rest + pos;
        out.push_str(&haystack[rest..start]);
        out.push_str(replacement);
        rest = start + needle.len();
    }
    out.push_str(&haystack[rest..]);
    out
}

/// Best-effort Windows version string. Registry is read-only here;
/// anything unreadable degrades to a generic label, never an error.
pub fn windows_version() -> String {
    #[cfg(windows)]
    {
        let key_path = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        if let Ok(key) =
            winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).open_subkey(key_path)
        {
            let product: String = key.get_value("ProductName").unwrap_or_default();
            let display: String = key.get_value("DisplayVersion").unwrap_or_default();
            let build: String = key.get_value("CurrentBuild").unwrap_or_default();
            let mut parts = Vec::new();
            if !product.is_empty() {
                parts.push(product);
            }
            if !display.is_empty() {
                parts.push(display);
            }
            if !build.is_empty() {
                parts.push(format!("build {build}"));
            }
            if !parts.is_empty() {
                return parts.join(" ");
            }
        }
        "Windows (version unavailable)".to_string()
    }
    #[cfg(not(windows))]
    {
        std::env::consts::OS.to_string()
    }
}

pub fn render_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string())
}

pub fn render_txt(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("C.U.R.E. SECURITY REPORT\n");
    out.push_str(&format!("generated : {}\n", report.generated_at));
    out.push_str(&format!("cure      : {}\n", report.cure_version));
    out.push_str(&format!("platform  : {}\n", report.windows_version));
    out.push_str(&format!(
        "redacted  : {}\n",
        if report.redacted {
            "yes (paths/user)"
        } else {
            "no"
        }
    ));
    out.push_str(&format!("intel     : {}\n", report.intel_provider));
    out.push_str("\nCOVERAGE\n");
    for row in &report.coverage {
        let mut line = format!(
            "  [{}] {} — {}",
            row.state.label().to_uppercase(),
            row.area,
            row.detail
        );
        if let CoverageState::Partial { skipped } = row.state {
            line.push_str(&format!(" ({skipped} skipped)"));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&format!(
        "\nFINDINGS ({}, {} safe persistence + {} safe processes not listed)\n",
        report.findings.len(),
        report.safe_persistence,
        report.safe_processes
    ));
    if report.findings.is_empty() {
        out.push_str("  none — every performed check came back without findings.\n");
        out.push_str("  (Areas marked not checked / unavailable above were NOT assessed.)\n");
    }
    for (i, f) in report.findings.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. [{}] {} ({})\n",
            i + 1,
            f.risk,
            f.name,
            f.kind
        ));
        out.push_str(&format!("   source   : {}\n", f.source));
        if !f.command.is_empty() {
            out.push_str(&format!("   command  : {}\n", f.command));
        }
        if !f.location.is_empty() && f.location != f.command {
            out.push_str(&format!("   location : {}\n", f.location));
        }
        out.push_str(&format!("   score    : {}\n", f.score));
        if !f.attack_id.is_empty() {
            out.push_str(&format!(
                "   att&ck   : {} ({})\n",
                f.attack_id, f.attack_name
            ));
        }
        out.push_str(&format!("   signature: {}", f.signature));
        if let Some(publisher) = &f.publisher {
            out.push_str(&format!(" ({publisher})"));
        }
        out.push('\n');
        if let Some(sha256) = &f.sha256 {
            out.push_str(&format!("   sha256   : {sha256}\n"));
        }
        for reason in &f.reasons {
            out.push_str(&format!("   evidence : {reason}\n"));
        }
        out.push_str(&format!("   action   : {}\n", f.recommended_action));
    }
    out.push_str("\nLIMITATIONS\n");
    out.push_str("  Threat intel is demo fixture data, not a live feed.\n");
    out.push_str("  Canary Guard is an experimental tripwire, not ransomware protection.\n");
    out.push_str(
        "  Registry, services, WMI, IFEO, AppInit, and COM findings are never auto-remediated.\n",
    );
    if let Some(incident) = &report.incident {
        out.push_str("\nINCIDENT INVESTIGATION\n");
        out.push_str(&format!("  id         : {}\n", incident.investigation_id));
        out.push_str(&format!("  started    : {}\n", incident.started_at));
        out.push_str(&format!("  observed   : {} s\n", incident.duration_secs));
        out.push_str(&format!(
            "  elevated   : {}\n",
            if incident.elevated { "yes" } else { "no" }
        ));
        out.push_str(&format!("  verdict    : {}\n", incident.verdict));
        out.push_str(&format!(
            "  why        : {}\n",
            incident.verdict_explanation
        ));
        out.push_str(&format!(
            "  processes  : {} observed ({})\n",
            incident.process_count, incident.process_observation
        ));
        out.push_str(&format!(
            "  windows    : {} observed, {} transient ({})\n",
            incident.window_count, incident.transient_count, incident.window_observation
        ));
        out.push_str("\n  CORRELATIONS\n");
        if incident.correlations.is_empty() {
            out.push_str("    none — no observed process relates to a startup finding.\n");
        }
        for c in &incident.correlations {
            out.push_str(&format!(
                "    [{}] {} (pid {}) ↔ {} ({})\n",
                c.level, c.process, c.pid, c.finding, c.source
            ));
            for e in &c.evidence {
                out.push_str(&format!("      · {e}\n"));
            }
        }
        out.push_str("\n  TIMELINE\n");
        for e in &incident.timeline {
            out.push_str(&format!("    {} {}\n", e.wall_time, e.text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_replaces_home_and_user_case_insensitively() {
        let home = std::path::Path::new(r"C:\Users\Bob");
        let text = r"payload at C:\USERS\bob\AppData\x.exe by BOB";
        let redacted = redact_text(text, Some(home), "bob");
        assert!(!redacted.to_ascii_lowercase().contains("bob"));
        assert!(redacted.contains("%USERPROFILE%"));
        assert!(redacted.contains("<user>") || !redacted.contains("BOB"));
    }

    #[test]
    fn redaction_without_home_only_masks_user() {
        let redacted = redact_text(r"C:\Windows\System32\svchost.exe", None, "bob");
        assert_eq!(redacted, r"C:\Windows\System32\svchost.exe");
    }

    /// Grep-style regression: a redacted export saturated with fake
    /// identity in EVERY finding field must leak nothing (case-insensitive)
    /// in either format, while staying forensic (paths present, masked).
    /// Uses the real assemble path with a synthetic identity — the same
    /// wiring production uses with the process environment.
    #[test]
    fn redacted_export_leaks_no_identity_in_any_format() {
        use crate::model::{PersistenceEntry, PersistenceSource};
        let user = "CURE-SYNTH-USER";
        let home = std::path::Path::new(r"C:\Users\CURE-SYNTH-USER");
        let entry = PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            format!("{user}-updater.lnk"),
            format!(
                r"{}\AppData\Local\Foo\updater.exe --user={}",
                home.display(),
                user
            ),
            format!(r"{}\AppData\Local\Foo\updater.exe", home.display()),
        );
        let scored = crate::risk::score_with_signals(
            &entry,
            crate::signature::SignatureStatus::Unsigned,
            None,
        );
        let input = ScanInput {
            persistence: vec![scored],
            processes: vec![],
            ransom: vec![],
            canary_note: String::new(),
        };
        let report = assemble_with_identity(
            input,
            vec![CoverageRow::checked("Startup folder", "1 entry")],
            &ReportOptions { redact: true },
            Some(home),
            user,
        );
        assert!(report.redacted);
        for (label, body) in [("txt", render_txt(&report)), ("json", render_json(&report))] {
            // Baseline sanity: the masked marker must be present, or the
            // test proves nothing.
            assert!(body.contains("%USERPROFILE%"), "{label} lost the path");
            assert!(
                !body
                    .to_ascii_lowercase()
                    .contains(&user.to_ascii_lowercase()),
                "{label} export leaks identity"
            );
            // Reasons carry evidence text — also covered.
            assert!(!body.contains("CURE-SYNTH-USER"));
        }
    }

    #[test]
    fn empty_report_renders_without_clean_claim() {
        let report = Report {
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            cure_version: "0.1.0".to_string(),
            windows_version: "test".to_string(),
            coverage: vec![
                CoverageRow::checked("Startup", "3 areas"),
                CoverageRow::not_checked("Canary", "session feature"),
                CoverageRow::unavailable("Registry", "non-Windows"),
            ],
            findings: vec![],
            safe_persistence: 5,
            safe_processes: 100,
            intel_provider: "DEMO".to_string(),
            redacted: false,
            incident: None,
        };
        let txt = render_txt(&report);
        assert!(txt.contains("[CHECKED] Startup"));
        assert!(txt.contains("[NOT CHECKED] Canary"));
        assert!(txt.contains("[UNAVAILABLE] Registry"));
        assert!(!txt.to_ascii_lowercase().contains("system is clean"));
        let json = render_json(&report);
        assert!(json.contains("\"safe_persistence\": 5"));
    }

    #[test]
    fn assemble_splits_findings_and_safe_counts() {
        let safe = ScoredEntry {
            entry: crate::model::PersistenceEntry::new(
                crate::model::PersistenceSource::StartupFolder,
                "CURE-SYNTH-ok",
                r"C:\Windows\System32\CURE-SYNTH-ok.exe",
                r"C:\Windows\System32\CURE-SYNTH-ok.exe",
            ),
            score: 0,
            risk: RiskLevel::Safe,
            reasons: vec![],
            attack: crate::risk::attack_info_for(&crate::model::PersistenceSource::StartupFolder),
        };
        let input = ScanInput {
            persistence: vec![safe],
            processes: vec![],
            ransom: vec![],
            canary_note: String::new(),
        };
        let report = assemble(
            input,
            vec![CoverageRow::checked("Startup", "ok")],
            &ReportOptions { redact: false },
        );
        assert!(report.findings.is_empty());
        assert_eq!(report.safe_persistence, 1);
    }
}
