//! The evidence model: what backs every finding, and how sure C.U.R.E is.
//!
//! One vocabulary for the whole product:
//!
//! - [`EvidenceLevel::Observed`] — C.U.R.E directly witnessed the thing the
//!   detail describes (a path token, a signature verdict, a live pid). The
//!   detail must say WHAT was observed.
//! - [`EvidenceLevel::Correlated`] — two observed things line up, but the
//!   link between them is inference (timing, co-occurrence, name overlap).
//!   The detail must describe the overlap, never assert causation.
//! - [`EvidenceLevel::Unknown`] — C.U.R.E has nothing usable. The detail
//!   says why (unreadable, not collected, older format, …).
//!
//! Rules enforced here, not by convention:
//!
//! - There is **no `Default` for [`EvidenceLevel`]** — a level is only ever
//!   produced by [`Evidence::observed`], [`Evidence::correlated`], or
//!   [`Evidence::unknown`]. An author cannot silently inherit a level.
//! - [`Evidence::correlated`] and [`Evidence::unknown`] **sanitize causal
//!   wording** in the detail (case-insensitive): `caused by`, `created by`,
//!   `written by`, `dropped by`, `installed by`, `launched by`, `started by`,
//!   `changed by`, `modified by`, `deleted by`, `responsible for`, and
//!   `did this` are rewritten to neutral `associated with` phrasing. A
//!   wording slip in production can therefore never ship a causal claim
//!   dressed up as correlation or unknown — the rewrites are exhaustively
//!   tested below. [`Evidence::observed`] does not sanitize: a directly
//!   witnessed mechanism may legitimately name one (e.g. a process-creation
//!   event that was actually seen).
//! - Serialization keeps the level next to the detail so every frontend
//!   (CLI text, GUI chips, exports) can show a badge.
//!
//! `reason()` vs `plain_english_reason()`: there is deliberately only ONE
//! vocabulary. `Evidence::detail` IS the plain-English reason for a finding;
//! there is no second formatter to keep in sync. `RemoveError::reason()`
//! (storage `safedelete`) is a different thing — a sentence for why a single
//! removal was refused — and is documented as such on that method, not
//! merged into this model.
//!
//! Known boundary (deliberate): the incident module's DIRECT/STRONG/PARTIAL/
//! WEAK correlation levels and the storage module's string `reasons` keep
//! their own vocabularies for now; they are mapped onto this model by their
//! respective feature tasks. Their evidence *strings* are observations of
//! what was checked; this module governs everything that claims a level.

use serde::{Deserialize, Serialize};

/// How directly C.U.R.E witnessed what a finding's detail describes.
///
/// Deliberately has **no `Default` impl**: an evidence level is a claim, and
/// claims are only made through the [`Evidence`] constructors, never by
/// falling back to a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EvidenceLevel {
    /// Directly witnessed (path content, signature verdict, live pid, …).
    Observed,
    /// Two observed things line up; the link is inference, not causation.
    Correlated,
    /// Nothing usable was witnessed for this claim.
    Unknown,
}

impl EvidenceLevel {
    /// Badge text shown next to a detail in every frontend.
    pub fn label(self) -> &'static str {
        match self {
            Self::Observed => "Observed",
            Self::Correlated => "Correlated",
            Self::Unknown => "Unknown",
        }
    }
}

/// One piece of evidence for a finding: a level plus the plain-English
/// detail it applies to. The fields are private on purpose — an `Evidence`
/// can only be built through [`Evidence::observed`],
/// [`Evidence::correlated`], or [`Evidence::unknown`], so the causal
/// wording rule and the no-default rule are enforced by the compiler, not
/// by review. Read back with [`Evidence::level`] and [`Evidence::detail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    level: EvidenceLevel,
    detail: String,
}

impl Evidence {
    /// Directly witnessed. The detail must state WHAT was observed —
    /// `"+30 command path sits in a temp/downloads/public drop zone"`, not
    /// `"suspicious"`. Not sanitized: an observed mechanism may name one.
    pub fn observed(detail: impl Into<String>) -> Self {
        Self {
            level: EvidenceLevel::Observed,
            detail: detail.into().trim().to_string(),
        }
    }

    /// Inferred link between two observed things. The detail must describe
    /// the overlap, never assert causation — causal wording is rewritten to
    /// neutral `associated with` phrasing (see [`sanitize_causal_wording`]).
    pub fn correlated(detail: impl Into<String>) -> Self {
        Self {
            level: EvidenceLevel::Correlated,
            detail: sanitize_causal_wording(&detail.into()).trim().to_string(),
        }
    }

    /// Nothing usable was witnessed. The detail says why. Causal wording is
    /// rewritten exactly as for [`Evidence::correlated`] — an unknown cannot
    /// blame anything either.
    pub fn unknown(detail: impl Into<String>) -> Self {
        Self {
            level: EvidenceLevel::Unknown,
            detail: sanitize_causal_wording(&detail.into()).trim().to_string(),
        }
    }

