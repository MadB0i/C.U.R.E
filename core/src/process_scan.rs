//! Live-process risk scoring.
//!
//! [`ProcessInfo`] is a pure data descriptor gathered by OS-specific glue
//! (see the `gui` crate for the Windows implementation).  The scoring
//! functions in this module are platform-independent and fully
//! unit-testable.

use crate::model::RiskLevel;
use crate::signature::SignatureStatus;

// ── scoring constants (same spirit as risk.rs) ────────────────────────

const UNSIGNED_FROM_DROP_ZONE: i32 = 35;
const UNSIGNED_FROM_TRUSTED: i32 = -20;
const NO_VISIBLE_WINDOW: i32 = 15;
const RUNNING_FROM_PROFILE: i32 = 10;
const RANDOMIZED_NAME_BONUS: i32 = 25;
const SIGNED_DISCOUNT: i32 = 40;
const INVALID_SIGNATURE_PENALTY: i32 = 40;
const UNSIGNED_WITH_POSITIVE_SCORE: i32 = 10;
const KNOWN_BAD_HASH_FORCE: i32 = 999;

const DROP_ZONE_TOKENS: [&str; 4] = [
    "appdata/local/temp",
    "windows/temp",
    "downloads",
    "users/public",
];
const TRUSTED_TOKENS: [&str; 3] = ["program files", "system32", "syswow64"];

// ── types ─────────────────────────────────────────────────────────────

/// Pure data descriptor for a running process.
/// Gathered by OS glue; scored by [`score_process`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub exe_path: String,
    /// Does the process own at least one visible (non-minimized) window?
    pub has_visible_window: bool,
    /// Running from within the current user's profile directory tree
    /// (e.g. `%LOCALAPPDATA%`, `%APPDATA%`)?
    pub from_user_profile: bool,
}

/// Result of scoring a single process.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProcessScore {
    pub score: i32,
    pub risk: RiskLevel,
    pub reasons: Vec<String>,
}

// ── helpers ───────────────────────────────────────────────────────────

