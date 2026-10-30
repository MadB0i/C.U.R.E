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

/// Detailed taxonomy separating process execution, on-disk artifacts, active locking, and recovery viability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStateKind {
    /// Live process executing on the host, but no local session directory or files exist.
    RunningProcessOnly,
    /// Saved session artifacts exist on disk, but the process is not currently running.
    ExistingSessionFilesOnly,
    /// Active session: Live running process with concurrently open/updated session store.
    ActiveSession,
    /// Offline session files exist and can be safely parsed or restored without data loss.
    RecoverableSession,
    /// Proprietary/opaque binary format (e.g. SQLite, LevelDB, internal IPC) - recovery unsupported.
    UnsupportedSessionFormat,
}

impl SessionStateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RunningProcessOnly => "Running Process Only",
            Self::ExistingSessionFilesOnly => "Existing Session Files (Inactive)",
            Self::ActiveSession => "Active Session",
            Self::RecoverableSession => "Recoverable Session",
            Self::UnsupportedSessionFormat => "Unsupported Session Format",
        }
    }
}

/// Real session recovery capabilities discovered from supported local on-disk stores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecoveryInfo {
    pub supported: bool,
    pub status: String,
    pub state_kind: SessionStateKind,
    pub session_count: usize,
    pub storage_path: Option<String>,
    pub latest_session_utc: Option<String>,
    /// Unix seconds of the most recent artifact modification, when known.
    /// Distinguishes "recently active" from "artifacts exist but stale".
    pub latest_activity_unix: Option<u64>,
    pub details: String,
}

/// An artifact modified within this window counts as recent activity.
pub const RECENT_ACTIVITY_WINDOW_SECS: u64 = 30 * 60;

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
    pub recovery: SessionRecoveryInfo,
}

/// Discovers genuine on-disk session journals or workspaces for supported tools.
pub fn discover_agent_session_storage(kind: AgentKind) -> SessionRecoveryInfo {
    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let app_data = std::env::var("APPDATA").unwrap_or_default();

    match kind {
        AgentKind::Codex => {
            let p1 = std::path::Path::new(&local_app_data).join("OpenAI").join("Codex").join("sessions");
            let p2 = std::path::Path::new(&user_profile).join(".codex").join("sessions");
            inspect_session_dir_candidate(&[&p1, &p2], "Codex JSON session journal store", false)
        }
        AgentKind::OpenCode => {
            let p1 = std::path::Path::new(&app_data).join("ai.opencode.desktop").join("sessions");
            let p2 = std::path::Path::new(&user_profile).join(".opencode").join("sessions");
            inspect_session_dir_candidate(&[&p1, &p2], "OpenCode desktop session history database", false)
        }
        AgentKind::Antigravity => {
            let p = std::path::Path::new(&user_profile).join(".gemini").join("antigravity").join("brain");
            inspect_session_dir_candidate(&[&p], "Antigravity active conversation workspaces", false)
        }
        AgentKind::Claude => {
            let p1 = std::path::Path::new(&user_profile).join(".claude").join("projects");
            let p2 = std::path::Path::new(&user_profile).join(".claude");
            inspect_session_dir_candidate(&[&p1, &p2], "Claude Code project transcripts", false)
        }
        AgentKind::Aider => {
            let p = std::path::Path::new(&user_profile).join(".aider.chat.history.md");
            if p.is_file() {
                let count = 1;
                let m = p.metadata().ok();
                let latest_str = m.as_ref().and_then(|meta| meta.modified().ok()).map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.to_rfc3339()
                });
                let latest_unix = m
                    .and_then(|meta| meta.modified().ok())
                    .and_then(|t| crate::storage::fsx::unix_seconds(Ok(t)));
                SessionRecoveryInfo {
                    supported: true,
                    status: "RECOVERABLE SESSIONS FOUND".into(),
                    state_kind: SessionStateKind::RecoverableSession,
                    session_count: count,
                    storage_path: Some(p.to_string_lossy().to_string()),
                    latest_session_utc: latest_str,
                    latest_activity_unix: latest_unix,
                    details: format!("Aider markdown chat history discovered at {}", p.display()),
                }
            } else {
                SessionRecoveryInfo {
                    supported: true,
                    status: "NO SAVED SESSIONS FOUND".into(),
                    state_kind: SessionStateKind::ExistingSessionFilesOnly,
                    session_count: 0,
                    storage_path: Some(p.to_string_lossy().to_string()),
                    latest_session_utc: None,
                    latest_activity_unix: None,
                    details: "Aider chat history file does not exist in user profile.".into(),
                }
            }
        }
        AgentKind::Cursor => {
            let p = std::path::Path::new(&app_data).join("Cursor").join("User").join("workspaceStorage");
            inspect_session_dir_candidate(&[&p], "Cursor workspace storage (VSCode SQLite/VSCDB format)", true)
        }
        AgentKind::Windsurf => {
            let p = std::path::Path::new(&app_data).join("Windsurf").join("User").join("workspaceStorage");
            inspect_session_dir_candidate(&[&p], "Windsurf workspace storage (Codeium SQLite format)", true)
        }
        AgentKind::Ollama => {
            let p = std::path::Path::new(&user_profile).join(".ollama").join("models");
            inspect_session_dir_candidate(&[&p], "Ollama model weight repository (inference cache, not conversation session)", true)
        }
        _ => SessionRecoveryInfo {
            supported: false,
            status: "SESSION RECOVERY UNSUPPORTED".into(),
            state_kind: SessionStateKind::UnsupportedSessionFormat,
            session_count: 0,
            storage_path: None,
            latest_session_utc: None,
            latest_activity_unix: None,
            details: format!(
                "{} does not expose a documented offline session restoration mechanism. Running process presence is monitored, but conversation recovery is not supported.",
                kind.label()
            ),
        },
    }
}

