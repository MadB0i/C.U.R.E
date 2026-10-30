//! AI coding-agent visibility, flight recorder, change radar and case files.
//!
//! * [`detection`] recognises supported agent processes and local model servers.
//! * [`recorder`] records metadata-only workspace changes during an
//!   explicitly started session.
//! * [`windows`] compares the persistence evidence C.U.R.E already collects.
//! * [`casefile`] builds privacy-safe shareable summaries (JSON, HTML, Markdown).
//!
//! The separation is deliberate: a running process is not activity, an
//! observed file change is not attribution, and a new startup entry is not
//! malware.

pub mod casefile;
pub mod detection;
pub mod recorder;
pub mod windows;

pub use casefile::AgentCaseFile;
pub use detection::{
    enumerate_agent_processes, mask_command_line, AgentCategory, AgentKind, DetectedAgentProcess,
    ExecutableIdentity,
};
pub use recorder::{
    capture_workspace, now_unix, ChangeKind, EvidenceConfidence, FlightCoverage, FlightEvent,
    FlightSession, ObservationSource, WorkspaceSnapshot,
};
pub use windows::{
    compare as compare_windows, load as load_windows_snapshot, save as save_windows_snapshot,
    WindowsChangeItem, WindowsChangeStatus, WindowsChanges, WindowsSnapshot,
};
