//! Supported AI coding-agent discovery.
//!
//! A detected process proves only that the executable was running at the
//! time of the snapshot. It does not prove that the agent generated code,
//! called a model, held a session open, or changed a file. Those are
//! separate observations and are represented separately by the radar and
//! the flight recorder.
//!
//! Discovery is deliberately name-based: C.U.R.E recognises the documented
//! executable names of the two supported agents and reports everything else
//! as unsupported. It does not guess from a Node.js process, a terminal, or
//! a window title.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The AI coding agents this build can recognise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    OpenCode,
    CodexCli,
}

impl AgentKind {
    pub const SUPPORTED: [AgentKind; 2] = [AgentKind::OpenCode, AgentKind::CodexCli];

    pub fn id(self) -> &'static str {
        match self {
            AgentKind::OpenCode => "opencode",
            AgentKind::CodexCli => "codex_cli",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::OpenCode => "OpenCode",
            AgentKind::CodexCli => "Codex CLI",
        }
    }

    /// Executable stems that identify the agent. Matching is exact and
    /// case-insensitive; a substring match is never sufficient.
    pub fn executable_stems(self) -> &'static [&'static str] {
        match self {
            AgentKind::OpenCode => &["opencode", "opencode-cli", "opencode-tui"],
            AgentKind::CodexCli => &["codex", "codex-cli", "codex-tui"],
        }
    }

    pub fn from_executable_name(name: &str) -> Option<Self> {
        let stem = name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(name)
            .trim()
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_ascii_lowercase();
        Self::SUPPORTED
            .into_iter()
            .find(|kind| kind.executable_stems().contains(&stem.as_str()))
    }

    /// How this build obtained the executable identity.
    pub fn integration_note(self) -> &'static str {
        match self {
            AgentKind::OpenCode => {
                "Supported local integration: process image name only. No session API is consumed."
            }
            AgentKind::CodexCli => {
                "Supported local integration: process image name only. No session API is consumed."
            }
        }
    }
}

/// One observed process for a supported agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedAgentProcess {
    pub kind: AgentKind,
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
    /// Unix seconds from the process creation time, when Windows returned it.
    pub started_unix: Option<u64>,
    /// How strongly the executable was identified.
    pub identity: ExecutableIdentity,
}

/// Executable identity is evidence, not a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableIdentity {
    /// The image name matched and the full image path resolved.
    ResolvedPath,
    /// The image name matched but the path could not be read.
    ImageNameOnly,
    /// The image name matched but the resolved path no longer exists.
    PathMissing,
}

impl ExecutableIdentity {
    pub fn label(self) -> &'static str {
        match self {
            ExecutableIdentity::ResolvedPath => "image name matched; full path resolved",
            ExecutableIdentity::ImageNameOnly => "image name matched; path unavailable",
            ExecutableIdentity::PathMissing => "image name matched; path missing",
        }
    }
}

/// Derive the identity state from a resolved path.
pub fn identity_for(path: &str) -> ExecutableIdentity {
    if path.trim().is_empty() {
        ExecutableIdentity::ImageNameOnly
    } else if Path::new(path).is_file() {
        ExecutableIdentity::ResolvedPath
    } else {
        ExecutableIdentity::PathMissing
    }
}

/// Snapshot the supported agent processes. Read-only; never opens a process
/// for anything but path and creation-time queries.
#[cfg(windows)]
pub fn enumerate_agent_processes() -> Vec<DetectedAgentProcess> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut results = Vec::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Ok(snapshot) = snapshot else {
        return results;
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        let _ = unsafe { CloseHandle(snapshot) };
        return results;
    }

    loop {
        let pid = entry.th32ProcessID;
        let name = String::from_utf16_lossy(
            &entry
                .szExeFile
                .iter()
                .take_while(|&&c| c != 0)
                .copied()
                .collect::<Vec<u16>>(),
        );

        if let Some(kind) = AgentKind::from_executable_name(&name) {
            let mut exe_path = String::new();
            let mut started_unix = None;
            unsafe {
                if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut buf = [0u16; 1024];
                    let mut size = buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buf.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok()
                    {
                        exe_path = String::from_utf16_lossy(&buf[..size as usize]).to_string();
                    }

                    let mut creation = FILETIME::default();
                    let mut exit = FILETIME::default();
                    let mut kernel = FILETIME::default();
                    let mut user = FILETIME::default();
                    if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)
                        .is_ok()
                    {
                        let ticks = (u64::from(creation.dwHighDateTime) << 32)
                            | u64::from(creation.dwLowDateTime);
                        if ticks >= 116_444_736_000_000_000 {
                            started_unix = Some((ticks - 116_444_736_000_000_000) / 10_000_000);
                        }
                    }
                    let _ = CloseHandle(handle);
                }
            }

            results.push(DetectedAgentProcess {
                kind,
                pid,
                name,
                identity: identity_for(&exe_path),
                exe_path,
                started_unix,
            });
        }

        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    let _ = unsafe { CloseHandle(snapshot) };
    results.sort_by_key(|p| (p.kind.id(), p.pid));
    results
}

#[cfg(not(windows))]
pub fn enumerate_agent_processes() -> Vec<DetectedAgentProcess> {
    Vec::new()
}

/// Count processes per kind. Pure helper used by the radar.
pub fn processes_by_kind(
    processes: &[DetectedAgentProcess],
    kind: AgentKind,
) -> Vec<&DetectedAgentProcess> {
    processes.iter().filter(|p| p.kind == kind).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_only_documented_executable_names() {
        assert_eq!(
            AgentKind::from_executable_name("opencode.exe"),
            Some(AgentKind::OpenCode)
        );
        assert_eq!(
            AgentKind::from_executable_name(r"C:\Tools\OPENCODE.EXE"),
            Some(AgentKind::OpenCode)
        );
        assert_eq!(
            AgentKind::from_executable_name("codex-cli.exe"),
            Some(AgentKind::CodexCli)
        );
        assert_eq!(
            AgentKind::from_executable_name("codex-tui"),
            Some(AgentKind::CodexCli)
        );
        assert_eq!(AgentKind::from_executable_name("node.exe"), None);
        assert_eq!(
            AgentKind::from_executable_name("my-opencode-helper.exe"),
            None
        );
        assert_eq!(AgentKind::from_executable_name("codex-helper.exe"), None);
    }

    #[test]
    fn identity_degrades_honestly() {
        assert_eq!(identity_for(""), ExecutableIdentity::ImageNameOnly);
        assert_eq!(
            identity_for(r"C:\definitely-not-a-real-file\opencode.exe"),
            ExecutableIdentity::PathMissing
        );
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(AgentKind::OpenCode.id(), "opencode");
        assert_eq!(AgentKind::CodexCli.id(), "codex_cli");
        assert_eq!(AgentKind::OpenCode.label(), "OpenCode");
        assert_eq!(AgentKind::CodexCli.label(), "Codex CLI");
    }
}
