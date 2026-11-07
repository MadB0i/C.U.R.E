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

impl AgentCaseFile {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# C.U.R.E. Agent Case File\n\n");
        md.push_str(&format!("**Case ID:** `{}`\n", self.case_id));
        md.push_str(&format!("**Tool / Agent:** {}\n", self.agent_label));
        md.push_str(&format!(
            "**Observation Duration:** {}\n",
            self.observation_label
        ));
        md.push_str(&format!("**Confidence:** {}\n", self.confidence_label));
        md.push_str(&format!("**Completeness:** {}\n\n", self.completeness));

        md.push_str("## Observed Activity Summary\n\n");
        md.push_str(&format!(
            "- Total Changes Observed: {}\n",
            self.changes_total
        ));
        md.push_str(&format!("  - Created: {}\n", self.created));
        md.push_str(&format!("  - Modified: {}\n", self.modified));
        md.push_str(&format!("  - Removed: {}\n", self.removed));
        md.push_str(&format!("  - Renamed: {}\n", self.renamed));
        md.push_str(&format!(
            "- Net Storage Delta: {}\n\n",
            self.storage_delta_label
        ));

        md.push_str("## Windows Persistence Tracking\n\n");
        md.push_str(&format!(
            "- Persistence Entries Added: {}\n",
            self.windows_added
        ));
        md.push_str(&format!(
            "- Persistence Entries Removed: {}\n",
            self.windows_removed
        ));
        md.push_str(&format!(
            "- Persistence Entries Changed: {}\n",
            self.windows_changed
        ));
        md.push_str(&format!("- Note: {}\n\n", self.windows_note));

        md.push_str("## Evidence & Limitations\n\n");
        md.push_str(&format!("> {}\n\n", self.confidence_note));
        md.push_str(&format!("> {}\n\n", self.coverage_note));

        if let Some(summary) = &self.summary {
            md.push_str("## User Notes (Sanitized)\n\n");
            md.push_str(summary);
            md.push_str("\n\n");
        }

        md.push_str("---\n*Generated locally by C.U.R.E. Portable Windows Security Toolkit (Privacy Preserved)*\n");
        md
    }

    pub fn to_html(&self) -> String {
        let summary_block = if let Some(summary) = &self.summary {
            format!(
                r#"
  <h2>Operator Notes (Sanitized)</h2>
  <div class="note">{}</div>"#,
                html_escape(summary)
            )
        } else {
            String::new()
        };

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>C.U.R.E. Case File {case_id}</title>
<style>
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #0f141c; color: #e1e7f0; padding: 2rem; max-width: 780px; margin: 0 auto; line-height: 1.6; }}
  h1 {{ color: #4ade80; border-bottom: 1px solid #1e293b; padding-bottom: 0.5rem; }}
  h2 {{ color: #38bdf8; margin-top: 1.5rem; font-size: 1.25rem; }}
  .badge {{ display: inline-block; padding: 0.25rem 0.6rem; border-radius: 4px; background: #1e293b; color: #94a3b8; font-size: 0.85rem; font-weight: 600; margin-right: 0.5rem; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 1rem; margin: 1rem 0; }}
  .card {{ background: #172033; padding: 1rem; border-radius: 6px; border: 1px solid #1e293b; }}
  .stat {{ font-size: 1.75rem; font-weight: 700; color: #f8fafc; }}
  .note {{ background: #1e293b; padding: 0.75rem 1rem; border-left: 4px solid #38bdf8; border-radius: 4px; font-size: 0.9rem; color: #cbd5e1; margin: 1rem 0; }}
  .footer {{ margin-top: 2rem; border-top: 1px solid #1e293b; padding-top: 1rem; font-size: 0.8rem; color: #64748b; }}
</style>
</head>
<body>
  <h1>C.U.R.E. Agent Case File</h1>
  <div>
    <span class="badge">CASE ID: {case_id}</span>
    <span class="badge">TOOL: {agent_label}</span>
    <span class="badge">{observation_label}</span>
  </div>

  <h2>Observed Workspace Activity</h2>
  <div class="grid">
    <div class="card"><div>Total Changes</div><div class="stat">{changes_total}</div></div>
    <div class="card"><div>Created</div><div class="stat">{created}</div></div>
    <div class="card"><div>Modified</div><div class="stat">{modified}</div></div>
    <div class="card"><div>Storage Delta</div><div class="stat">{storage_delta_label}</div></div>
  </div>

  <h2>Windows Persistence Status</h2>
  <div class="note">
    Added: {windows_added} | Removed: {windows_removed} | Changed: {windows_changed}<br>
    {windows_note}
  </div>

  <h2>Evidence &amp; Limitations</h2>
  <div class="note">{confidence_note}</div>
  <div class="note">{coverage_note}</div>{summary_block}

  <div class="footer">
    Generated locally by C.U.R.E. (Clean, Undoable, Resilient, Evidence-first) · No telemetry · No cloud API
  </div>
</body>
</html>"#,
            case_id = html_escape(&self.case_id),
            agent_label = html_escape(&self.agent_label),
            observation_label = html_escape(&self.observation_label),
            changes_total = self.changes_total,
            created = self.created,
            modified = self.modified,
            storage_delta_label = html_escape(&self.storage_delta_label),
            windows_added = self.windows_added,
            windows_removed = self.windows_removed,
            windows_changed = self.windows_changed,
            windows_note = html_escape(&self.windows_note),
            confidence_note = html_escape(&self.confidence_note),
            coverage_note = html_escape(&self.coverage_note),
            summary_block = summary_block,
        )
    }
}

/// Minimal, strict HTML entity escaping for user-facing case file exports.
pub fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn short_id(session: &FlightSession) -> String {
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
        "Filesystem events were observed while a supported agent process was running in the background. The directory watcher records filesystem notifications, not process write handles. Time correlation does not establish causation or agent attribution."
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
        let json = case.to_json().unwrap();
        assert!(!json.contains("janedoe"));
        assert!(!json.contains("private-repo"));
        assert!(!json.contains("secret-module"));
        assert!(!json.contains(r"C:\\"));
        assert!(!json.contains("workspace"));
        assert!(!json.contains("prompt"));
        assert!(!json.contains("transcript"));
    }

    #[test]
    fn exports_markdown_and_html_correctly() {
        let case = build(&session_with_event(), None, None, false, false);
        let md = case.to_markdown();
        assert!(md.contains("# C.U.R.E. Agent Case File"));
        assert!(md.contains("OpenCode"));
        assert!(md.contains("Correlated, not attributed"));

        let html = case.to_html();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("OpenCode"));
        assert!(html.contains("C.U.R.E."));
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

    #[test]
    fn html_escaping_neutralizes_xss_in_notes_and_metadata() {
        let mut case = build(&session_with_event(), None, None, false, false);
        case.agent_label = "<script>alert('pwn')</script>".to_string();
        case.summary = Some("<b>bold note & comment</b>".to_string());
        let html = case.to_html();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;alert(&#x27;pwn&#x27;)&lt;/script&gt;"));
        assert!(!html.contains("<b>"));
        assert!(html.contains("&lt;b&gt;bold note &amp; comment&lt;/b&gt;"));
    }
}