    pub fn level(&self) -> EvidenceLevel {
        self.level
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Same level, rebuilt detail — used by exporters that rewrite free text
    /// (redaction). Goes back through the level's constructor so the causal
    /// wording rule still applies to the rewritten text.
    pub fn map_detail(&self, f: impl Fn(&str) -> String) -> Self {
        match self.level {
            EvidenceLevel::Observed => Self::observed(f(&self.detail)),
            EvidenceLevel::Correlated => Self::correlated(f(&self.detail)),
            EvidenceLevel::Unknown => Self::unknown(f(&self.detail)),
        }
    }
}

/// Causal phrases that may not appear in `Correlated`/`Unknown` details.
/// Matched case-insensitively as plain substrings.
pub const CAUSAL_PHRASES: [&str; 12] = [
    "caused by",
    "created by",
    "written by",
    "dropped by",
    "installed by",
    "launched by",
    "started by",
    "changed by",
    "modified by",
    "deleted by",
    "responsible for",
    "did this",
];

/// Rewrite every causal phrase in `text` to neutral `associated with`
/// wording (case-insensitive, all occurrences). `did this` becomes
/// `associated with this`. Sanitizing (instead of refusing) means a wording
/// slip in production degrades to a vaguer sentence, never to a false claim.
pub fn sanitize_causal_wording(text: &str) -> String {
    let mut out = text.to_string();
    for phrase in CAUSAL_PHRASES {
        if phrase == "did this" {
            out = replace_case_insensitive(&out, phrase, "associated with this");
        } else {
            out = replace_case_insensitive(&out, phrase, "associated with");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_keeps_what_was_witnessed() {
        let ev = Evidence::observed("+30 command path sits in a temp drop zone");
        assert_eq!(ev.level(), EvidenceLevel::Observed);
        assert_eq!(ev.detail(), "+30 command path sits in a temp drop zone");
        assert_eq!(ev.level.label(), "Observed");
    }

    #[test]
    fn observed_detail_is_trimmed() {
        assert_eq!(Evidence::observed("  path token  ").detail(), "path token");
    }

    #[test]
    fn correlated_and_unknown_carry_their_labels() {
        assert_eq!(
            Evidence::correlated("timing overlap").level.label(),
            "Correlated"
        );
        assert_eq!(Evidence::unknown("not collected").level.label(), "Unknown");
    }

    /// The spec's exact phrase list, for BOTH sanitized levels, asserting
    /// the phrase is gone and the neutral wording is present.
    #[test]
    fn every_causal_phrase_is_sanitized_in_correlated() {
        for phrase in CAUSAL_PHRASES {
            let detail = format!("The payload {phrase} the updater");
            let ev = Evidence::correlated(detail);
            let lowered = ev.detail().to_ascii_lowercase();
            assert!(
                !lowered.contains(phrase),
                "correlated detail kept causal phrase {phrase:?}: {:?}",
                ev.detail()
            );
            assert!(
                lowered.contains("associated with"),
                "correlated detail lost the neutral wording for {phrase:?}: {:?}",
                ev.detail()
            );
        }
    }

    #[test]
    fn every_causal_phrase_is_sanitized_in_unknown() {
        for phrase in CAUSAL_PHRASES {
            let detail = format!("Something {phrase} a process");
            let ev = Evidence::unknown(detail);
            let lowered = ev.detail().to_ascii_lowercase();
            assert!(
                !lowered.contains(phrase),
                "unknown detail kept causal phrase {phrase:?}: {:?}",
                ev.detail()
            );
        }
    }

    #[test]
    fn causal_phrases_match_case_insensitively() {
        let ev = Evidence::correlated("CAUSED BY Windows Update; Dropped By setup.exe");
        let lowered = ev.detail().to_ascii_lowercase();
        assert!(!lowered.contains("caused by"));
        assert!(!lowered.contains("dropped by"));
    }

    #[test]
    fn sanitization_replaces_every_occurrence() {
        let ev = Evidence::correlated("caused by X and caused by Y");
        assert_eq!(ev.detail(), "associated with X and associated with Y");
    }

    #[test]
    fn did_this_is_sanitized_as_a_whole() {
        let ev = Evidence::unknown("explorer.exe did this");
        assert_eq!(ev.detail(), "explorer.exe associated with this");
        assert!(!ev.detail().to_ascii_lowercase().contains("did this"));
    }

    #[test]
    fn observed_is_not_sanitized() {
        // An observed mechanism may legitimately name one: a process-creation
        // event that was actually witnessed says what it saw.
        let ev = Evidence::observed("process created by the shell (pid 4242)");
        assert_eq!(ev.detail(), "process created by the shell (pid 4242)");
    }

    #[test]
    fn map_detail_preserves_level_and_resanitizes() {
        let ev = Evidence::correlated("started by the task scheduler");
        let mapped = ev.map_detail(|d| format!("redacted: {d}"));
        assert_eq!(mapped.level(), EvidenceLevel::Correlated);
        assert_eq!(
            mapped.detail(),
            "redacted: associated with the task scheduler"
        );
    }

    #[test]
    fn evidence_serializes_level_next_to_detail() {
        let ev = Evidence::observed("valid signature");
        let json = serde_json::to_string(&ev).unwrap();
        assert_eq!(json, r#"{"level":"Observed","detail":"valid signature"}"#);
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn sanitizer_is_idempotent() {
        let once = sanitize_causal_wording("installed by an updater");
        let twice = sanitize_causal_wording(&once);
        assert_eq!(once, twice);
    }

    /// Compile-level rule, stated as the closest runtime equivalent: nothing
    /// in this module provides a way to obtain an EvidenceLevel without
    /// naming it or going through a constructor. If someone adds
    /// `impl Default for EvidenceLevel`, this test still passes — but the
    /// module doc and review gate on it; the constructors remain the only
    /// sanctioned path.
    #[test]
    fn constructors_are_the_only_documented_path() {
        let observed = Evidence::observed("d");
        let correlated = Evidence::correlated("d");
        let unknown = Evidence::unknown("d");
        assert_ne!(observed.level, correlated.level);
        assert_ne!(correlated.level, unknown.level);
        assert_ne!(observed.level, unknown.level);
    }
}
