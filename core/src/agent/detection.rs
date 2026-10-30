//! Supported AI coding-agent and local model server discovery.
//!
//! A detected process proves only that the executable was running at the
//! time of the snapshot. It does not prove that the agent generated code,
//! called a model, held a session open, or changed a file. Those are
//! separate observations and are represented separately by the radar and
//! the flight recorder.
//!
//! Discovery examines process metadata, executable paths, parent-child
//! relationships, and masked command line parameters. Secrets, tokens, and
//! credentials are NEVER stored or logged.

use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Secret Masking (guarantees credentials are never stored or logged)
// ---------------------------------------------------------------------------

const SECRET_FLAG_NAMES: [&str; 16] = [
    "api-key",
    "api_key",
    "apikey",
    "token",
    "auth-token",
    "auth_token",
    "access-token",
    "access_token",
    "secret",
    "password",
    "passwd",
    "key",
    "private-key",
    "refresh-token",
    "session-token",
    "authorization",
];

const SECRET_TOKEN_PREFIXES: [&str; 9] = [
    "sk-",
    "sk-ant-",
    "sk-proj-",
    "ghp_",
    "gho_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "xoxa-",
];

const SECRET_HEADERS: [&str; 3] = ["authorization:", "proxy-authorization:", "x-api-key:"];

/// Mask secret-looking material in a command line BEFORE it is stored or
/// printed. Never keep the raw string: this is the single entry point for
/// command lines in Agent Radar.
pub fn mask_command_line(cmd: &str) -> String {
    if cmd.trim().is_empty() {
        return String::new();
    }
    let mut out: Vec<String> = Vec::with_capacity(cmd.len() / 4 + 1);
    let mut mask_next = false;
    for word in cmd.split_whitespace() {
        let masked = mask_word(word, &mut mask_next);
        out.push(masked);
    }
    out.join(" ")
}

fn unquote(word: &str) -> (&str, char) {
    for q in ['"', '\''] {
        if let Some(inner) = word.strip_prefix(q).and_then(|w| w.strip_suffix(q)) {
            if word.len() >= 2 {
                return (inner, q);
            }
        }
    }
    (word, '\0')
}

fn mask_word(word: &str, mask_next: &mut bool) -> String {
    let (core, q) = unquote(word);
    let lower = core.to_ascii_lowercase();
    let trimmed = lower.trim_matches(['\'', '"']);

    // Bearer token keyword: keep Bearer label, mask the following token
    if trimmed == "bearer" {
        *mask_next = true;
        return word.to_string();
    }

    if *mask_next {
        *mask_next = false;
        return format!("{q}***{q}");
    }

    if SECRET_HEADERS.iter().any(|h| lower.ends_with(h)) {
        *mask_next = true;
        return format!("{q}{core}{q}");
    }

    if let Some(eq) = core.find('=') {
        let lhs = &core[..eq];
        let rhs = &core[eq + 1..];
        if is_secret_name(lhs) || (!rhs.is_empty() && is_secret_like(rhs)) {
            return format!("{q}{lhs}=***{q}");
        }
        return word.to_string();
    }

    let flag = lower.trim_start_matches('-');
    if core.starts_with('-') && SECRET_FLAG_NAMES.contains(&flag) {
        *mask_next = true;
        return word.to_string();
    }

    if is_secret_like(core) {
        return format!("{q}***{q}");
    }
    word.to_string()
}

fn is_secret_name(name: &str) -> bool {
    let bare = name.trim_matches('"').trim_start_matches('-');
    let lower = bare.to_ascii_lowercase();
    if SECRET_FLAG_NAMES.contains(&lower.as_str()) {
        return true;
    }
    if !bare.is_empty()
        && bare
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        && ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD"]
            .iter()
            .any(|suffix| lower.ends_with(&suffix.to_ascii_lowercase()))
    {
        return true;
    }
    false
}