fn normalize(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

fn looks_randomized(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let len = stem.len();
    if len < 8 {
        return false;
    }
    let digits = stem.chars().filter(|c| c.is_ascii_digit()).count();
    let vowels = stem.chars().filter(|c| "aeiou".contains(*c)).count();
    (digits as f64 / len as f64) > 0.4
        || (len >= 10 && (vowels as f64 / len as f64) < 0.15)
}

fn in_drop_zone(path: &str) -> bool {
    let n = normalize(path);
    DROP_ZONE_TOKENS.iter().any(|t| n.contains(t))
}

fn in_trusted(path: &str) -> bool {
    let n = normalize(path);
    TRUSTED_TOKENS.iter().any(|t| n.contains(t))
}

// ── public API ────────────────────────────────────────────────────────

/// Score a single running process.  Pure function — no filesystem access.
pub fn score_process(
    info: &ProcessInfo,
    signature: &SignatureStatus,
    hash_match: Option<&str>,
) -> ProcessScore {
    let mut score: i32 = 0;
    let mut reasons: Vec<String> = Vec::new();

    // location heuristic
    if in_drop_zone(&info.exe_path) {
        score += UNSIGNED_FROM_DROP_ZONE;
        reasons.push(format!("+{UNSIGNED_FROM_DROP_ZONE} running from drop-zone location"));
    }
    if in_trusted(&info.exe_path) {
        score += UNSIGNED_FROM_TRUSTED;
        reasons.push(format!("{}{UNSIGNED_FROM_TRUSTED} running from trusted location", UNSIGNED_FROM_TRUSTED));
    }
    if info.from_user_profile {
        score += RUNNING_FROM_PROFILE;
        reasons.push(format!("+{RUNNING_FROM_PROFILE} running from user profile directory"));
    }

    // no visible window → higher suspicion (headless background process)
    if !info.has_visible_window {
        score += NO_VISIBLE_WINDOW;
        reasons.push(format!("+{NO_VISIBLE_WINDOW} no visible window"));
    }

    // randomized filename
    if looks_randomized(&info.name) {
        score += RANDOMIZED_NAME_BONUS;
        reasons.push(format!("+{RANDOMIZED_NAME_BONUS} randomized filename"));
    }

    // binary reputation
    match signature {
        SignatureStatus::ValidSigned => {
            score -= SIGNED_DISCOUNT;
            reasons.push(format!("-{SIGNED_DISCOUNT} valid signature"));
        }
        SignatureStatus::Invalid => {
            score += INVALID_SIGNATURE_PENALTY;
            reasons.push(format!("+{INVALID_SIGNATURE_PENALTY} invalid signature"));
        }
        SignatureStatus::Unsigned => {
            if score > 0 {
                score += UNSIGNED_WITH_POSITIVE_SCORE;
                reasons.push(format!("+{UNSIGNED_WITH_POSITIVE_SCORE} unsigned (elevated by other signals)"));
            }
        }
        SignatureStatus::Unknown => {}
    }

    // known-bad hash → force high risk
    if let Some(ref desc) = hash_match {
        score = KNOWN_BAD_HASH_FORCE;
        reasons.clear();
        reasons.push(format!("KNOWN BAD HASH: {}", desc));
    }

    let score = score.max(0);
    let risk = risk_level(score);
    ProcessScore { score, risk, reasons }
}

/// Score thresholds → risk level.  Mirrors `risk::risk_level`.
pub fn risk_level(score: i32) -> RiskLevel {
    if score >= 40 {
        RiskLevel::HighRisk
    } else if score >= 15 {
        RiskLevel::Suspicious
    } else {
        RiskLevel::Safe
    }
}

/// Indices of processes that should be offered for termination.
/// Pure filter — no OS calls.
pub fn pick_suspicious_processes(
    scored: &[(ProcessInfo, ProcessScore)],
) -> Vec<usize> {
    scored
        .iter()
        .enumerate()
        .filter(|(_, (_, ps))| ps.risk == RiskLevel::HighRisk)
        .map(|(i, _)| i)
        .collect()
}

// ── tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, path: &str) -> ProcessInfo {
        ProcessInfo {
            name: name.to_string(),
            pid: 1234,
            exe_path: path.to_string(),
            has_visible_window: true,
            from_user_profile: false,
        }
    }

    #[test]
    fn signed_trusted_low_score() {
        let i = info("svchost.exe", r"C:\Windows\System32\svchost.exe");
        let ps = score_process(&i, &SignatureStatus::ValidSigned, None);
        assert_eq!(ps.risk, RiskLevel::Safe);
        assert!(ps.score < 15, "score = {}", ps.score);
    }

    #[test]
    fn unsigned_drop_zone_high_risk() {
        let i = info("xkcd792.exe", r"C:\Users\Bob\Downloads\xkcd792.exe");
        let ps = score_process(&i, &SignatureStatus::Unsigned, None);
        assert_eq!(ps.risk, RiskLevel::HighRisk);
        assert!(ps.score >= 40);
    }

    #[test]
    fn headless_in_profile_gets_extra_suspicion() {
        let mut i = info("helper.exe", r"C:\Users\Bob\AppData\Local\Temp\helper.exe");
        i.has_visible_window = false;
        let ps = score_process(&i, &SignatureStatus::Unsigned, None);
        assert!(ps.score >= 35 + 15, "score = {}", ps.score);
    }

    #[test]
    fn invalid_signature_penalty() {
        let i = info("tool.exe", r"C:\Program Files\tool.exe");
        let ps = score_process(&i, &SignatureStatus::Invalid, None);
        assert!(ps.score >= 20); // -20 trusted + 40 invalid = 20
    }

    #[test]
    fn known_bad_hash_forces_high_risk() {
        let i = info("svchost.exe", r"C:\Windows\System32\svchost.exe");
        let ps = score_process(&i, &SignatureStatus::ValidSigned, Some("known malware"));
        assert_eq!(ps.risk, RiskLevel::HighRisk);
        assert!(ps.reasons[0].contains("KNOWN BAD HASH"));
    }

    #[test]
    fn randomized_name_bonus() {
        let i = info("a3f8k2m9x1.exe", r"C:\Users\Bob\Downloads\a3f8k2m9x1.exe");
        let ps = score_process(&i, &SignatureStatus::Unsigned, None);
        assert!(ps.score >= 35 + 25, "score = {}", ps.score);
    }

    #[test]
    fn pick_suspicious_returns_only_high_risk() {
        let safe = ProcessInfo { name: "ok.exe".into(), pid: 1, exe_path: r"C:\ok.exe".into(), has_visible_window: true, from_user_profile: false };
        let bad  = ProcessInfo { name: "bad.exe".into(), pid: 2, exe_path: r"C:\Downloads\bad.exe".into(), has_visible_window: false, from_user_profile: true };
        let scored = vec![
            (safe.clone(), score_process(&safe, &SignatureStatus::ValidSigned, None)),
            (bad.clone(),  score_process(&bad,  &SignatureStatus::Unsigned, None)),
        ];
        let picks = pick_suspicious_processes(&scored);
        assert_eq!(picks, vec![1]);
    }

    #[test]
    fn risk_level_thresholds() {
        assert_eq!(risk_level(0), RiskLevel::Safe);
        assert_eq!(risk_level(14), RiskLevel::Safe);
        assert_eq!(risk_level(15), RiskLevel::Suspicious);
        assert_eq!(risk_level(39), RiskLevel::Suspicious);
        assert_eq!(risk_level(40), RiskLevel::HighRisk);
    }

    #[test]
    fn visible_window_from_trusted_is_low_risk() {
        let i = info("explorer.exe", r"C:\Windows\explorer.exe");
        let ps = score_process(&i, &SignatureStatus::ValidSigned, None);
        assert_eq!(ps.risk, RiskLevel::Safe);
        assert!(ps.score < 15);
    }
}
