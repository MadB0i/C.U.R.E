//! First-run consent gate for the watcher's Startup-folder self-install.
//!
//! The decision logic is a pure function over the marker file's contents so
//! every branch is unit-testable without showing a real message box. The
//! marker lives next to cure-watch.log in %APPDATA% — deliberately NOT the
//! Startup folder itself, because a user deleting the installed copy must
//! not be misread as revoking (or granting) consent.

use std::path::PathBuf;

pub const CONSENT_FILE_NAME: &str = "cure-watch-consent.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDecision {
    /// No marker yet (or unreadable/garbled) -> show the prompt.
    AskNow,
    /// User previously chose Enable -> self-install without prompting.
    ProceedEnabled,
    /// User previously declined -> no install, no prompt, exit.
    SkipDeclined,
}

/// Given the raw contents of the consent marker (None = file missing),
/// decide what this run should do.
///
/// Strict and fail-closed: the marker must be valid JSON `{"status": ...}`
/// with exactly `"enabled"` or `"declined"`. Anything else — missing file,
/// malformed JSON, missing field, unknown value, or free text that merely
/// *mentions* "enabled" — re-asks (or, equivalently, never grants consent).
pub fn decide_consent(raw: Option<&str>) -> ConsentDecision {
    let Some(raw) = raw else {
        return ConsentDecision::AskNow;
    };
    let parsed: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return ConsentDecision::AskNow,
    };
    match parsed.get("status").and_then(|status| status.as_str()) {
        Some("enabled") => ConsentDecision::ProceedEnabled,
        Some("declined") => ConsentDecision::SkipDeclined,
        _ => ConsentDecision::AskNow,
    }
}

/// Canonical marker contents. Written once per decision so the choice sticks.
pub fn marker_body(enabled: bool) -> String {
    if enabled {
        "{\"status\":\"enabled\"}".to_string()
    } else {
        "{\"status\":\"declined\"}".to_string()
    }
}

#[cfg(target_os = "windows")]
pub fn marker_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| {
        PathBuf::from(appdata).join(CONSENT_FILE_NAME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_marker_asks() {
        assert_eq!(decide_consent(None), ConsentDecision::AskNow);
    }

    #[test]
    fn enabled_marker_proceeds_without_prompt() {
        assert_eq!(
            decide_consent(Some("{\"status\":\"enabled\"}")),
            ConsentDecision::ProceedEnabled
        );
    }

    #[test]
    fn declined_marker_skips_without_prompt() {
        assert_eq!(
            decide_consent(Some("{\"status\":\"declined\"}")),
            ConsentDecision::SkipDeclined
        );
    }

    #[test]
    fn garbled_or_empty_marker_reasks() {
        assert_eq!(decide_consent(Some("")), ConsentDecision::AskNow);
        assert_eq!(decide_consent(Some("not json at all")), ConsentDecision::AskNow);
        assert_eq!(decide_consent(Some("{\"status\":\"maybe\"}")), ConsentDecision::AskNow);
    }

    #[test]
    fn free_text_mentioning_enabled_never_grants_consent() {
        // Fail-closed: these contain the word "enabled" but are not the
        // canonical marker, so they must NOT enable background watching.
        assert_eq!(decide_consent(Some("enabled")), ConsentDecision::AskNow);
        assert_eq!(
            decide_consent(Some("junk enabled junk")),
            ConsentDecision::AskNow
        );
        assert_eq!(
            decide_consent(Some("junk {\"status\":\"declined\"} trailing")),
            ConsentDecision::AskNow
        );
        assert_eq!(
            decide_consent(Some("{\"status\":\"enabled\"} trailing junk")),
            ConsentDecision::AskNow
        );
    }

    #[test]
    fn missing_status_field_reasks() {
        assert_eq!(decide_consent(Some("{}")), ConsentDecision::AskNow);
        assert_eq!(
            decide_consent(Some("{\"foo\":\"enabled\"}")),
            ConsentDecision::AskNow
        );
        assert_eq!(
            decide_consent(Some("{\"status\":null}")),
            ConsentDecision::AskNow
        );
        assert_eq!(
            decide_consent(Some("{\"status\":true}")),
            ConsentDecision::AskNow
        );
    }

    #[test]
    fn unknown_status_value_reasks() {
        assert_eq!(
            decide_consent(Some("{\"status\":\"maybe\"}")),
            ConsentDecision::AskNow
        );
        assert_eq!(
            decide_consent(Some("{\"status\":\"ENABLED\"}")),
            ConsentDecision::AskNow
        );
        assert_eq!(
            decide_consent(Some("{\"status\":\"\"}")),
            ConsentDecision::AskNow
        );
    }

    #[test]
    fn marker_bodies_roundtrip_through_decide() {
        assert_eq!(
            decide_consent(Some(&marker_body(true))),
            ConsentDecision::ProceedEnabled
        );
        assert_eq!(
            decide_consent(Some(&marker_body(false))),
            ConsentDecision::SkipDeclined
        );
    }
}
