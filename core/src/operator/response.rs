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
    pub status: String,
    pub return_code: i32,
    pub threat_detected: bool,
    pub raw_output: String,
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
    // MpCmdRun exit codes:
    // 0 = No threat found
    // 2 = Threat detected
    let lower_combined = combined.to_ascii_lowercase();
    let has_threat_text = lower_combined.contains("threat")
        && !lower_combined.contains("no threats")
        && !lower_combined.contains("0 threat");
    let threat_detected = code == 2 || has_threat_text;

    let status = if code == 0 {
        "Clean (No threats detected)".into()
    } else if code == 2 || threat_detected {
        "Threat Detected by Defender (Remediation Disabled)".into()
    } else {
        format!("Defender scan exited with code {code}")
    };

    audit_log(
        "DefenderScan",
        &file_path.to_string_lossy(),
        None,
        &status,
        serde_json::json!({ "exit_code": code, "threat_detected": threat_detected, "disable_remediation": true }),
        None,
    );

    Ok(DefenderScanResult {
        target_path: file_path.to_path_buf(),
        status,
        return_code: code,
        threat_detected,
        raw_output: combined,
    })
}

/// Explicit, user-approved containment scan with remediation enabled.
/// Requires confirmation token `CONTAIN_DEFENDER_<filename>`.
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

    // Remediation scan: does NOT pass -DisableRemediation
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
    let status = if code == 0 {
        "Remediation scan completed: No threats found".into()
    } else if code == 2 {
        "Remediation applied: Threat quarantined or remediated by Defender".into()
    } else {
        format!("Defender remediation exited with code {code}")
    };

    audit_log(
        "DefenderRemediate",
        &file_path.to_string_lossy(),
        Some(confirmation_token),
        &status,
        serde_json::json!({ "exit_code": code }),
        None,
    );

    Ok(DefenderScanResult {
        target_path: file_path.to_path_buf(),
        status,
        return_code: code,
        threat_detected: code == 2,
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
