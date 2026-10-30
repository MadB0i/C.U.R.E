//! AI Agent & Session Intelligence: Process tree tracking, orphan detection,
//! session lifecycle states, and safe controlled termination.
//!
//! Enforces:
//! - Evidence-first status: A running process is not necessarily an active chat session.
//! - Orphan classification requires structural evidence (dead parent PID or non-existent ancestor).
//! - PID race protection and critical system process exclusion.
//! - Non-invasive shutdown: attempts graceful closure (WM_CLOSE) before force termination.

use serde::{Deserialize, Serialize};

use super::detection::{enumerate_agent_processes, AgentCategory, AgentKind, ExecutableIdentity};
use crate::operator::defense::{
    is_critical_system_process, verify_pid_identity, ProcessVerificationAnchor,
};
use crate::operator::response::{
    get_process_descendants, terminate_process_guarded, ProcessContainmentReceipt,
};

#[cfg(windows)]
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

/// Defined session and process lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionProcessState {
    /// Verified tool identity with Authenticode signature.
    Verified,
    /// Unverified process (matches image pattern, but unconfirmed signer/location).
    Unverified,
    /// Disguised binary or conflicting runtime metadata.
    SuspectMismatch,
    /// Process query access was denied by OS security.
    AccessDenied,
    /// Process was previously recorded but has now terminated.
    Exited,
    /// Ambiguous or indeterminate lifecycle state.
    UnknownSessionState,
}

impl SessionProcessState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Verified => "VERIFIED",
            Self::Unverified => "UNVERIFIED",
            Self::SuspectMismatch => "SUSPECT MISMATCH",
            Self::AccessDenied => "ACCESS DENIED",
            Self::Exited => "EXITED",
            Self::UnknownSessionState => "UNKNOWN SESSION STATE",
        }
    }
}

/// Detailed session investigation record for an observed AI agent or model server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionDetail {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub kind: AgentKind,
    pub category: AgentCategory,
    pub name: String,
    pub exe_path: String,
    pub started_unix: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub state: SessionProcessState,
    pub is_orphan: bool,
    pub child_pids: Vec<u32>,
    pub evidence_summary: String,
    pub limitations: String,
}

/// Investigates active agent processes, constructs process tree hierarchies,
/// and determines orphan and lifecycle states.
pub fn inspect_agent_sessions() -> Vec<AgentSessionDetail> {
    let detected = enumerate_agent_processes();
    let now_unix = crate::agent::recorder::now_unix();

    let mut sessions = Vec::with_capacity(detected.len());

    for proc in detected {
        let child_pids = get_process_descendants(proc.pid);

        // Determine if parent is dead / orphan state
        let is_orphan = if let Some(ppid) = proc.parent_pid {
            let anchor = ProcessVerificationAnchor {
                pid: ppid,
                expected_exe_name: String::new(),
                expected_start_time_unix: None,
            };
            matches!(
                verify_pid_identity(&anchor),
                crate::operator::defense::PidVerificationResult::ProcessExited
            )
        } else {
            false
        };

        let uptime = proc
            .started_unix
            .map(|start| now_unix.saturating_sub(start));

        let state = match proc.identity {
            ExecutableIdentity::Verified => SessionProcessState::Verified,
            ExecutableIdentity::Unverified => SessionProcessState::Unverified,
            ExecutableIdentity::SuspectMismatch => SessionProcessState::SuspectMismatch,
            ExecutableIdentity::AccessDenied => SessionProcessState::AccessDenied,
            ExecutableIdentity::PathMissing => SessionProcessState::Exited,
            ExecutableIdentity::ImageNameOnly => SessionProcessState::UnknownSessionState,
        };

        let evidence_summary = if is_orphan {
            format!(
                "{} (PID {}) is running as an ORPHANED process (parent PID {} has exited). {} child worker processes active.",
                proc.name,
                proc.pid,
                proc.parent_pid.unwrap_or(0),
                child_pids.len()
            )
        } else {
            format!(
                "{} (PID {}) running with {} child worker processes. Identity: {}.",
                proc.name,
                proc.pid,
                child_pids.len(),
                state.label()
            )
        };

        sessions.push(AgentSessionDetail {
            pid: proc.pid,
            parent_pid: proc.parent_pid,
            kind: proc.kind,
            category: proc.category,
            name: proc.name,
            exe_path: proc.exe_path,
            started_unix: proc.started_unix,
            uptime_seconds: uptime,
            state,
            is_orphan,
            child_pids,
            evidence_summary,
            limitations: proc.limitations,
        });
    }

    sessions
}

/// Attempts a non-destructive graceful shutdown by sending WM_CLOSE to any windows owned by PID.
pub fn request_graceful_shutdown(target_pid: u32) -> Result<bool, String> {
    if is_critical_system_process(target_pid, "") {
        return Err("Cannot terminate protected system process".into());
    }

    #[cfg(windows)]
    unsafe {
        struct EnumData {
            target_pid: u32,
            sent_count: usize,
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let data = &mut *(lparam.0 as *mut EnumData);
            let mut proc_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut proc_id));
            if proc_id == data.target_pid {
                let _ = PostMessageW(
                    hwnd,
                    WM_CLOSE,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
                data.sent_count += 1;
            }
            BOOL(1)
        }

        let mut data = EnumData {
            target_pid,
            sent_count: 0,
        };

        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut EnumData as isize));

        if data.sent_count > 0 {
            // Give the process a brief grace window to finish and exit cleanly
            std::thread::sleep(std::time::Duration::from_millis(600));
            let anchor = ProcessVerificationAnchor {
                pid: target_pid,
                expected_exe_name: String::new(),
                expected_start_time_unix: None,
            };
            let exited = matches!(
                verify_pid_identity(&anchor),
                crate::operator::defense::PidVerificationResult::ProcessExited
            );
            Ok(exited)
        } else {
            Ok(false)
        }
    }

    #[cfg(not(windows))]
    {
        let _ = target_pid;
        Ok(false)
    }
}

/// Safely ends an agent process tree with required explicit confirmation token.
pub fn end_agent_process_tree(
    target_pid: u32,
    expected_exe: &str,
    expected_start_time: Option<u64>,
    confirmation_token: &str,
) -> Result<ProcessContainmentReceipt, String> {
    // Attempt graceful shutdown first
    let _ = request_graceful_shutdown(target_pid);

    // If still running, execute guarded termination
    terminate_process_guarded(
        target_pid,
        expected_exe,
        expected_start_time,
        confirmation_token,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_process_state_labels_are_defined() {
        assert_eq!(SessionProcessState::Verified.label(), "VERIFIED");
        assert_eq!(SessionProcessState::Unverified.label(), "UNVERIFIED");
        assert_eq!(
            SessionProcessState::SuspectMismatch.label(),
            "SUSPECT MISMATCH"
        );
        assert_eq!(SessionProcessState::AccessDenied.label(), "ACCESS DENIED");
        assert_eq!(SessionProcessState::Exited.label(), "EXITED");
        assert_eq!(
            SessionProcessState::UnknownSessionState.label(),
            "UNKNOWN SESSION STATE"
        );
    }

    #[test]
    fn graceful_shutdown_refuses_system_process() {
        assert!(request_graceful_shutdown(0).is_err());
        assert!(request_graceful_shutdown(4).is_err());
    }
}
