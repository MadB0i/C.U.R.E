//! Before/after comparison of the persistence evidence C.U.R.E already
//! collects. This is not a second security engine: the caller supplies the
//! same [`PersistenceEntry`] list the Rescue scan produces, and this module
//! only stores a trusted snapshot and compares two of them.
//!
//! Read-only by construction. There is no remediation path here, and a new
//! startup entry is never labelled malicious.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{PersistenceEntry, PersistenceSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsSnapshot {
    pub saved_at: DateTime<Utc>,
    pub entries: Vec<PersistenceEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsChangeStatus {
    Added,
    Removed,
    Changed,
}

impl WindowsChangeStatus {
    pub fn label(self) -> &'static str {
        match self {
            WindowsChangeStatus::Added => "Added",
            WindowsChangeStatus::Removed => "Removed",
            WindowsChangeStatus::Changed => "Changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsChangeItem {
    pub status: WindowsChangeStatus,
    pub source: PersistenceSource,
    pub source_label: String,
    pub name: String,
    pub location_before: Option<String>,
    pub location_after: Option<String>,
    pub command_before: Option<String>,
    pub command_after: Option<String>,
    pub review_hints: Vec<String>,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsChanges {
    pub compared_unix: u64,
    pub snapshot_saved_at_unix: Option<u64>,
    pub entries_before: usize,
    pub entries_after: usize,
    pub added: Vec<WindowsChangeItem>,
    pub removed: Vec<WindowsChangeItem>,
    pub changed: Vec<WindowsChangeItem>,
    pub complete: bool,
    pub note: String,
}

impl WindowsChanges {
    pub fn total(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

fn source_label(source: PersistenceSource) -> &'static str {
    match source {
        PersistenceSource::RegistryRun => "Registry Run key",
        PersistenceSource::StartupFolder => "Startup folder",
        PersistenceSource::ScheduledTask => "Scheduled task",
        PersistenceSource::WindowsService => "Windows service",
        PersistenceSource::WmiSubscription => "WMI subscription",
        PersistenceSource::IfeoDebugger => "IFEO debugger",
        PersistenceSource::AppInitDlls => "AppInit DLL",
        PersistenceSource::ComHijack => "COM registration",
    }
}

fn user_writable(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    [
        "/users/",
        "/appdata/",
        "/temp/",
        "/downloads/",
        "/public/",
        "/programdata/",
    ]
    .iter()
    .any(|token| lower.contains(token))
}

fn hints(entry: &PersistenceEntry) -> Vec<String> {
    let mut hints = Vec::new();
    if user_writable(&entry.command) || user_writable(&entry.location) {
        hints.push(
            "The command or location is in a user-writable folder. This is context, not a verdict."
                .to_string(),
        );
    }
    if entry.command.trim().is_empty() {
        hints.push(
            "No command was reported for this entry; verify it manually before acting.".to_string(),
        );
    }
    hints
}

fn key(entry: &PersistenceEntry) -> (String, String) {
    (
        entry.source.tag().to_string(),
        entry.name.trim().to_ascii_lowercase(),
    )
}

pub fn save(path: &Path, entries: &[PersistenceEntry]) -> io::Result<WindowsSnapshot> {
    let snapshot = WindowsSnapshot {
        saved_at: Utc::now(),
        entries: entries.to_vec(),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(&snapshot)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, json)?;
    Ok(snapshot)
}

pub fn load(path: &Path) -> io::Result<WindowsSnapshot> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

/// Compare the current evidence with a trusted snapshot.
///
/// A changed entry means the same source and display name now has a
/// different command or location. It does not mean the change was malicious,
/// and it never claims which process caused it.
pub fn compare(before: Option<&WindowsSnapshot>, current: &[PersistenceEntry]) -> WindowsChanges {
    let mut current_map: BTreeMap<(String, String), &PersistenceEntry> = BTreeMap::new();
    for entry in current {
        current_map.insert(key(entry), entry);
    }

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    if let Some(snapshot) = before {
        let mut before_map: BTreeMap<(String, String), &PersistenceEntry> = BTreeMap::new();
        for entry in &snapshot.entries {
            before_map.insert(key(entry), entry);
        }

        for (entry_key, after) in &current_map {
            match before_map.get(entry_key) {
                None => added.push(WindowsChangeItem {
                    status: WindowsChangeStatus::Added,
                    source: after.source,
                    source_label: source_label(after.source).to_string(),
                    name: after.name.clone(),
                    location_before: None,
                    location_after: Some(after.location.clone()),
                    command_before: None,
                    command_after: Some(after.command.clone()),
                    review_hints: hints(after),
                    evidence:
                        "Present in the current read-only scan, absent from the trusted snapshot."
                            .to_string(),
                }),
                Some(old) if old.command != after.command || old.location != after.location => {
                    let mut review = hints(after);
                    review.push(
                        "The command or location differs from the trusted snapshot. Compare both before acting."
                            .to_string(),
                    );
                    changed.push(WindowsChangeItem {
                        status: WindowsChangeStatus::Changed,
                        source: after.source,
                        source_label: source_label(after.source).to_string(),
                        name: after.name.clone(),
                        location_before: Some(old.location.clone()),
                        location_after: Some(after.location.clone()),
                        command_before: Some(old.command.clone()),
                        command_after: Some(after.command.clone()),
                        review_hints: review,
                        evidence: "Same source and name, different command or location."
                            .to_string(),
                    });
                }
                Some(_) => {}
            }
        }

        for (entry_key, old) in &before_map {
            if !current_map.contains_key(entry_key) {
                removed.push(WindowsChangeItem {
                    status: WindowsChangeStatus::Removed,
                    source: old.source,
                    source_label: source_label(old.source).to_string(),
                    name: old.name.clone(),
                    location_before: Some(old.location.clone()),
                    location_after: None,
                    command_before: Some(old.command.clone()),
                    command_after: None,
                    review_hints: vec![
                        "This entry was in the trusted snapshot and is no longer reported."
                            .to_string(),
                    ],
                    evidence:
                        "Absent from the current read-only scan; present in the trusted snapshot."
                            .to_string(),
                });
            }
        }
    } else {
        for entry in current {
            added.push(WindowsChangeItem {
                status: WindowsChangeStatus::Added,
                source: entry.source,
                source_label: source_label(entry.source).to_string(),
                name: entry.name.clone(),
                location_before: None,
                location_after: Some(entry.location.clone()),
                command_before: None,
                command_after: Some(entry.command.clone()),
                review_hints: hints(entry),
                evidence: "No trusted snapshot exists; this is the baseline inventory.".to_string(),
            });
        }
    }

    added.sort_by_key(|a| a.name.to_lowercase());
    removed.sort_by_key(|a| a.name.to_lowercase());
    changed.sort_by_key(|a| a.name.to_lowercase());

    let note = match before {
        None => "No trusted snapshot yet. Capture one to compare future changes.".to_string(),
        Some(_) if added.is_empty() && removed.is_empty() && changed.is_empty() => {
            "No supported persistence differences were observed.".to_string()
        }
        Some(_) => "Differences are evidence for review, not an assessment of intent.".to_string(),
    };

    WindowsChanges {
        compared_unix: Utc::now().timestamp().max(0) as u64,
        snapshot_saved_at_unix: before.map(|s| s.saved_at.timestamp().max(0) as u64),
        entries_before: before.map(|s| s.entries.len()).unwrap_or(0),
        entries_after: current.len(),
        added,
        removed,
        changed,
        complete: true,
        note,
    }
}

pub fn snapshot_path(data_dir: &Path) -> PathBuf {
    data_dir.join("windows-snapshot.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(name: &str, command: &str) -> PersistenceEntry {
        PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            name,
            command,
            r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
        )
    }

    #[test]
    fn snapshot_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("snap.json");
        let saved = save(&path, &[entry("a.bat", "a.bat")]).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.entries, saved.entries);
        assert_eq!(loaded.saved_at, saved.saved_at);
    }

