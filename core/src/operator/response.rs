//! Threat Detection & Rapid Response Engine.
//!
//! Categorizes security observations into strictly defined response levels:
//! - Level 0: OBSERVE (Normal process / baseline verified)
//! - Level 1: REVIEW (Unverified agent, unknown persistent startup)
//! - Level 2: ACTION RECOMMENDED (High-confidence behavioral anomaly, canary tamper)
//! - Level 3: EMERGENCY CONTAINMENT (Confirmed external AV detection or active destructive loop)
//!
//! Principles:
//! - Never auto-terminate ordinary tools based only on unverified identity.
//! - Default to ALERT + REQUEST APPROVAL.
//! - Never bypass OS protections or disable Windows Defender.
//! - Controlled containment checks process identities to prevent PID races.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::defense::{is_critical_system_process, verify_pid_identity, ProcessVerificationAnchor};
use super::journal::audit_log;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

/// Defined response levels for operator intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseLevel {
    /// Level 0: Normal activity. Monitoring only.
    Level0Observe,
    /// Level 1: Ambiguous or unverified activity. Review evidence before any decision.
    Level1Review,
    /// Level 2: High-confidence indicator or policy divergence. Action recommended.
    Level2ActionRecommended,
    /// Level 3: Confirmed malicious detection or active compromise. Controlled containment.
    Level3EmergencyContainment,
}

impl ResponseLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Level0Observe => "Level 0: Observe",
            Self::Level1Review => "Level 1: Review Evidence",
            Self::Level2ActionRecommended => "Level 2: Action Recommended",
            Self::Level3EmergencyContainment => "Level 3: Emergency Containment",
        }
    }
}

/// Unambiguous Defender scan and remediation outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefenderScanOutcome {
    /// Scan completed cleanly; Defender confirmed no threats found.
    CleanNoThreats,
    /// File was skipped by Defender policy, configuration, or exclusion.
    SkippedByPolicy,
    /// Malicious software or threat detected (read-only audit mode with -DisableRemediation).
    ThreatDetectedAudit,
    /// Malicious threat remediated or quarantined by Defender.
    ThreatRemediated,
    /// Threat detected, but Defender could not remediate (e.g. requires reboot, admin elevation, or active lock).
    ThreatRemediationFailed,
    /// Defender CLI returned an execution error (e.g. file not found, access denied, invalid parameters).
    ScanError,
    /// Output could not be unambiguously categorized.
    UnknownOutcome,
}

impl DefenderScanOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::CleanNoThreats => "Clean: No Threats Detected",
            Self::SkippedByPolicy => "Skipped by Policy / Exclusion",
            Self::ThreatDetectedAudit => "Threat Detected (Audit Mode)",
            Self::ThreatRemediated => "Threat Remediated / Quarantined",
            Self::ThreatRemediationFailed => "Threat Remediation Failed",
            Self::ScanError => "Scan Execution Error",
            Self::UnknownOutcome => "Unknown Scan Outcome",
        }
    }
}

/// Category of threat or anomaly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreatCategory {
    /// Confirmed detection by Windows Defender or external AV engine.
    ConfirmedExternalAv,
    /// High-confidence behavioral evidence (e.g. decoy file modified by non-whitelisted process).
    CanaryTamper,
    /// Disguised binary: metadata or signature contradicts reported tool identity.
    ProcessMasquerade,
    /// Unverified process claiming agent identity or unknown persistent startup.
    UnverifiedIndicator,
    /// Benign software bug, crash, or transient lock. Not malware.
    BenignAnomaly,
}

/// An individual security or process anomaly observed by C.U.R.E.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatFinding {
    pub id: String,
    pub level: ResponseLevel,
    pub category: ThreatCategory,
    pub title: String,
    pub details: String,
    pub target_path: Option<PathBuf>,
    pub target_pid: Option<u32>,
    pub expected_exe: Option<String>,
    pub timestamp_unix: u64,
}

/// Summary result of a Windows Defender scan on a target path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenderScanResult {
    pub target_path: PathBuf,
    pub outcome: DefenderScanOutcome,
    pub status: String,
    pub return_code: i32,
    pub threat_detected: bool,
    pub detected_threats: Vec<String>,
    pub raw_output: String,
}