fn inspect_session_dir_candidate(
    candidates: &[&std::path::Path],
    description: &str,
    is_proprietary_format: bool,
) -> SessionRecoveryInfo {
    for dir in candidates {
        if dir.is_dir() {
            let mut count = 0usize;
            let mut latest_mod: Option<std::time::SystemTime> = None;
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    count += 1;
                    if let Ok(m) = e.metadata() {
                        if let Ok(modified) = m.modified() {
                            if latest_mod.is_none() || latest_mod.unwrap() < modified {
                                latest_mod = Some(modified);
                            }
                        }
                    }
                }
            }
            let latest_str = latest_mod.map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            });
            let latest_unix = latest_mod.and_then(|t| crate::storage::fsx::unix_seconds(Ok(t)));

            if is_proprietary_format {
                return SessionRecoveryInfo {
                    supported: false,
                    status: "UNSUPPORTED SESSION FORMAT (PROPRIETARY)".into(),
                    state_kind: SessionStateKind::UnsupportedSessionFormat,
                    session_count: count,
                    storage_path: Some(dir.to_string_lossy().to_string()),
                    latest_session_utc: latest_str,
                    latest_activity_unix: latest_unix,
                    details: format!(
                        "{description}: Found {count} database files. Direct offline recovery unsupported to prevent binary database corruption."
                    ),
                };
            }

            return SessionRecoveryInfo {
                supported: true,
                status: "RECOVERABLE SESSIONS FOUND".into(),
                state_kind: SessionStateKind::RecoverableSession,
                session_count: count,
                storage_path: Some(dir.to_string_lossy().to_string()),
                latest_session_utc: latest_str,
                latest_activity_unix: latest_unix,
                details: format!(
                    "{description}: {count} session artifacts discovered at {}",
                    dir.display()
                ),
            };
        }
    }

    if is_proprietary_format {
        return SessionRecoveryInfo {
            supported: false,
            status: "NO SESSION STORE FOUND".into(),
            state_kind: SessionStateKind::UnsupportedSessionFormat,
            session_count: 0,
            storage_path: candidates.first().map(|p| p.to_string_lossy().to_string()),
            latest_session_utc: None,
            latest_activity_unix: None,
            details: format!("{description}: Directory not found on disk."),
        };
    }

    SessionRecoveryInfo {
        supported: true,
        status: "NO SAVED SESSIONS FOUND".into(),
        state_kind: SessionStateKind::ExistingSessionFilesOnly,
        session_count: 0,
        storage_path: candidates.first().map(|p| p.to_string_lossy().to_string()),
        latest_session_utc: None,
        latest_activity_unix: None,
        details: format!(
            "{description}: Local session directory exists but contains zero saved sessions."
        ),
    }
}