fn is_secret_like(token: &str) -> bool {
    if token.len() >= 10 && SECRET_TOKEN_PREFIXES.iter().any(|p| token.starts_with(p)) {
        return true;
    }
    if token.len() < 20 {
        return false;
    }
    let digits = token.chars().filter(|c| c.is_ascii_digit()).count();
    let has_upper = token.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = token.chars().any(|c| c.is_ascii_lowercase());
    if digits >= 8 || (has_upper && has_lower && digits >= 1) {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Model & Taxonomy
// ---------------------------------------------------------------------------

/// Distinguishes autonomous AI agents from local model servers and editor extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCategory {
    /// Autonomous or interactive agent tool (e.g. OpenCode, Codex, Claude Code, Aider).
    Agent,
    /// Local inference/model server (e.g. Ollama).
    ModelServer,
    /// IDE/editor extension or plugin (e.g. GitHub Copilot, Continue, Cline).
    Extension,
}

impl AgentCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "AI Agent",
            Self::ModelServer => "Local Model Server",
            Self::Extension => "Editor Extension",
        }
    }
}

/// The AI coding tools and model servers recognisable by C.U.R.E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    OpenCode,
    Codex,
    Claude,
    Aider,
    Ollama,
    Cursor,
    Cline,
    Continue,
    GeminiCli,
    Copilot,
}

impl AgentKind {
    pub const ALL: [AgentKind; 10] = [
        AgentKind::OpenCode,
        AgentKind::Codex,
        AgentKind::Claude,
        AgentKind::Aider,
        AgentKind::Ollama,
        AgentKind::Cursor,
        AgentKind::Cline,
        AgentKind::Continue,
        AgentKind::GeminiCli,
        AgentKind::Copilot,
    ];

    pub const SUPPORTED: [AgentKind; 10] = Self::ALL;

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Aider => "aider",
            Self::Ollama => "ollama",
            Self::Cursor => "cursor",
            Self::Cline => "cline",
            Self::Continue => "continue",
            Self::GeminiCli => "gemini_cli",
            Self::Copilot => "copilot",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenCode => "OpenCode",
            Self::Codex => "Codex CLI",
            Self::Claude => "Claude Code",
            Self::Aider => "Aider",
            Self::Ollama => "Ollama",
            Self::Cursor => "Cursor",
            Self::Cline => "Cline",
            Self::Continue => "Continue",
            Self::GeminiCli => "Gemini CLI",
            Self::Copilot => "GitHub Copilot",
        }
    }

    pub fn category(self) -> AgentCategory {
        match self {
            Self::Ollama => AgentCategory::ModelServer,
            Self::Copilot => AgentCategory::Extension,
            _ => AgentCategory::Agent,
        }
    }

    pub fn is_model_server(self) -> bool {
        self.category() == AgentCategory::ModelServer
    }

    /// Executable stems that identify the agent. Matching is exact and
    /// case-insensitive; a substring match is never sufficient.
    pub fn executable_stems(self) -> &'static [&'static str] {
        match self {
            Self::OpenCode => &["opencode", "opencode-cli", "opencode-tui"],
            Self::Codex => &["codex", "codex-cli", "codex-tui"],
            Self::Claude => &["claude", "claude-code"],
            Self::Aider => &["aider"],
            Self::Ollama => &["ollama", "ollama_app", "ollama-app"],
            Self::Cursor => &["cursor"],
            Self::Cline => &["cline"],
            Self::Continue => &["continue"],
            Self::GeminiCli => &["gemini", "gemini-cli"],
            Self::Copilot => &["copilot"],
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
        Self::ALL
            .into_iter()
            .find(|kind| kind.executable_stems().contains(&stem.as_str()))
    }

    /// Notes on integration, telemetry, and scope.
    pub fn integration_note(self) -> &'static str {
        match self {
            Self::Ollama => {
                "Local inference server: hosts weights and exposes an HTTP API; not an autonomous agent."
            }
            Self::Copilot => {
                "IDE extension: agentic features integrated into VS Code / editor environment."
            }
            _ => {
                "Supported local integration: process image name and metadata. No cloud or session API consumed."
            }
        }
    }
}

/// One observed process for a supported tool or model server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedAgentProcess {
    pub kind: AgentKind,
    pub category: AgentCategory,
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe_path: String,
    pub command_line: Option<String>,
    /// Unix seconds from the process creation time, when Windows returned it.
    pub started_unix: Option<u64>,
    /// How strongly the executable was identified.
    pub identity: ExecutableIdentity,
    pub evidence: String,
    pub confidence: String,
    pub limitations: String,
    pub is_model_server: bool,
}