/// Strictly parse output and exit code from Microsoft Defender `MpCmdRun.exe`.
pub fn parse_defender_scan_output(
    code: i32,
    raw_output: &str,
    remediation_disabled: bool,
) -> (DefenderScanOutcome, String, Vec<String>) {
    let lower = raw_output.to_ascii_lowercase();

    // Check for execution error first
    if code == 1
        || lower.contains("[failed]")
        || lower.contains("cmdtool: failed")
        || lower.contains("cannot find the file specified")
        || lower.contains("access is denied")
    {
        return (
            DefenderScanOutcome::ScanError,
            format!("Defender scan execution failed (exit code {code})"),
            Vec::new(),
        );
    }

    // Check for skipped by policy
    if lower.contains("was skipped.") {
        return (
            DefenderScanOutcome::SkippedByPolicy,
            "File was skipped by Windows Defender policy or system exclusion.".into(),
            Vec::new(),
        );
    }

    // Extract any listed threats: e.g. "Threat : Trojan:Win32/..."
    let mut detected_threats = Vec::new();
    for line in raw_output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Threat") {
            let threat_name = rest.trim_start_matches([':', ' ']).trim();
            if !threat_name.is_empty() {
                detected_threats.push(threat_name.to_string());
            }
        }
    }

    // If explicit "found no threats" present and code == 0
    if lower.contains("found no threats.") && code == 0 {
        return (
            DefenderScanOutcome::CleanNoThreats,
            "Clean: Windows Defender scanned the file and detected no threats.".into(),
            Vec::new(),
        );
    }

    // When threats are detected
    let has_threat_signatures = !detected_threats.is_empty()
        || lower.contains("list of detected threats")
        || (code == 2 && !lower.contains("no threats"));

    if has_threat_signatures {
        let threat_desc = if detected_threats.is_empty() {
            "Threat detected by Defender signatures".to_string()
        } else {
            format!("Detected threat: {}", detected_threats.join(", "))
        };

        if remediation_disabled {
            return (
                DefenderScanOutcome::ThreatDetectedAudit,
                format!("{threat_desc} (-DisableRemediation was active; file remains untouched)."),
                detected_threats,
            );
        } else if code == 0
            || lower.contains("remediation successful")
            || lower.contains("quarantined")
        {
            return (
                DefenderScanOutcome::ThreatRemediated,
                format!("{threat_desc} (Successfully quarantined/remediated by Defender)."),
                detected_threats,
            );
        } else {
            return (
                DefenderScanOutcome::ThreatRemediationFailed,
                format!("{threat_desc} (Defender remediation failed with code {code}; administrator elevation or system reboot may be required)."),
                detected_threats,
            );
        }
    }

    // Code 0 with no threats mentioned and scan finished
    if code == 0 && lower.contains("scan finished") {
        return (
            DefenderScanOutcome::CleanNoThreats,
            "Scan finished with exit code 0; no threat signatures reported.".into(),
            Vec::new(),
        );
    }

    (
        DefenderScanOutcome::UnknownOutcome,
        format!("Defender exited with code {code}"),
        Vec::new(),
    )
}

/// Locates the native Microsoft Defender command line utility (`MpCmdRun.exe`).
/// Prioritizes latest platform update engine, then falls back to standard program files.
pub fn find_defender_cli() -> Option<PathBuf> {
    // Windows 10/11 Platform updates store active MpCmdRun under ProgramData\Microsoft\Windows Defender\Platform\<version>
    let platform_dir = Path::new(r"C:\ProgramData\Microsoft\Windows Defender\Platform");
    if let Ok(entries) = std::fs::read_dir(platform_dir) {
        let mut candidates: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("MpCmdRun.exe"))
            .filter(|p| p.exists())
            .collect();
        // Sort to pick latest platform engine version
        candidates.sort();
        if let Some(latest) = candidates.pop() {
            return Some(latest);
        }
    }

    let standard = PathBuf::from(r"C:\Program Files\Windows Defender\MpCmdRun.exe");
    if standard.exists() {
        return Some(standard);
    }

    None
}

