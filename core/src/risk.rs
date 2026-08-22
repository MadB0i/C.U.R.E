use crate::model::{PersistenceEntry, RiskLevel, ScoredEntry};

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

pub fn score_entry(entry: &PersistenceEntry) -> ScoredEntry {
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

    let score = score.max(0);
    ScoredEntry {
        entry: entry.clone(),
        score,
        risk: risk_level(score),
        reasons,
    }
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

    #[test]
    fn trusted_binary_is_safe() {
        let scored = score_entry(&entry(
            "AcmeTray",
            r#""C:\Program Files\Acme\acmetray.exe" /quiet"#,
        ));
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert_eq!(scored.score, 0);
        assert!(scored.reasons.iter().any(|r| r.starts_with("-20")));
    }

    #[test]
    fn system32_task_is_safe() {
        let scored = score_entry(&entry(
            "DiskCleanup",
            r"C:\Windows\System32\cleanmgr.exe /autoclean",
        ));
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert_eq!(scored.score, 0);
    }

    #[test]
    fn temp_folder_random_name_is_high_risk() {
        let scored = score_entry(&entry(
            "a7x9k2p9",
            r"C:\Users\bob\AppData\Local\Temp\a7x9k2p9.exe",
        ));
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored.score >= 40);
        assert!(scored.reasons.iter().any(|r| r.starts_with("+30")));
        assert!(scored.reasons.iter().any(|r| r.starts_with("+25 entry name")));
        assert!(scored.reasons.iter().any(|r| r.starts_with("+10")));
    }

    #[test]
    fn encoded_powershell_is_flagged() {
        let scored = score_entry(&entry(
            "OfficeTelemetry",
            "powershell.exe -nop -enc SQBFAFgAKABOAGUAdwAtAE8AYgBqAGUAYwB0AA",
        ));
        assert!(scored.reasons.iter().any(|r| r.contains("PowerShell")));
        assert_eq!(scored.score, 25);
        assert_eq!(scored.risk, RiskLevel::Suspicious);
    }

    #[test]
    fn hidden_window_powershell_is_flagged() {
        let scored = score_entry(&entry(
            "UpdaterSvc",
            "poWERSHELL -WindowStyle Hidden -File C:\\tools\\sync.ps1",
        ));
        assert!(scored.reasons.iter().any(|r| r.contains("PowerShell")));
        assert_eq!(scored.score, 25);
    }

    #[test]
    fn plain_user_profile_exe_gets_small_bump_only() {
        let scored = score_entry(&entry("toolkit", r"C:\Users\bob\toolkit.exe"));
        assert_eq!(scored.score, 10);
        assert_eq!(scored.risk, RiskLevel::Safe);
    }

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