/// Enumerates offline session directories across all supported agents regardless of whether processes are running.
pub fn inspect_offline_agent_sessions() -> Vec<(AgentKind, SessionRecoveryInfo)> {
    AgentKind::ALL
        .into_iter()
        .map(|kind| (kind, discover_agent_session_storage(kind)))
        .collect()
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

        let mut recovery = discover_agent_session_storage(proc.kind);
        if recovery.supported && recovery.session_count > 0 {
            // "Active session" requires RECENT artifact activity, not merely a
            // non-empty store: a running tool with days-old artifacts is a
            // process with stale session files, not an active conversation.
            let recent = recovery
                .latest_activity_unix
                .map(|t| now_unix.saturating_sub(t) < RECENT_ACTIVITY_WINDOW_SECS)
                .unwrap_or(false);
            if recent {
                recovery.state_kind = SessionStateKind::ActiveSession;
                recovery.status = "ACTIVE SESSION (RECENT ARTIFACT ACTIVITY)".into();
            } else {
                recovery.state_kind = SessionStateKind::ExistingSessionFilesOnly;
                recovery.status =
                    "PROCESS RUNNING (SESSION ARTIFACTS PRESENT BUT NOT RECENTLY ACTIVE)".into();
            }
        } else if !recovery.supported {
            // Keep UnsupportedSessionFormat with clear explanation
        } else {
            recovery.state_kind = SessionStateKind::RunningProcessOnly;
            recovery.status = "PROCESS RUNNING (NO SAVED SESSIONS)".into();
        }

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
            recovery,
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
    fn session_taxonomy_distinguishes_all_five_lifecycle_states() {
        assert_eq!(
            SessionStateKind::RunningProcessOnly.label(),
            "Running Process Only"
        );
        assert_eq!(
            SessionStateKind::ExistingSessionFilesOnly.label(),
            "Existing Session Files (Inactive)"
        );
        assert_eq!(SessionStateKind::ActiveSession.label(), "Active Session");
        assert_eq!(
            SessionStateKind::RecoverableSession.label(),
            "Recoverable Session"
        );
        assert_eq!(
            SessionStateKind::UnsupportedSessionFormat.label(),
            "Unsupported Session Format"
        );
    }

    #[test]
    fn inspect_session_dir_candidate_discovers_offline_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let sessions_dir = temp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Create 2 mock session files
        std::fs::write(sessions_dir.join("session_1.json"), "{\"chat\": 1}").unwrap();
        std::fs::write(sessions_dir.join("session_2.json"), "{\"chat\": 2}").unwrap();

        let info = inspect_session_dir_candidate(&[&sessions_dir], "Mock Codex store", false);
        assert!(info.supported);
        assert_eq!(info.session_count, 2);
        assert_eq!(info.state_kind, SessionStateKind::RecoverableSession);
        assert!(info.status.contains("RECOVERABLE"));
        assert!(info.latest_session_utc.is_some());
    }

    #[test]
    fn inspect_session_dir_identifies_proprietary_format_honestly() {
        let temp = tempfile::tempdir().unwrap();
        let vscdb_dir = temp.path().join("workspaceStorage");
        std::fs::create_dir_all(&vscdb_dir).unwrap();
        std::fs::write(vscdb_dir.join("state.vscdb"), "SQLite format 3").unwrap();

        let info = inspect_session_dir_candidate(&[&vscdb_dir], "Cursor proprietary storage", true);
        assert!(!info.supported);
        assert_eq!(info.session_count, 1);
        assert_eq!(info.state_kind, SessionStateKind::UnsupportedSessionFormat);
        assert!(info.status.contains("UNSUPPORTED"));
        assert!(info.details.contains("unsupported"));
    }

    #[test]
    fn empty_or_missing_session_directory_reports_honestly() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("non_existent_folder");

        let info = inspect_session_dir_candidate(&[&missing], "Missing store", false);
        assert!(info.supported);
        assert_eq!(info.session_count, 0);
        assert_eq!(info.state_kind, SessionStateKind::ExistingSessionFilesOnly);
        assert_eq!(info.status, "NO SAVED SESSIONS FOUND");
    }

    #[test]
    fn graceful_shutdown_refuses_system_process() {
        assert!(request_graceful_shutdown(0).is_err());
        assert!(request_graceful_shutdown(4).is_err());
    }
}
