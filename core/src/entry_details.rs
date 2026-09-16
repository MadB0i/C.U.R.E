//! On-demand forensic details for a single finding.
//!
//! Strictly read-only enrichment for display: shortcut resolution, task
//! metadata, publisher/signature state. Callers resolve ids through a
//! FRESH scan first (`find_current_entry` / scan lookup), so this module
//! never becomes an arbitrary-path oracle — it only enriches entries the
//! scanner actually reported.
//!
//! Nothing here executes anything: shortcut targets are inspected, task
//! actions are parsed, binaries are verified — and every byte stays data.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::lnk::LnkInfo;
use crate::model::{PersistenceEntry, PersistenceSource};
use crate::scanners::scheduled_tasks::TaskDetails;

/// Enrichment for one finding, by source shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDetails {
    /// VALID / INVALID / UNSIGNED / UNKNOWN for the entry's resolved
    /// executable, or UNKNOWN when nothing resolves.
    pub signature: String,
    /// Signer display name when extractable. `None` is normal for
    /// unsigned/unverifiable targets and says nothing about intent.
    pub publisher: Option<String>,
    /// Shortcut resolution (Startup `.lnk` entries only).
    pub shortcut: Option<ShortcutDetails>,
    /// Structured task metadata (scheduled-task entries only).
    pub task: Option<TaskDetails>,
}

/// Resolved shortcut: target and liveness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutDetails {
    pub info: LnkInfo,
    /// The resolved target, with `%VAR%` segments expanded on Windows.
    pub expanded_target: Option<String>,
    pub target_exists: bool,
}

pub fn for_entry(entry: &PersistenceEntry) -> EntryDetails {
    // Signature/publisher for whatever the command resolves to (any source).
    let exe = crate::signature::resolve_executable_path(&entry.command);
    let (signature, publisher) = match exe.as_deref() {
        Some(path) => {
            let detail = crate::signature::signature_detail(path);
            (detail.status.label().to_string(), detail.publisher)
        }
        None => ("UNKNOWN".to_string(), None),
    };
    let shortcut = match entry.source {
        PersistenceSource::StartupFolder if is_shortcut_name(&entry.name) => {
            crate::lnk::analyze(Path::new(&entry.location))
                .map(|info| ShortcutDetails::for_info(&info))
        }
        _ => None,
    };
    let task = match entry.source {
        PersistenceSource::ScheduledTask => {
            let xml = read_task_file(Path::new(&entry.location));
            if xml.is_empty() {
                None
            } else {
                Some(crate::scanners::scheduled_tasks::parse_details(&xml))
            }
        }
        _ => None,
    };
    EntryDetails { signature, publisher, shortcut, task }
}

fn is_shortcut_name(name: &str) -> bool {
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".lnk")
}

fn read_task_file(path: &Path) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    if bytes.len() > 4 * 1024 * 1024 {
        return String::new();
    }
    // Same BOM handling as the task scanner (UTF-16 task files are normal).
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => {
            String::from_utf16_lossy(&rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
        }
        [0xFE, 0xFF, rest @ ..] => {
            String::from_utf16_lossy(&rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect::<Vec<_>>())
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    }
}

impl ShortcutDetails {
    pub fn for_info(info: &LnkInfo) -> Self {
        let expanded_target = info.target.as_deref().map(|t| {
            crate::scanners::services::expand_env_vars(t)
        });
        let target_exists = expanded_target
            .as_deref()
            .map(|t| Path::new(t).is_file())
            .unwrap_or(false);
        ShortcutDetails {
            info: info.clone(),
            expanded_target,
            target_exists,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use crate::model::PersistenceSource;

    #[test]
    fn shortcut_name_matching_is_case_insensitive() {
        assert!(is_shortcut_name("updater.lnk"));
        assert!(is_shortcut_name("UPDATER.LNK"));
        assert!(!is_shortcut_name("updater.exe"));
        assert!(!is_shortcut_name("lnk"));
        assert!(!is_shortcut_name(""));
    }

    #[test]
    fn shortcut_entry_resolves_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updater.lnk");
        let target = dir.path().join("CURE-SYNTH-target.exe");
        std::fs::write(&target, b"MZ").unwrap();
        let target_text = target.to_string_lossy().into_owned();
        std::fs::write(
            &path,
            fixtures::minimal_lnk_unicode(&target_text, "--silent", ""),
        )
        .unwrap();
        let entry = PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            "updater.lnk",
            path.to_string_lossy().as_ref(),
            path.to_string_lossy().as_ref(),
        );
        let details = for_entry(&entry);
        let shortcut = details.shortcut.expect("shortcut details");
        assert_eq!(shortcut.expanded_target.as_deref(), Some(target_text.as_str()));
        assert!(shortcut.target_exists);
        // The entry command is the .lnk path itself: signature reflects the
        // shortcut file (unsigned fixture), publisher stays empty.
        assert_eq!(details.publisher, None);
    }

    #[test]
    fn missing_shortcut_target_reports_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ghost.lnk");
        let ghost = dir.path().join("CURE-SYNTH-ghost.exe");
        std::fs::write(
            &path,
            fixtures::minimal_lnk_unicode(&ghost.to_string_lossy(), "", ""),
        )
        .unwrap();
        let entry = PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            "ghost.lnk",
            path.to_string_lossy().as_ref(),
            path.to_string_lossy().as_ref(),
        );
        let details = for_entry(&entry);
        let shortcut = details.shortcut.expect("shortcut details");
        assert!(!shortcut.target_exists);
        assert_eq!(details.publisher, None);
    }

    #[test]
    fn task_entry_parses_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.xml");
        std::fs::write(&path, fixtures::multi_action_task_xml()).unwrap();
        let entry = PersistenceEntry::new(
            PersistenceSource::ScheduledTask,
            "CURE-SYNTH\\Task",
            "powershell.exe",
            path.to_string_lossy().as_ref(),
        );
        let details = for_entry(&entry);
        let task = details.task.expect("task details");
        assert_eq!(task.actions.len(), 2);
        assert_eq!(task.run_level, "HighestAvailable");
    }

    #[test]
    fn other_sources_carry_signature_only() {
        let entry = PersistenceEntry::new(
            PersistenceSource::RegistryRun,
            "CURE-SYNTH",
            r"C:\cure-synth\x.exe",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        );
        let details = for_entry(&entry);
        assert_eq!(details.shortcut, None);
        assert_eq!(details.task, None);
        // Unresolvable fixture path: UNKNOWN, no publisher.
        assert_eq!(details.signature, "UNKNOWN");
        assert_eq!(details.publisher, None);
    }
}