/// Executes an on-demand, read-only Defender scan on an exact file using MpCmdRun.exe.
/// Invokes the binary directly without shell expansion and enforces `-DisableRemediation`
/// so that scanning never silently quarantines or destroys user data.
pub fn scan_file_with_defender(file_path: &Path) -> Result<DefenderScanResult, String> {
    if !file_path.exists() {
        return Err(format!("File does not exist: {}", file_path.display()));
    }

    let defender_exe = find_defender_cli()
        .ok_or_else(|| "Windows Defender MpCmdRun.exe not found on system".to_string())?;

    let output = Command::new(&defender_exe)
        .arg("-Scan")
        .arg("-ScanType")
        .arg("3")
        .arg("-File")
        .arg(file_path)
        .arg("-DisableRemediation")
        .output()
        .map_err(|e| format!("Failed to spawn MpCmdRun.exe: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}").trim().to_string();
    let code = output.status.code().unwrap_or(-1);

    let (outcome, status, detected_threats) = parse_defender_scan_output(code, &combined, true);
    let threat_detected = outcome == DefenderScanOutcome::ThreatDetectedAudit;

    audit_log(
        "DefenderScan",
        &file_path.to_string_lossy(),
        None,
        &status,
        serde_json::json!({
            "exit_code": code,
            "outcome": format!("{:?}", outcome),
            "threat_detected": threat_detected,
            "detected_threats": detected_threats,
            "disable_remediation": true
        }),
        None,
    );

    Ok(DefenderScanResult {
        target_path: file_path.to_path_buf(),
        outcome,
        status,
        return_code: code,
        threat_detected,
        detected_threats,
        raw_output: combined,
    })
}

/// Explicit, user-approved containment scan with remediation enabled.
/// Requires confirmation token `CONTAIN_DEFENDER_<filename>`.
/// Executes documented targeted scan without `-DisableRemediation` and verifies post-remediation file state.
pub fn contain_file_with_defender(
    file_path: &Path,
    confirmation_token: &str,
) -> Result<DefenderScanResult, String> {
    if !file_path.exists() {
        return Err(format!("File does not exist: {}", file_path.display()));
    }

    let file_name = file_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "target".into());
    let expected_token = format!("CONTAIN_DEFENDER_{file_name}");

    if confirmation_token != expected_token {
        return Err(format!(
            "Invalid confirmation token. Expected '{expected_token}', got '{confirmation_token}'"
        ));
    }

    let defender_exe = find_defender_cli()
        .ok_or_else(|| "Windows Defender MpCmdRun.exe not found on system".to_string())?;

    // Documented targeted remediation scan: calls MpCmdRun.exe -Scan -ScanType 3 -File <file_path> WITHOUT -DisableRemediation
    let output = Command::new(&defender_exe)
        .arg("-Scan")
        .arg("-ScanType")
        .arg("3")
        .arg("-File")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to spawn MpCmdRun.exe for remediation: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}").trim().to_string();
    let code = output.status.code().unwrap_or(-1);

    let (mut outcome, mut status, detected_threats) =
        parse_defender_scan_output(code, &combined, false);

    // Post-remediation verification: check if file was actually removed/quarantined by Defender
    let file_still_exists = file_path.exists();
    if outcome == DefenderScanOutcome::ThreatRemediated && file_still_exists {
        status.push_str(" [Post-check: Target file still present on disk; in-place disinfection or reboot pending]");
    } else if outcome == DefenderScanOutcome::ThreatRemediationFailed && !file_still_exists {
        outcome = DefenderScanOutcome::ThreatRemediated;
        status = "Target file successfully quarantined/removed by Windows Defender.".into();
    }

    let threat_detected = outcome == DefenderScanOutcome::ThreatRemediated
        || outcome == DefenderScanOutcome::ThreatRemediationFailed
        || outcome == DefenderScanOutcome::ThreatDetectedAudit;

    audit_log(
        "DefenderRemediate",
        &file_path.to_string_lossy(),
        Some(confirmation_token),
        &status,
        serde_json::json!({
            "exit_code": code,
            "outcome": format!("{:?}", outcome),
            "threat_detected": threat_detected,
            "detected_threats": detected_threats,
            "file_still_exists": file_still_exists,
        }),
        None,
    );

    Ok(DefenderScanResult {
        target_path: file_path.to_path_buf(),
        outcome,
        status,
        return_code: code,
        threat_detected,
        detected_threats,
        raw_output: combined,
    })
}