    #[test]
    fn first_comparison_is_an_inventory_not_a_finding() {
        let changes = compare(None, &[entry("a.bat", "a.bat")]);
        assert_eq!(changes.added.len(), 1);
        assert!(changes.removed.is_empty());
        assert!(changes.note.contains("No trusted snapshot"));
        assert!(changes.added[0].evidence.contains("baseline inventory"));
    }

    #[test]
    fn detects_added_removed_and_changed() {
        let before = WindowsSnapshot {
            saved_at: Utc::now(),
            entries: vec![
                entry("keep.bat", "keep.bat"),
                entry("edit.bat", "old.exe"),
                entry("gone.bat", "gone.bat"),
            ],
        };
        let after = vec![
            entry("keep.bat", "keep.bat"),
            entry("edit.bat", "new.exe"),
            entry("new.bat", "new.bat"),
        ];
        let changes = compare(Some(&before), &after);
        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.added[0].name, "new.bat");
        assert_eq!(changes.removed.len(), 1);
        assert_eq!(changes.removed[0].name, "gone.bat");
        assert_eq!(changes.changed.len(), 1);
        assert_eq!(
            changes.changed[0].command_before.as_deref(),
            Some("old.exe")
        );
        assert_eq!(changes.changed[0].command_after.as_deref(), Some("new.exe"));
        assert_eq!(changes.total(), 3);
    }

    #[test]
    fn identical_snapshots_report_no_differences() {
        let before = WindowsSnapshot {
            saved_at: Utc::now(),
            entries: vec![entry("keep.bat", "keep.bat")],
        };
        let changes = compare(Some(&before), &[entry("keep.bat", "keep.bat")]);
        assert!(changes.is_empty());
        assert!(!changes.note.contains("malware"));
    }

    #[test]
    fn user_writable_path_is_context_only() {
        let changes = compare(None, &[entry("a.bat", r"C:\Users\me\Downloads\a.exe")]);
        assert!(changes.added[0]
            .review_hints
            .iter()
            .any(|h| h.contains("not a verdict")));
    }
}
