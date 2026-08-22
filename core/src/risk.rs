use std::path::Path;

use crate::model::{PersistenceEntry, RiskLevel, ScoredEntry};
use crate::signature::SignatureStatus;

const SIGNED_DISCOUNT: i32 = 40;
const INVALID_SIGNATURE_PENALTY: i32 = 40;
const UNSIGNED_WITH_WARNINGS_PENALTY: i32 = 10;

const DROP_ZONE_TOKENS: [&str; 4] = [
    "appdata/local/temp",
    "windows/temp",
    "downloads",
    "users/public",
];

const TRUSTED_TOKENS: [&str; 3] = ["program files", "system32", "syswow64"];

const SNEAKY_POWERSHELL_TOKENS: [&str; 3] = ["-enc", "-windowstyle hidden", "-w hidden"];

pub fn extract_program_path(command: &str) -> &str {
    let trimmed = command.trim();
    if let Some(unquoted) = trimmed.strip_prefix('"') {
        return match unquoted.find('"') {
            Some(end) => &unquoted[..end],
            None => unquoted,
        };
    }
    trimmed.split_whitespace().next().unwrap_or(trimmed)
}

pub fn looks_randomized(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let compact: Vec<char> = stem.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let len = compact.len();
    if len < 6 {
        return false;
    }
    let digits = compact.iter().filter(|c| c.is_ascii_digit()).count();
    if digits * 5 > len * 2 {
        return true;
    }
    if len >= 8 {
        let vowels = compact
            .iter()
            .filter(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'))
            .count();
        if vowels * 10 < len {
            return true;
        }
    }
    false
}

pub fn risk_level(score: i32) -> RiskLevel {
    if score >= 40 {
        RiskLevel::HighRisk
    } else if score >= 15 {
        RiskLevel::Suspicious
    } else {
        RiskLevel::Safe
    }
}

/// Live scoring path used by the CLI and GUI: performs real Authenticode
/// and known-bad-hash lookups when a resolved executable path is available.
/// Callers should resolve the path once via
/// [`crate::signature::resolve_executable_path`] and pass it in.
pub fn score_entry(entry: &PersistenceEntry, exe_path: Option<&Path>) -> ScoredEntry {
    let signature = match exe_path {
        Some(path) => crate::signature::check_signature(path),
        None => SignatureStatus::Unknown,
    };
    let hash_match = exe_path.and_then(crate::hash_intel::check_hash);
    score_with_signals(entry, signature, hash_match.as_deref())
}

/// Pure scoring core: identical logic to [`score_entry`] but with the
/// signature verdict / hash-IOC result injected, so it is unit-testable on
/// any platform without touching the filesystem or WinTrust.
///
/// Scoring model (Detection Engine v2):
/// - heuristic points as before (drop zone +30, trusted location -20,
///   randomized name +25, sneaky PowerShell +25, profile exe +10)
/// - valid Authenticode signature: **-40**
/// - invalid/tampered signature: **+40**
/// - unsigned binary: **+10 only** when other warning signs are already present
/// - unknown/unresolvable signature: no change (v1 behaviour preserved)
/// - a known-bad hash match hard-forces [`RiskLevel::HighRisk`], overriding
///   the computed score entirely.
pub fn score_with_signals(
    entry: &PersistenceEntry,
    signature: SignatureStatus,
    hash_match: Option<&str>,
) -> ScoredEntry {
    let mut score: i32 = 0;
    let mut reasons: Vec<String> = Vec::new();

    let command_norm = normalize(&entry.command);
    let program_norm = normalize(extract_program_path(&entry.command));

    let drop_zone = DROP_ZONE_TOKENS.iter().any(|t| command_norm.contains(t));
    let trusted = TRUSTED_TOKENS.iter().any(|t| command_norm.contains(t));

    if drop_zone {
        score += 30;
        reasons.push("+30 command path sits in a temp/downloads/public drop zone".to_string());
    }
    if trusted {
        score -= 20;
        reasons.push("-20 command path is a trusted install location (Program Files/System32)".to_string());
    }

    if looks_randomized(&entry.name) {
        score += 25;
        reasons.push("+25 entry name looks randomly generated".to_string());
    }

    let sneaky_powershell = command_norm.contains("powershell")
        && SNEAKY_POWERSHELL_TOKENS.iter().any(|t| command_norm.contains(t));
    if sneaky_powershell {
        score += 25;
        reasons.push("+25 PowerShell invoked with encoded command or hidden window".to_string());
    }

    let profile_exe =
        program_norm.ends_with(".exe") && program_norm.contains("/users/") && !trusted;
    if profile_exe {
        score += 10;
        reasons.push("+10 executable runs directly from a user profile folder".to_string());
    }

    // --- Detection Engine v2: binary reputation signals ---
    // Applied after heuristics so the unsigned penalty can see whether
    // other warning signs are already on the board. Reason strings double
    // as GUI chip tags — keep them short (see app.js reasonChipLabel).
    match signature {
        SignatureStatus::ValidSigned => {
            reasons.push(format!("-{SIGNED_DISCOUNT} Valid Signature"));
            score -= SIGNED_DISCOUNT;
        }
        SignatureStatus::Invalid => {
            reasons.push(format!("+{INVALID_SIGNATURE_PENALTY} Invalid Signature"));
            score += INVALID_SIGNATURE_PENALTY;
        }
        SignatureStatus::Unsigned if score > 0 => {
            reasons.push(format!(
                "+{UNSIGNED_WITH_WARNINGS_PENALTY} Unsigned Binary"
            ));
            score += UNSIGNED_WITH_WARNINGS_PENALTY;
        }
        SignatureStatus::Unsigned | SignatureStatus::Unknown => {}
    }

    let mut scored = ScoredEntry {
        entry: entry.clone(),
        score: score.max(0),
        risk: RiskLevel::Safe,
        reasons,
    };
    scored.risk = risk_level(scored.score);

    if let Some(_description) = hash_match {
        // Hard IOC override: beats every heuristic, even a valid signature.
        // The full malware description stays in the embedded IOC list
        // (core/src/known_bad_hashes.json); the chip tag stays short.
        scored.reasons.push("Known Malware Hash".to_string());
        scored.risk = RiskLevel::HighRisk;
    }

    scored
}

fn normalize(text: &str) -> String {
    text.trim().to_ascii_lowercase().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PersistenceSource;

    fn entry(name: &str, command: &str) -> PersistenceEntry {
        PersistenceEntry::new(PersistenceSource::StartupFolder, name, command, "")
    }

    // ---- heuristic behaviour, unchanged from v1 (Unknown / no hash) ----

    #[test]
    fn trusted_binary_is_safe() {
        let scored = score_with_signals(
            &entry("AcmeTray", r#""C:\Program Files\Acme\acmetray.exe" /quiet"#),
            SignatureStatus::Unknown,
            None,
        );
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert_eq!(scored.score, 0);
        assert!(scored.reasons.iter().any(|r| r.starts_with("-20")));
    }

    #[test]
    fn system32_task_is_safe() {
        let scored = score_with_signals(
            &entry("DiskCleanup", r"C:\Windows\System32\cleanmgr.exe /autoclean"),
            SignatureStatus::Unknown,
            None,
        );
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert_eq!(scored.score, 0);
    }

    #[test]
    fn temp_folder_random_name_is_high_risk() {
        let scored = score_with_signals(
            &entry("a7x9k2p9", r"C:\Users\bob\AppData\Local\Temp\a7x9k2p9.exe"),
            SignatureStatus::Unknown,
            None,
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored.score >= 40);
        assert!(scored.reasons.iter().any(|r| r.starts_with("+30")));
        assert!(scored.reasons.iter().any(|r| r.starts_with("+25 entry name")));
        assert!(scored.reasons.iter().any(|r| r.starts_with("+10")));
    }

    #[test]
    fn encoded_powershell_is_flagged() {
        let scored = score_with_signals(
            &entry(
                "OfficeTelemetry",
                "powershell.exe -nop -enc SQBFAFgAKABOAGUAdwAtAE8AYgBqAGUAYwB0AA",
            ),
            SignatureStatus::Unknown,
            None,
        );
        assert!(scored.reasons.iter().any(|r| r.contains("PowerShell")));
        assert_eq!(scored.score, 25);
        assert_eq!(scored.risk, RiskLevel::Suspicious);
    }

    #[test]
    fn hidden_window_powershell_is_flagged() {
        let scored = score_with_signals(
            &entry(
                "UpdaterSvc",
                "poWERSHELL -WindowStyle Hidden -File C:\\tools\\sync.ps1",
            ),
            SignatureStatus::Unknown,
            None,
        );
        assert!(scored.reasons.iter().any(|r| r.contains("PowerShell")));
        assert_eq!(scored.score, 25);
    }

    #[test]
    fn plain_user_profile_exe_gets_small_bump_only() {
        let scored = score_with_signals(
            &entry("toolkit", r"C:\Users\bob\toolkit.exe"),
            SignatureStatus::Unknown,
            None,
        );
        assert_eq!(scored.score, 10);
        assert_eq!(scored.risk, RiskLevel::Safe);
    }

    #[test]
    fn unresolved_path_preserves_v1_scoring() {
        // no resolved file => Unknown signature + no hash lookup => identical
        let e = entry(
            "OfficeTelemetry",
            "powershell.exe -nop -enc SQBFAFgAKABOAGUAdwAtAE8AYgBqAGUAYwB0AA",
        );
        let live = score_entry(&e, None);
        let pure = score_with_signals(&e, SignatureStatus::Unknown, None);
        assert_eq!(live.score, pure.score);
        assert_eq!(live.risk, pure.risk);
        assert_eq!(pure.score, 25);
        assert!(!pure
            .reasons
            .iter()
            .any(|r| r.contains("signature") || r.contains("hash")));
    }

    // ---- Detection Engine v2: signature scoring ----

    #[test]
    fn valid_signature_scores_lower_than_unsigned_for_same_path() {
        let e = entry("a7x9k2p9", r"C:\Users\bob\AppData\Local\Temp\a7x9k2p9.exe");
        let signed = score_with_signals(&e, SignatureStatus::ValidSigned, None);
        let unsigned = score_with_signals(&e, SignatureStatus::Unsigned, None);

        assert!(signed.score < unsigned.score);
        // 65 heuristic - 40 discount = 25 => drops a level
        assert_eq!(signed.score, 25);
        assert_eq!(signed.risk, RiskLevel::Suspicious);
        assert!(signed.reasons.iter().any(|r| r.starts_with("-40")));

        // unsigned +10 fires only because other warning signs exist here
        assert_eq!(unsigned.score, 75);
        assert_eq!(unsigned.risk, RiskLevel::HighRisk);
        assert!(unsigned
            .reasons
            .iter()
            .any(|r| r.starts_with("+10 Unsigned Binary")));
    }

    #[test]
    fn unsigned_binary_without_other_signals_is_not_penalized() {
        // benign-looking location: unsigned alone must not add points
        let scored = score_with_signals(
            &entry("AcmeTray", r#""C:\Program Files\Acme\acmetray.exe" /quiet"#),
            SignatureStatus::Unsigned,
            None,
        );
        assert_eq!(scored.score, 0);
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert!(
            scored.reasons.iter().all(|r| r.starts_with("-20")),
            "only the trusted-location discount should be present"
        );
    }

    #[test]
    fn invalid_signature_forces_high_risk() {
        let scored = score_with_signals(
            &entry("toolkit", r"C:\Users\bob\toolkit.exe"),
            SignatureStatus::Invalid,
            None,
        );
        // 10 profile bump + 40 invalid signature = 50
        assert_eq!(scored.score, 50);
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored.reasons.iter().any(|r| r.starts_with("+40")));
    }

    #[test]
    fn valid_signature_can_downgrade_a_suspicious_entry() {
        // sneaky powershell +25 normally Suspicious; signed tool calms it down
        let scored = score_with_signals(
            &entry(
                "OfficeTelemetry",
                "powershell.exe -nop -enc SQBFAFgAKABOAGUAdwAtAE8AYgBqAGUAYwB0AA",
            ),
            SignatureStatus::ValidSigned,
            None,
        );
        assert_eq!(scored.score, 0);
        assert_eq!(scored.risk, RiskLevel::Safe);
    }

    // ---- Detection Engine v2: hash IOC override ----

    #[test]
    fn known_bad_hash_overrides_everything_including_valid_signature() {
        let scored = score_with_signals(
            &entry("DiskCleanup", r"C:\Windows\System32\cleanmgr.exe /autoclean"),
            SignatureStatus::ValidSigned,
            Some("CURE test fixture #1 (placeholder string hash)"),
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.starts_with("Known Malware Hash")));
    }

    #[test]
    fn known_bad_hash_overrides_low_heuristic_score() {
        let scored = score_with_signals(
            &entry("toolkit", r"C:\Users\bob\toolkit.exe"),
            SignatureStatus::Unsigned,
            Some("CURE demo fixture"),
        );
        assert_eq!(scored.score, 20); // score itself stays honest...
        assert_eq!(scored.risk, RiskLevel::HighRisk); // ...but the IOC wins
    }

    // ---- live pipeline on Windows (real WinTrust + real files) ----

    #[cfg(windows)]
    #[test]
    fn live_pipeline_verifies_signed_system_binary_as_safe() {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let notepad = Path::new(&system_root).join("System32").join("notepad.exe");
        if !notepad.is_file() {
            return;
        }
        let cmd = format!("{} /newinstance", notepad.display());
        let resolved = crate::signature::resolve_executable_path(&cmd).expect("notepad exists");
        let scored = score_entry(&entry("NotepadAutostart", &cmd), Some(resolved.as_path()));
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.starts_with("-40 Valid Signature")));
    }

    #[cfg(windows)]
    #[test]
    fn live_pipeline_flags_tampered_copy_as_high_risk() {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let source = Path::new(&system_root)
            .join("System32")
            .join("chkdsk.exe");
        if !source.is_file() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("tampered-chkdsk.exe");
        let mut bytes = std::fs::read(&source).unwrap();
        let flip_at = bytes.len() / 2;
        bytes[flip_at] ^= 0xFF;
        std::fs::write(&copy, &bytes).unwrap();

        let cmd = format!("{}", copy.display());
        let scored = score_entry(&entry("DiskDoctor", &cmd), Some(copy.as_path()));
        // Tampering must never look like a trusted binary: on embedded-signed
        // systems the verdict is Invalid (+40); on catalog-only systems it
        // degrades to Unsigned (+10 alongside the other warning signs).
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored.reasons.iter().any(|r| r.starts_with("+40")
            || r.starts_with("+10 Unsigned Binary")));
        assert!(!scored
            .reasons
            .iter()
            .any(|r| r.starts_with("-40 Valid Signature")));
    }

    // ---- helpers (unchanged) ----

    #[test]
    fn quoted_and_unquoted_paths_are_extracted() {
        assert_eq!(
            extract_program_path(r#""C:\Program Files\A\b.exe" --flag"#),
            r"C:\Program Files\A\b.exe"
        );
        assert_eq!(extract_program_path("C:\\tools\\x.exe -go"), r"C:\tools\x.exe");
        assert_eq!(extract_program_path("justname"), "justname");
        assert_eq!(extract_program_path("   "), "");
    }

    #[test]
    fn randomized_names_detected_legit_names_pass() {
        assert!(looks_randomized("a7x9k2p9"));
        assert!(looks_randomized("a7x9k2p9.bat"));
        assert!(looks_randomized("xjqvzkwt"));
        assert!(!looks_randomized("Backup2016.cfg"));
        assert!(!looks_randomized("WindowsDefender"));
        assert!(!looks_randomized("svchost"));
        assert!(!looks_randomized("Update"));
    }

    #[test]
    fn risk_level_boundaries() {
        assert_eq!(risk_level(14), RiskLevel::Safe);
        assert_eq!(risk_level(15), RiskLevel::Suspicious);
        assert_eq!(risk_level(39), RiskLevel::Suspicious);
        assert_eq!(risk_level(40), RiskLevel::HighRisk);
    }
}