/// Discovers all descendant process PIDs of a given parent PID.
pub fn get_process_descendants(parent_pid: u32) -> Vec<u32> {
    let mut descendants = Vec::new();

    #[cfg(windows)]
    unsafe {
        let snapshot_res = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let snapshot: HANDLE = match snapshot_res {
            Ok(h) => h,
            Err(_) => return descendants,
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut parent_to_children: Vec<(u32, u32)> = Vec::new();

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                parent_to_children.push((entry.th32ParentProcessID, entry.th32ProcessID));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);

        // Collect tree iteratively
        let mut queue = vec![parent_pid];
        while let Some(current) = queue.pop() {
            for &(p, c) in &parent_to_children {
                if p == current && c != current && !descendants.contains(&c) {
                    descendants.push(c);
                    queue.push(c);
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        let _ = parent_pid;
    }

    descendants
}

/// Result of a process containment operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessContainmentReceipt {
    pub target_pid: u32,
    pub target_exe: String,
    pub pids_terminated: Vec<u32>,
    pub pids_failed: Vec<u32>,
    pub outcome: String,
    pub verified_stopped: bool,
    /// How the process was ended: "graceful" (WM_CLOSE), "forced"
    /// (TerminateProcess), or "already_exited". The tree variant reports
    /// "forced" for its TerminateProcess pass.
    #[serde(default)]
    pub method: String,
}

/// Safely terminates a process tree after verifying PID identity and checking critical process guards.
pub fn terminate_process_guarded(
    target_pid: u32,
    expected_exe: &str,
    expected_start_time: Option<u64>,
    confirmation_token: &str,
) -> Result<ProcessContainmentReceipt, String> {
    // 1. Validate confirmation token
    let expected_token = format!("TERMINATE_{target_pid}");
    if confirmation_token != expected_token {
        return Err(format!(
            "Invalid confirmation token. Expected '{expected_token}', got '{confirmation_token}'"
        ));
    }

    // 2. Validate against critical system process protections
    if is_critical_system_process(target_pid, expected_exe) {
        return Err(format!(
            "Process {target_pid} ('{expected_exe}') is a protected Windows system process. Termination refused."
        ));
    }

    // 3. Verify PID identity to prevent race condition
    let anchor = ProcessVerificationAnchor {
        pid: target_pid,
        expected_exe_name: expected_exe.to_string(),
        expected_start_time_unix: expected_start_time,
    };
    match verify_pid_identity(&anchor) {
        super::defense::PidVerificationResult::VerifiedMatch => {}
        super::defense::PidVerificationResult::ProcessExited => {
            return Ok(ProcessContainmentReceipt {
                target_pid,
                target_exe: expected_exe.into(),
                pids_terminated: vec![],
                pids_failed: vec![],
                outcome: "Process already exited before signal".into(),
                verified_stopped: true,
                method: "already_exited".into(),
            });
        }
        super::defense::PidVerificationResult::PidReusedMismatch { actual_exe } => {
            return Err(format!(
                "PID RACE DETECTED: PID {target_pid} has been recycled by an unrelated process ('{actual_exe}'). Refusing termination."
            ));
        }
        super::defense::PidVerificationResult::CriticalSystemProtected => {
            return Err("Protected system process. Action prohibited.".into());
        }
        super::defense::PidVerificationResult::AccessDenied => {
            return Err("Access denied while querying process identity.".into());
        }
    }

    // 4. Collect child processes
    let children = get_process_descendants(target_pid);

    let mut terminated = Vec::new();
    let mut failed = Vec::new();

    // Terminate children first (leaf to root)
    for &child_pid in children.iter().rev() {
        if is_critical_system_process(child_pid, "") {
            continue;
        }
        #[cfg(windows)]
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, child_pid) {
                if TerminateProcess(handle, 1).is_ok() {
                    terminated.push(child_pid);
                } else {
                    failed.push(child_pid);
                }
                let _ = CloseHandle(handle);
            }
        }
    }

    // Terminate root process
    #[cfg(windows)]
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, target_pid) {
            if TerminateProcess(handle, 1).is_ok() {
                terminated.push(target_pid);
            } else {
                failed.push(target_pid);
            }
            let _ = CloseHandle(handle);
        } else {
            failed.push(target_pid);
        }
    }

    // 5. Post-termination verification
    let stopped = matches!(
        verify_pid_identity(&anchor),
        super::defense::PidVerificationResult::ProcessExited
            | super::defense::PidVerificationResult::PidReusedMismatch { .. }
    );

    let outcome = if stopped {
        format!(
            "Successfully terminated target PID {target_pid} and {} child processes",
            terminated.len().saturating_sub(1)
        )
    } else {
        format!(
            "Termination attempted, but PID {target_pid} may still be running (check permissions)"
        )
    };

    audit_log(
        "ProcessContainment",
        &format!("PID {target_pid} ({expected_exe})"),
        Some(confirmation_token),
        if stopped { "Terminated" } else { "Failed" },
        serde_json::json!({
            "terminated_pids": terminated,
            "failed_pids": failed,
            "verified_stopped": stopped
        }),
        if stopped { None } else { Some(&outcome) },
    );

    Ok(ProcessContainmentReceipt {
        target_pid,
        target_exe: expected_exe.into(),
        pids_terminated: terminated,
        pids_failed: failed,
        outcome,
        verified_stopped: stopped,
        method: "forced".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_level_ordering_is_correct() {
        assert!(ResponseLevel::Level0Observe < ResponseLevel::Level1Review);
        assert!(ResponseLevel::Level1Review < ResponseLevel::Level2ActionRecommended);
        assert!(ResponseLevel::Level2ActionRecommended < ResponseLevel::Level3EmergencyContainment);
    }

    #[test]
    fn invalid_confirmation_token_is_rejected() {
        let res = terminate_process_guarded(1234, "test.exe", None, "WRONG_TOKEN");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Invalid confirmation token"));
    }

    #[test]
    fn critical_system_process_refuses_containment() {
        let res = terminate_process_guarded(4, "System", None, "TERMINATE_4");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("protected Windows system process"));
    }

    #[test]
    fn test_defender_output_parsing_clean_scan() {
        let raw = "Scan starting...\nScan finished.\nScanning D:\\test.txt found no threats.\n";
        let (outcome, status, threats) = parse_defender_scan_output(0, raw, true);
        assert_eq!(outcome, DefenderScanOutcome::CleanNoThreats);
        assert!(status.contains("Clean"));
        assert!(threats.is_empty());
    }

    #[test]
    fn test_defender_output_parsing_skipped_by_policy() {
        let raw = "Scan starting...\nScan finished.\nScanning C:\\Windows\\System32\\notepad.exe was skipped.\n";
        let (outcome, status, threats) = parse_defender_scan_output(0, raw, true);
        assert_eq!(outcome, DefenderScanOutcome::SkippedByPolicy);
        assert!(status.contains("skipped"));
        assert!(threats.is_empty());
    }

    #[test]
    fn test_defender_output_parsing_threat_detected_audit() {
        let raw = "Scan starting...\nLIST OF DETECTED THREATS\nThreat : Trojan:Win32/Wacatac.B!ml\nResources : D:\\sample.exe\nScan finished.\n";
        let (outcome, status, threats) = parse_defender_scan_output(2, raw, true);
        assert_eq!(outcome, DefenderScanOutcome::ThreatDetectedAudit);
        assert!(status.contains("Trojan:Win32/Wacatac.B!ml"));
        assert_eq!(threats, vec!["Trojan:Win32/Wacatac.B!ml"]);
    }

    #[test]
    fn test_defender_output_parsing_threat_remediated() {
        let raw = "Scan starting...\nLIST OF DETECTED THREATS\nThreat : Virus:DOS/EICAR_Test_File\nRemediation successful.\nScan finished.\n";
        let (outcome, status, threats) = parse_defender_scan_output(0, raw, false);
        assert_eq!(outcome, DefenderScanOutcome::ThreatRemediated);
        assert!(status.contains("quarantined/remediated"));
        assert_eq!(threats, vec!["Virus:DOS/EICAR_Test_File"]);
    }

    #[test]
    fn test_defender_output_parsing_remediation_failed() {
        let raw = "Scan starting...\nLIST OF DETECTED THREATS\nThreat : Ransom:Win32/Locky\nRemediation failed.\nScan finished.\n";
        let (outcome, status, threats) = parse_defender_scan_output(2, raw, false);
        assert_eq!(outcome, DefenderScanOutcome::ThreatRemediationFailed);
        assert!(status.contains("remediation failed"));
        assert_eq!(threats, vec!["Ransom:Win32/Locky"]);
    }

    #[test]
    fn test_defender_output_parsing_execution_error() {
        let raw = "[Failed][0x80070002] The system cannot find the file specified.\nCmdTool: Failed with hr = 0x80070002\n";
        let (outcome, status, threats) = parse_defender_scan_output(1, raw, true);
        assert_eq!(outcome, DefenderScanOutcome::ScanError);
        assert!(status.contains("execution failed"));
        assert!(threats.is_empty());
    }

    #[test]
    fn test_find_defender_cli() {
        if let Some(path) = find_defender_cli() {
            assert!(path.is_file());
            assert_eq!(path.file_name().unwrap(), "MpCmdRun.exe");
        }
    }

    #[test]
    fn test_defender_contain_file_token_validation() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("malware_sample.exe");
        std::fs::write(&file, b"fake_content").unwrap();

        // 1. Wrong token
        let res = contain_file_with_defender(&file, "WRONG_TOKEN");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Invalid confirmation token"));

        // 2. Non-existent file
        let non_existent = temp.path().join("does_not_exist.exe");
        let res = contain_file_with_defender(&non_existent, "CONTAIN_DEFENDER_does_not_exist.exe");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("File does not exist"));
    }

    #[test]
    fn test_defender_scan_nonexistent_file() {
        let res = scan_file_with_defender(Path::new(r"C:\Windows\Temp\cure_nonexistent_file.tmp"));
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("File does not exist"));
    }
}