/// Executable identity is evidence, not a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableIdentity {
    /// Canonical installation path or verified Authenticode signer confirms the genuine tool.
    Verified,
    /// Image name matched and path resolved, but tool identity is unconfirmed
    /// (e.g. scratch directory, unverified user path, or unsigned binary).
    Unverified,
    /// Authenticode signer or binary metadata contradicts the claimed tool identity
    /// (e.g. Python or generic utility renamed to match an AI tool image name).
    SuspectMismatch,
    /// Process matched image name in snapshot, but query rights were denied.
    AccessDenied,
    /// Image name matched in snapshot, but process path could not be queried.
    ImageNameOnly,
    /// Image path was retrieved but the file no longer exists on disk.
    PathMissing,
}

impl ExecutableIdentity {
    pub fn label(self) -> &'static str {
        match self {
            ExecutableIdentity::Verified => {
                "verified tool identity (valid Authenticode signature from publisher)"
            }
            ExecutableIdentity::Unverified => {
                "unverified process (image name match; tool identity unconfirmed)"
            }
            ExecutableIdentity::SuspectMismatch => {
                "suspect identity mismatch (metadata contradicts claimed tool)"
            }
            ExecutableIdentity::AccessDenied => "access denied (process query limited info denied)",
            ExecutableIdentity::ImageNameOnly => "image name matched; full path unavailable",
            ExecutableIdentity::PathMissing => "image name matched; path missing from disk",
        }
    }

    pub fn confidence_label(self) -> &'static str {
        match self {
            ExecutableIdentity::Verified => "Observed (Verified tool identity)",
            ExecutableIdentity::Unverified => {
                "Unverified (Image name match; tool identity unconfirmed)"
            }
            ExecutableIdentity::SuspectMismatch => {
                "Unverified (Metadata mismatch; likely renamed binary)"
            }
            ExecutableIdentity::AccessDenied => "Unknown (Process query access denied)",
            ExecutableIdentity::ImageNameOnly => "Unknown (Process path unavailable)",
            ExecutableIdentity::PathMissing => "Unknown (Target image missing from disk)",
        }
    }

    pub fn is_verified(self) -> bool {
        matches!(self, ExecutableIdentity::Verified)
    }

    pub fn is_suspect(self) -> bool {
        matches!(self, ExecutableIdentity::SuspectMismatch)
    }
}

