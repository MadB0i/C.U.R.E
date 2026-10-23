//! The shareable Agent Case File.
//!
//! Like the storage case file, this is an allowlisted model: there is no
//! field for a workspace path, a file name, a user name, a machine name, a
//! repository name, an agent prompt or a transcript. The renderer receives
//! counts, fixed labels and a duration. Nothing else can leak because there
//! is nowhere for it to go.

use serde::{Deserialize, Serialize};

use super::detection::AgentKind;
use super::recorder::FlightSession;
use super::windows::WindowsChanges;
use crate::storage::casefile::sanitize_summary;
use crate::storage::human_bytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCaseFile {
    pub case_id: String,
    pub kind: String,
    pub title: String,
    pub generated_unix: u64,
    pub agent_label: String,
    pub agent_presence: String,
    pub observation_seconds: u64,
    pub observation_label: String,
    pub changes_total: u64,
    pub created: u64,
    pub modified: u64,
    pub removed: u64,
    pub renamed: u64,
    pub storage_delta_bytes: i64,
    pub storage_delta_label: String,
    pub storage_note: String,
    pub confidence_label: String,
    pub confidence_note: String,
    pub completeness: String,
    pub coverage_note: String,
    pub observed_events: u64,
    pub correlated_events: u64,
    pub unknown_events: u64,
    pub windows_added: usize,
    pub windows_removed: usize,
    pub windows_changed: usize,
    pub windows_note: String,
    pub reconciliation_used: bool,
    pub summary: Option<String>,
    pub summary_withheld: bool,
    pub demo: bool,
}

fn short_id(session: &FlightSession) -> String {
    // A local, non-reversible marker. It cannot be reversed into the session
    // id, the workspace or a path, and it is not derived from a user name.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in session
        .id
        .as_bytes()
        .iter()
        .chain(session.started_unix.to_le_bytes().iter())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash ^ (hash >> 32)) as u32)
}