/// Evaluate executable identity from tool kind, resolved path, and PE Authenticode metadata.
pub fn verify_process_identity(kind: AgentKind, path: &str) -> ExecutableIdentity {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return ExecutableIdentity::ImageNameOnly;
    }
    let p = Path::new(trimmed);
    if !p.is_file() {
        return ExecutableIdentity::PathMissing;
    }

    let lower_path = trimmed.to_ascii_lowercase().replace('/', "\\");

    // Check for scratch, temporary or untrusted locations
    let is_temp_or_scratch = lower_path.contains("\\temp\\")
        || lower_path.contains("\\tmp\\")
        || lower_path.contains("\\appdata\\local\\temp\\")
        || lower_path.contains("\\windows\\temp\\")
        || lower_path.contains("\\downloads\\")
        || lower_path.contains("\\desktop\\");

    // Check Authenticode signature
    let sig = crate::signature::signature_detail(p);
    if let Some(ref pub_name) = sig.publisher {
        let pub_lower = pub_name.to_ascii_lowercase();

        // Check for conflicting known runtimes:
        // E.g. Python executable renamed to opencode.exe or claude.exe
        let is_python_runtime = pub_lower.contains("python software foundation");
        let is_node_runtime =
            pub_lower.contains("node.js foundation") || pub_lower.contains("openjs foundation");
        let is_ms_system = pub_lower.contains("microsoft windows");

        if is_python_runtime && kind != AgentKind::Aider {
            return ExecutableIdentity::SuspectMismatch;
        }

        if is_ms_system && kind != AgentKind::Copilot {
            return ExecutableIdentity::SuspectMismatch;
        }

        if is_node_runtime && kind != AgentKind::Cline && kind != AgentKind::Continue {
            return ExecutableIdentity::SuspectMismatch;
        }

        // Expected publishers for specific tools:
        let is_expected_publisher = match kind {
            AgentKind::Ollama => pub_lower.contains("ollama"),
            AgentKind::Cursor => pub_lower.contains("anysphere"),
            AgentKind::Copilot => pub_lower.contains("microsoft"),
            AgentKind::Claude => pub_lower.contains("anthropic"),
            AgentKind::GeminiCli => pub_lower.contains("google"),
            AgentKind::Codex => pub_lower.contains("openai"),
            AgentKind::Aider => pub_lower.contains("aider") || pub_lower.contains("paul gauthier"),
            AgentKind::Continue => pub_lower.contains("continue"),
            AgentKind::Cline => pub_lower.contains("cline"),
            _ => false,
        };

        if is_expected_publisher {
            if sig.status == crate::signature::SignatureStatus::ValidSigned
                || sig.status == crate::signature::SignatureStatus::ValidRevocationUnknown
            {
                return ExecutableIdentity::Verified;
            } else {
                return ExecutableIdentity::SuspectMismatch;
            }
        }

        // If signed by a completely unexpected company for a known branded tool
        if (kind == AgentKind::Ollama && !pub_lower.contains("ollama"))
            || (kind == AgentKind::Cursor && !pub_lower.contains("anysphere"))
            || (kind == AgentKind::Claude && !pub_lower.contains("anthropic"))
            || (kind == AgentKind::Copilot && !pub_lower.contains("microsoft"))
            || (kind == AgentKind::GeminiCli && !pub_lower.contains("google"))
            || (kind == AgentKind::Codex && !pub_lower.contains("openai"))
        {
            return ExecutableIdentity::SuspectMismatch;
        }
    } else if sig.status == crate::signature::SignatureStatus::Invalid {
        return ExecutableIdentity::SuspectMismatch;
    }

    // Binaries running from temporary/scratch storage are never verified
    if is_temp_or_scratch {
        return ExecutableIdentity::Unverified;
    }

    // A resolved executable path alone MUST NOT be treated as proof of genuine tool identity.
    // Without a verified Authenticode signature from the expected publisher,
    // unsigned or unverified binaries remain Unverified even if located in a canonical directory.
    ExecutableIdentity::Unverified
}