pub fn duration_label(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn agent_label(agents: &[AgentKind]) -> String {
    match agents {
        [] => "No supported agent observed".to_string(),
        [one] => one.label().to_string(),
        many => many
            .iter()
            .map(|a| a.label())
            .collect::<Vec<_>>()
            .join(" + "),
    }
}

pub fn build(
    session: &FlightSession,
    windows: Option<&WindowsChanges>,
    summary: Option<&str>,
    include_summary: bool,
    demo: bool,
) -> AgentCaseFile {
    let counts = session.counts();
    let (summary_text, summary_withheld) = match (include_summary, summary) {
        (true, Some(text)) => match sanitize_summary(text) {
            Some(clean) => (Some(clean), false),
            None => (None, true),
        },
        _ => (None, false),
    };

    let confidence_label = if counts.unknown > 0 {
        "Incomplete observation".to_string()
    } else if counts.correlated > 0 {
        "Correlated, not attributed".to_string()
    } else if counts.total > 0 {
        "Observed".to_string()
    } else {
        "No changes observed".to_string()
    };
    let confidence_note = if counts.unknown > 0 {
        "Some events lack complete observation context. Do not treat them as a full record."
            .to_string()
    } else if counts.correlated > 0 {
        "Changes happened while a supported agent process was present. Presence does not establish causation."
            .to_string()
    } else if counts.total > 0 {
        "Changes were observed with no supported agent process present in the same sample."
            .to_string()
    } else {
        "The session recorded no supported filesystem changes.".to_string()
    };

    let net = counts.net_bytes();
    let storage_delta_label = if net == 0 {
        "No net size change".to_string()
    } else if net > 0 {
        format!("+{}", human_bytes(net as u64))
    } else {
        format!("−{}", human_bytes(net.unsigned_abs()))
    };

    let coverage_note = if session.coverage.complete {
        "Baseline and watcher coverage were complete within the recorder's documented limits."
            .to_string()
    } else if session.coverage.note.trim().is_empty() {
        "Observation coverage was incomplete. Missing changes cannot be inferred.".to_string()
    } else {
        session.coverage.note.clone()
    };

    let windows_note = match windows {
        None => "No Windows persistence comparison was included.".to_string(),
        Some(diff) if diff.is_empty() => {
            "No supported Windows persistence differences were observed.".to_string()
        }
        Some(_) => {
            "Windows persistence differences are review leads, not malware verdicts.".to_string()
        }
    };
    let reconciliation_used = session.coverage.snapshot_reconciled;

    AgentCaseFile {
        case_id: short_id(session),
        kind: "agent_session".to_string(),
        title: "AGENT SESSION CASE".to_string(),
        generated_unix: session.ended_unix.unwrap_or_else(super::recorder::now_unix),
        agent_label: agent_label(&session.agents_seen),
        agent_presence: if session.agents_seen.is_empty() {
            "No supported agent process was observed during the session.".to_string()
        } else {
            "A supported agent process was observed during the session.".to_string()
        },
        observation_seconds: session.duration_secs(),
        observation_label: duration_label(session.duration_secs()),
        changes_total: counts.total,
        created: counts.created,
        modified: counts.modified,
        removed: counts.removed,
        renamed: counts.renamed,
        storage_delta_bytes: net,
        storage_delta_label,
        storage_note: "Net metadata size change across observed events, not verified free space."
            .to_string(),
        confidence_label,
        confidence_note,
        completeness: if session.coverage.complete {
            "Complete within recorder limits".to_string()
        } else {
            "Incomplete coverage".to_string()
        },
        coverage_note,
        observed_events: counts.observed,
        correlated_events: counts.correlated,
        unknown_events: counts.unknown,
        windows_added: windows.map(|w| w.added.len()).unwrap_or(0),
        windows_removed: windows.map(|w| w.removed.len()).unwrap_or(0),
        windows_changed: windows.map(|w| w.changed.len()).unwrap_or(0),
        windows_note,
        reconciliation_used,
        summary: summary_text,
        summary_withheld,
        demo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::detection::AgentKind;
    use crate::agent::recorder::{
        ChangeKind, EvidenceConfidence, FlightEvent, FlightSession, ObservationSource,
        WorkspaceSnapshot,
    };

    fn baseline() -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            captured_unix: 1,
            files: vec![],
            truncated: false,
            unreadable_dirs: 0,
            skipped_links: 0,
            max_entries: 50_000,
        }
    }

    fn session_with_event() -> FlightSession {
        let mut session = FlightSession::new(
            "secret-session-id",
            r"C:\Users\janedoe\Projects\private-repo",
            &baseline(),
            vec![AgentKind::OpenCode],
            true,
        );
        session.record_event(FlightEvent {
            seq: 0,
            at_unix: session.started_unix + 10,
            kind: ChangeKind::Modified,
            path: "src/secret-module.rs".into(),
            from_path: None,
            size_before: Some(10),
            size_after: Some(30),
            source: ObservationSource::DirectoryWatcher,
            confidence: EvidenceConfidence::Correlated,
            agents: vec![AgentKind::OpenCode],
            note: "changed".into(),
        });
        session.finish();
        session
    }

    #[test]
    fn case_file_has_no_private_fields() {
        let case = build(&session_with_event(), None, None, false, false);
        let json = serde_json::to_string(&case).unwrap();
        assert!(!json.contains("janedoe"));
        assert!(!json.contains("private-repo"));
        assert!(!json.contains("secret-module"));
        assert!(!json.contains(r"C:\\"));
        assert!(!json.contains("workspace"));
        assert!(!json.contains("prompt"));
        assert!(!json.contains("transcript"));
    }

    #[test]
    fn counts_and_confidence_are_explicit() {
        let case = build(&session_with_event(), None, None, false, false);
        assert_eq!(case.changes_total, 1);
        assert_eq!(case.modified, 1);
        assert_eq!(case.agent_label, "OpenCode");
        assert_eq!(case.confidence_label, "Correlated, not attributed");
        assert!(case
            .confidence_note
            .contains("does not establish causation"));
        assert!(!case.confidence_label.contains("Attributed"));
    }

    #[test]
    fn a_private_note_is_withheld() {
        let case = build(
            &session_with_event(),
            None,
            Some(r"C:\Users\janedoe\secret"),
            true,
            false,
        );
        assert!(case.summary.is_none());
        assert!(case.summary_withheld);
    }

    #[test]
    fn duration_is_readable() {
        assert_eq!(duration_label(9), "9s");
        assert_eq!(duration_label(65), "1m 05s");
        assert_eq!(duration_label(3661), "1h 01m");
    }

    #[test]
    fn demo_is_always_marked() {
        let case = build(&session_with_event(), None, None, false, true);
        assert!(case.demo);
        assert_eq!(case.kind, "agent_session");
    }
}