/// Derive the identity state from a resolved path (pure helper defaulting to OpenCode kind).
pub fn identity_for(path: &str) -> ExecutableIdentity {
    verify_process_identity(AgentKind::OpenCode, path)
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
    // SAFETY: CreateToolhelp32Snapshot creates a read-only snapshot of running processes.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Ok(snapshot) = snapshot else {
        return results;
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        // SAFETY: zeroing memory for PROCESSENTRY32W struct is standard Win32 pattern.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: Process32FirstW reads into our stack-allocated entry.
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        // SAFETY: Closing open snapshot handle.
        let _ = unsafe { CloseHandle(snapshot) };
        return results;
    }

    loop {
        let pid = entry.th32ProcessID;
        let parent_pid = if entry.th32ParentProcessID != 0 {
            Some(entry.th32ParentProcessID)
        } else {
            None
        };
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
            let mut access_denied = false;
            // SAFETY: OpenProcess with PROCESS_QUERY_LIMITED_INFORMATION is safe and read-only.
            unsafe {
                match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    Ok(handle) => {
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
                    Err(_) => {
                        access_denied = true;
                    }
                }
            }

            let category = kind.category();
            let is_model_server = kind.is_model_server();
            let identity = if access_denied {
                ExecutableIdentity::AccessDenied
            } else {
                verify_process_identity(kind, &exe_path)
            };
            let evidence = match identity {
                ExecutableIdentity::Verified => format!(
                    "Process PID {} named '{}' matches verified image pattern and digital signature for {}.",
                    pid,
                    name,
                    kind.label()
                ),
                ExecutableIdentity::Unverified => format!(
                    "Process PID {} named '{}' matches image pattern for {}, but tool identity is unconfirmed (unsigned or unverified binary at {}).",
                    pid,
                    name,
                    kind.label(),
                    if exe_path.is_empty() {
                        "unavailable"
                    } else {
                        &exe_path
                    }
                ),
                ExecutableIdentity::SuspectMismatch => format!(
                    "Process PID {} named '{}' has contradictory signer metadata (renamed binary suspected).",
                    pid, name
                ),
                ExecutableIdentity::AccessDenied => format!(
                    "Process PID {} named '{}' matches image pattern for {}, but process query access was denied.",
                    pid,
                    name,
                    kind.label()
                ),
                ExecutableIdentity::ImageNameOnly => format!(
                    "Process PID {} named '{}' matches image pattern for {}, but full path could not be resolved.",
                    pid,
                    name,
                    kind.label()
                ),
                ExecutableIdentity::PathMissing => format!(
                    "Process PID {} named '{}' previously referenced path '{}' which is no longer on disk.",
                    pid, name, exe_path
                ),
            };
            let confidence = identity.confidence_label().to_string();
            let limitations = if is_model_server {
                "Model server process only; indicates available local inference, not active autonomous execution."
            } else if identity == ExecutableIdentity::SuspectMismatch {
                "Binary metadata conflicts with claimed tool identity. Do NOT treat as genuine AI agent."
            } else if identity == ExecutableIdentity::Unverified {
                "Unverified executable identity. An executable was renamed, unsigned, or launched from an unverified path. Presence of this process does not prove it is the genuine AI tool."
            } else if identity == ExecutableIdentity::AccessDenied {
                "Process query access denied. Cannot verify executable path or Authenticode signature."
            } else {
                "Running process does not prove active generation, task execution, or session history."
            }
            .to_string();

            results.push(DetectedAgentProcess {
                kind,
                category,
                pid,
                parent_pid,
                name,
                identity,
                exe_path,
                command_line: None,
                started_unix,
                evidence,
                confidence,
                limitations,
                is_model_server,
            });
        }

        // SAFETY: Process32NextW safely iterates through the snapshot.
        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    // SAFETY: Closing open snapshot handle.
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
    fn recognises_documented_executable_names() {
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
            Some(AgentKind::Codex)
        );
        assert_eq!(
            AgentKind::from_executable_name("codex-tui"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            AgentKind::from_executable_name("claude.exe"),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            AgentKind::from_executable_name("claude-code"),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            AgentKind::from_executable_name("aider.exe"),
            Some(AgentKind::Aider)
        );
        assert_eq!(
            AgentKind::from_executable_name("ollama.exe"),
            Some(AgentKind::Ollama)
        );
        assert_eq!(
            AgentKind::from_executable_name("ollama_app.exe"),
            Some(AgentKind::Ollama)
        );
        assert_eq!(
            AgentKind::from_executable_name("cursor.exe"),
            Some(AgentKind::Cursor)
        );
        assert_eq!(
            AgentKind::from_executable_name("cline.exe"),
            Some(AgentKind::Cline)
        );
        assert_eq!(
            AgentKind::from_executable_name("continue.exe"),
            Some(AgentKind::Continue)
        );
        assert_eq!(
            AgentKind::from_executable_name("gemini-cli.exe"),
            Some(AgentKind::GeminiCli)
        );

        // Negative tests: false positives must NOT match
        assert_eq!(AgentKind::from_executable_name("node.exe"), None);
        assert_eq!(AgentKind::from_executable_name("evilclaude.exe"), None);
        assert_eq!(AgentKind::from_executable_name("notopencode.exe"), None);
        assert_eq!(AgentKind::from_executable_name("codex-helper.exe"), None);
    }

    #[test]
    fn distinguishes_model_servers_from_agents() {
        assert!(AgentKind::Ollama.is_model_server());
        assert_eq!(AgentKind::Ollama.category(), AgentCategory::ModelServer);

        assert!(!AgentKind::Claude.is_model_server());
        assert_eq!(AgentKind::Claude.category(), AgentCategory::Agent);

        assert!(!AgentKind::OpenCode.is_model_server());
        assert_eq!(AgentKind::OpenCode.category(), AgentCategory::Agent);

        assert!(!AgentKind::Aider.is_model_server());
        assert_eq!(AgentKind::Aider.category(), AgentCategory::Agent);
    }

    #[test]
    fn mask_command_line_redacts_credentials() {
        let input = "opencode --api-key sk-proj-1234567890abcdef --port 8080";
        let masked = mask_command_line(input);
        assert!(!masked.contains("sk-proj-1234567890abcdef"));
        assert!(masked.contains("***"));
        assert!(masked.contains("--port 8080"));

        let env_input = "claude OPENAI_API_KEY=sk-12345678901234567890 --auth-token secret12345";
        let env_masked = mask_command_line(env_input);
        assert!(!env_masked.contains("sk-12345678901234567890"));
        assert!(!env_masked.contains("secret12345"));
        assert!(env_masked.contains("OPENAI_API_KEY=***"));

        let header_input = "curl -H 'Authorization: Bearer my_top_secret_token_12345'";
        let header_masked = mask_command_line(header_input);
        assert!(!header_masked.contains("my_top_secret_token_12345"));
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
    fn identity_confidence_labels_are_accurate() {
        assert_eq!(
            ExecutableIdentity::Verified.confidence_label(),
            "Observed (Verified tool identity)"
        );
        assert_eq!(
            ExecutableIdentity::Unverified.confidence_label(),
            "Unverified (Image name match; tool identity unconfirmed)"
        );
        assert_eq!(
            ExecutableIdentity::SuspectMismatch.confidence_label(),
            "Unverified (Metadata mismatch; likely renamed binary)"
        );
        assert_eq!(
            ExecutableIdentity::AccessDenied.confidence_label(),
            "Unknown (Process query access denied)"
        );
        assert_eq!(
            ExecutableIdentity::ImageNameOnly.confidence_label(),
            "Unknown (Process path unavailable)"
        );
        assert_eq!(
            ExecutableIdentity::PathMissing.confidence_label(),
            "Unknown (Target image missing from disk)"
        );
    }

    #[test]
    fn scratch_and_temp_paths_are_unverified() {
        let temp_dir = std::env::temp_dir();
        let dummy = temp_dir.join("test_scratch_opencode.exe");
        let _ = std::fs::write(&dummy, b"dummy binary bytes");
        let identity = verify_process_identity(AgentKind::OpenCode, &dummy.to_string_lossy());
        let _ = std::fs::remove_file(&dummy);
        assert_eq!(identity, ExecutableIdentity::Unverified);
    }

    #[test]
    fn canonical_path_alone_does_not_confer_verified() {
        // A resolved executable path alone must NOT be treated as proof of genuine tool identity.
        let temp_dir = std::env::temp_dir();
        let pf_dir = temp_dir.join("Program Files").join("OpenCode");
        let _ = std::fs::create_dir_all(&pf_dir);
        let dummy = pf_dir.join("opencode.exe");
        let _ = std::fs::write(&dummy, b"unsigned binary in canonical-like path");
        let identity = verify_process_identity(AgentKind::OpenCode, &dummy.to_string_lossy());
        let _ = std::fs::remove_file(&dummy);
        let _ = std::fs::remove_dir_all(&pf_dir);
        assert_eq!(identity, ExecutableIdentity::Unverified);
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(AgentKind::OpenCode.id(), "opencode");
        assert_eq!(AgentKind::Codex.id(), "codex");
        assert_eq!(AgentKind::Claude.id(), "claude");
        assert_eq!(AgentKind::Aider.id(), "aider");
        assert_eq!(AgentKind::Ollama.id(), "ollama");
        assert_eq!(AgentKind::OpenCode.label(), "OpenCode");
        assert_eq!(AgentKind::Claude.label(), "Claude Code");
        assert_eq!(AgentKind::Ollama.label(), "Ollama");
    }

    fn find_installed_codex_binary() -> Option<std::path::PathBuf> {
        if let Some(explicit) = std::env::var_os("CURE_TEST_CODEX_EXE") {
            let p = std::path::PathBuf::from(explicit);
            if p.is_file() {
                return Some(p);
            }
        }
        #[cfg(windows)]
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let base = std::path::Path::new(&local_app_data)
                .join("OpenAI")
                .join("Codex")
                .join("bin");
            if let Ok(entries) = std::fs::read_dir(base) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("codex.exe");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn real_codex_binary_verifies_as_genuine() {
        if let Some(path) = find_installed_codex_binary() {
            let sig = crate::signature::signature_detail(&path);
            assert_eq!(sig.status, crate::signature::SignatureStatus::ValidSigned);
            assert!(
                sig.publisher
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("openai"),
                "expected OpenAI publisher, got {:?}",
                sig.publisher
            );
            let ident = verify_process_identity(AgentKind::Codex, &path.to_string_lossy());
            assert_eq!(ident, ExecutableIdentity::Verified);
        }
    }
}
