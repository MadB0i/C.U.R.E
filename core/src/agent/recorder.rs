//! Agent Flight Recorder: bounded workspace snapshots, observed filesystem
//! changes and an explicit evidence-confidence model.
//!
//! The recorder never reads file contents. It records metadata only: a
//! workspace-relative path, logical size and last-write time. Paths stay on
//! this machine; the shareable case-file model in [`super::casefile`] has no
//! field for one.
//!
//! A filesystem watcher does not identify the process that wrote a file.
//! This module therefore never emits `Attributed`. A change made while a
//! supported agent process was present is `Correlated`, and the note says
//! exactly that. Everything else is `Observed` or `Unknown`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::detection::AgentKind;

/// Hard cap on baseline entries. A workspace larger than this is recorded as
/// truncated rather than silently sampled.
pub const MAX_BASELINE_FILES: usize = 50_000;
/// Hard cap on a recorded session. Once reached, further changes are counted
/// as dropped, not invented or overwritten.
pub const MAX_FLIGHT_EVENTS: usize = 5_000;
/// Repeated notifications for one path inside this window update the same
/// event instead of producing a burst of near-duplicates.
pub const DEDUPE_WINDOW_SECS: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    /// Workspace-relative path with forward slashes.
    pub path: String,
    pub size: u64,
    pub modified_unix: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub captured_unix: u64,
    pub files: Vec<FileSnapshot>,
    pub truncated: bool,
    pub unreadable_dirs: u64,
    pub skipped_links: u64,
    pub max_entries: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
    Renamed,
}

impl ChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Created => "Created",
            ChangeKind::Modified => "Modified",
            ChangeKind::Removed => "Removed",
            ChangeKind::Renamed => "Renamed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidence {
    /// The change itself was observed. No supported agent process was present
    /// during the observation window.
    Observed,
    /// The change was observed while a supported agent process was present.
    /// Presence is not proof of causation.
    Correlated,
    /// Observation coverage was incomplete, so the event may be missing
    /// context or may itself be an artefact of reconciliation.
    Unknown,
}

impl EvidenceConfidence {
    pub fn label(self) -> &'static str {
        match self {
            EvidenceConfidence::Observed => "Observed",
            EvidenceConfidence::Correlated => "Correlated",
            EvidenceConfidence::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    BaselineSnapshot,
    DirectoryWatcher,
    SnapshotReconciliation,
}

impl ObservationSource {
    pub fn label(self) -> &'static str {
        match self {
            ObservationSource::BaselineSnapshot => "baseline snapshot",
            ObservationSource::DirectoryWatcher => "directory watcher",
            ObservationSource::SnapshotReconciliation => "snapshot reconciliation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightEvent {
    pub seq: u64,
    pub at_unix: u64,
    pub kind: ChangeKind,
    /// Workspace-relative path. Empty for an overflow marker.
    pub path: String,
    pub from_path: Option<String>,
    pub size_before: Option<u64>,
    pub size_after: Option<u64>,
    pub source: ObservationSource,
    pub confidence: EvidenceConfidence,
    pub agents: Vec<AgentKind>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightCoverage {
    pub baseline_captured: bool,
    pub baseline_truncated: bool,
    pub baseline_files: usize,
    pub unreadable_dirs: u64,
    pub skipped_links: u64,
    pub watcher_started: bool,
    pub overflow_batches: u64,
    pub dropped_events: u64,
    pub snapshot_reconciled: bool,
    pub complete: bool,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightSession {
    pub id: String,
    pub workspace: String,
    pub started_unix: u64,
    pub ended_unix: Option<u64>,
    pub active: bool,
    pub files_at_start: usize,
    pub events: Vec<FlightEvent>,
    pub agents_seen: Vec<AgentKind>,
    pub coverage: FlightCoverage,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightCounts {
    pub total: u64,
    pub created: u64,
    pub modified: u64,
    pub removed: u64,
    pub renamed: u64,
    pub observed: u64,
    pub correlated: u64,
    pub unknown: u64,
    pub bytes_added: u64,
    pub bytes_removed: u64,
}

impl FlightCounts {
    pub fn net_bytes(&self) -> i64 {
        self.bytes_added as i64 - self.bytes_removed as i64
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn modified_unix(meta: &fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let text = rel.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Capture a metadata-only baseline. Never follows links, never reads file
/// contents, and reports truncation and unreadable directories explicitly.
pub fn capture_workspace(root: &Path, max_entries: usize) -> io::Result<WorkspaceSnapshot> {
    let meta = fs::metadata(root)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the selected workspace is not a directory",
        ));
    }

    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut unreadable_dirs = 0u64;
    let mut skipped_links = 0u64;
    let mut truncated = false;

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                unreadable_dirs += 1;
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                unreadable_dirs += 1;
                continue;
            };
            let path = entry.path();
            let Ok(link_meta) = fs::symlink_metadata(&path) else {
                unreadable_dirs += 1;
                continue;
            };
            let file_type = link_meta.file_type();
            if file_type.is_symlink() {
                skipped_links += 1;
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else {
                unreadable_dirs += 1;
                continue;
            };
            if !meta.is_file() {
                skipped_links += 1;
                continue;
            }
            let Some(path) = relative_path(root, &path) else {
                continue;
            };
            if files.len() >= max_entries {
                truncated = true;
                break;
            }
            files.push(FileSnapshot {
                path,
                size: meta.len(),
                modified_unix: modified_unix(&meta),
            });
        }
        if truncated {
            break;
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(WorkspaceSnapshot {
        captured_unix: now_unix(),
        files,
        truncated,
        unreadable_dirs,
        skipped_links,
        max_entries,
    })
}

fn confidence_for(agents: &[AgentKind], unknown_context: bool) -> EvidenceConfidence {
    if unknown_context {
        EvidenceConfidence::Unknown
    } else if agents.is_empty() {
        EvidenceConfidence::Observed
    } else {
        EvidenceConfidence::Correlated
    }
}

fn note_for(kind: ChangeKind, confidence: EvidenceConfidence, agents: &[AgentKind]) -> String {
    let who = if agents.is_empty() {
        String::new()
    } else {
        let names = agents
            .iter()
            .map(|a| a.label())
            .collect::<Vec<_>>()
            .join(", ");
        format!(" while {names} was present")
    };
    match (kind, confidence) {
        (_, EvidenceConfidence::Unknown) => {
            "Observation coverage was incomplete; context is not established.".to_string()
        }
        (ChangeKind::Created, EvidenceConfidence::Correlated) => {
            format!("File creation observed{who}. Correlation is not attribution.")
        }
        (ChangeKind::Modified, EvidenceConfidence::Correlated) => {
            format!("File metadata changed{who}. Correlation is not attribution.")
        }
        (ChangeKind::Removed, EvidenceConfidence::Correlated) => {
            format!("File removal observed{who}. Correlation is not attribution.")
        }
        (ChangeKind::Renamed, EvidenceConfidence::Correlated) => {
            format!("Rename pair observed{who}. Correlation is not attribution.")
        }
        (ChangeKind::Created, _) => {
            "File creation observed. No supported agent process was present.".to_string()
        }
        (ChangeKind::Modified, _) => {
            "File metadata changed. No supported agent process was present.".to_string()
        }
        (ChangeKind::Removed, _) => {
            "File removal observed. No supported agent process was present.".to_string()
        }
        (ChangeKind::Renamed, _) => {
            "Rename pair observed. No supported agent process was present.".to_string()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn make_event(
    kind: ChangeKind,
    path: String,
    from_path: Option<String>,
    size_before: Option<u64>,
    size_after: Option<u64>,
    at_unix: u64,
    source: ObservationSource,
    agents: &[AgentKind],
    unknown_context: bool,
) -> FlightEvent {
    let confidence = confidence_for(agents, unknown_context);
    FlightEvent {
        seq: 0,
        at_unix,
        kind,
        path,
        from_path,
        size_before,
        size_after,
        source,
        confidence,
        agents: agents.to_vec(),
        note: note_for(kind, confidence, agents),
    }
}

/// Compare two snapshots and return the metadata differences. The result is
/// deterministic and independent of directory enumeration order.
pub fn diff_snapshots(
    before: &WorkspaceSnapshot,
    after: &WorkspaceSnapshot,
    at_unix: u64,
    source: ObservationSource,
    agents: &[AgentKind],
    unknown_context: bool,
) -> Vec<FlightEvent> {
    let before_map: HashMap<&str, &FileSnapshot> =
        before.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let after_map: HashMap<&str, &FileSnapshot> =
        after.files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut events = Vec::new();
    for file in &after.files {
        match before_map.get(file.path.as_str()) {
            None => events.push(make_event(
                ChangeKind::Created,
                file.path.clone(),
                None,
                None,
                Some(file.size),
                at_unix,
                source,
                agents,
                unknown_context,
            )),
            Some(old) if old.size != file.size || old.modified_unix != file.modified_unix => events
                .push(make_event(
                    ChangeKind::Modified,
                    file.path.clone(),
                    None,
                    Some(old.size),
                    Some(file.size),
                    at_unix,
                    source,
                    agents,
                    unknown_context,
                )),
            Some(_) => {}
        }
    }
    for file in &before.files {
        if !after_map.contains_key(file.path.as_str()) {
            events.push(make_event(
                ChangeKind::Removed,
                file.path.clone(),
                None,
                Some(file.size),
                None,
                at_unix,
                source,
                agents,
                unknown_context,
            ));
        }
    }
    events.sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.label().cmp(b.kind.label())));
    events
}

impl FlightSession {
    pub fn new(
        id: impl Into<String>,
        workspace: impl Into<String>,
        baseline: &WorkspaceSnapshot,
        agents: Vec<AgentKind>,
        watcher_started: bool,
    ) -> Self {
        let note = if baseline.truncated {
            format!(
                "Baseline truncated at {} entries. Files beyond the cap are not represented.",
                baseline.max_entries
            )
        } else if baseline.unreadable_dirs > 0 {
            format!(
                "Baseline captured with {} unreadable director{}.",
                baseline.unreadable_dirs,
                if baseline.unreadable_dirs == 1 {
                    "y"
                } else {
                    "ies"
                }
            )
        } else {
            "Baseline captured. Watching begins from this snapshot.".to_string()
        };
        Self {
            id: id.into(),
            workspace: workspace.into(),
            started_unix: now_unix(),
            ended_unix: None,
            active: true,
            files_at_start: baseline.files.len(),
            events: Vec::new(),
            agents_seen: agents,
            coverage: FlightCoverage {
                baseline_captured: true,
                baseline_truncated: baseline.truncated,
                baseline_files: baseline.files.len(),
                unreadable_dirs: baseline.unreadable_dirs,
                skipped_links: baseline.skipped_links,
                watcher_started,
                overflow_batches: 0,
                dropped_events: 0,
                snapshot_reconciled: false,
                complete: !baseline.truncated && baseline.unreadable_dirs == 0,
                note,
            },
        }
    }

    pub fn note_agents(&mut self, agents: &[AgentKind]) {
        for kind in agents {
            if !self.agents_seen.contains(kind) {
                self.agents_seen.push(*kind);
            }
        }
    }

    /// Close a session that was still recording when the application exited.
    /// Stop-time reconciliation did not run, so the coverage cannot be called
    /// complete and the note must say so rather than inventing a clean stop.
    pub fn recover_interrupted(mut self) -> Self {
        if self.active {
            self.active = false;
            self.ended_unix = Some(
                self.events
                    .iter()
                    .map(|event| event.at_unix)
                    .max()
                    .unwrap_or(self.started_unix),
            );
            self.coverage.complete = false;
            self.coverage.note =
                "Session was recovered after the application stopped. Stop-time reconciliation did not run; coverage is incomplete."
                    .to_string();
        }
        self
    }

    fn note_agent(&mut self, agents: &[AgentKind]) {
        self.note_agents(agents);
    }

    /// Record a watcher observation. Returns `false` when the event budget is
    /// exhausted; the caller may surface that as a coverage limitation.
    pub fn record_event(&mut self, mut event: FlightEvent) -> bool {
        self.note_agent(&event.agents);
        if !self.coverage.complete
            && event.confidence != EvidenceConfidence::Correlated
            && event.confidence != EvidenceConfidence::Unknown
        {
            event.confidence = EvidenceConfidence::Unknown;
            event.note = note_for(event.kind, event.confidence, &event.agents);
        }

        if let Some(last) = self.events.iter().rposition(|e| e.path == event.path) {
            let previous = &mut self.events[last];
            let close = event.at_unix.saturating_sub(previous.at_unix) <= DEDUPE_WINDOW_SECS;
            if close {
                let replace = (previous.kind == ChangeKind::Modified
                    && event.kind == ChangeKind::Modified)
                    || (previous.kind == ChangeKind::Created && event.kind == ChangeKind::Modified)
                    || (previous.kind == ChangeKind::Removed && event.kind == ChangeKind::Removed);
                if replace {
                    previous.at_unix = event.at_unix;
                    previous.size_after = event.size_after;
                    previous.kind = if previous.kind == ChangeKind::Created {
                        ChangeKind::Created
                    } else {
                        event.kind
                    };
                    previous.source = event.source;
                    previous.confidence = event.confidence;
                    previous.agents = event.agents;
                    previous.note = note_for(previous.kind, previous.confidence, &previous.agents);
                    return true;
                }
            }
        }

        if self.events.len() >= MAX_FLIGHT_EVENTS {
            self.coverage.dropped_events += 1;
            self.coverage.complete = false;
            self.coverage.note = format!(
                "Event limit reached ({}). Later changes were not recorded.",
                MAX_FLIGHT_EVENTS
            );
            return false;
        }

        event.seq = self.events.len() as u64 + 1;
        self.events.push(event);
        true
    }

    pub fn record_rename(
        &mut self,
        from: String,
        to: String,
        size_before: Option<u64>,
        size_after: Option<u64>,
        at_unix: u64,
        agents: &[AgentKind],
    ) -> bool {
        let event = make_event(
            ChangeKind::Renamed,
            to,
            Some(from),
            size_before,
            size_after,
            at_unix,
            ObservationSource::DirectoryWatcher,
            agents,
            !self.coverage.complete,
        );
        self.record_event(event)
    }

    pub fn note_overflow(&mut self, count: u64) {
        self.coverage.overflow_batches += count;
        self.coverage.complete = false;
        self.coverage.note = format!(
            "{} watcher overflow batch(es). Some changes may be missing; reconciliation runs at stop.",
            self.coverage.overflow_batches
        );
    }

    /// Merge a final baseline-vs-final-state comparison into the session.
    /// This can recover changes the watcher missed, but it cannot recover
    /// files that were created and removed between snapshots.
    pub fn reconcile(&mut self, final_snapshot: &WorkspaceSnapshot, baseline: &WorkspaceSnapshot) {
        let events = diff_snapshots(
            baseline,
            final_snapshot,
            final_snapshot.captured_unix,
            ObservationSource::SnapshotReconciliation,
            &[],
            !self.coverage.complete,
        );
        // Any path the watcher already reported is already in the event list.
        // Reconciliation exists to recover *missed* changes, not to replay a
        // second copy of changes that were observed live. A rename covers
        // both its old and new path.
        let observed_paths: std::collections::HashSet<String> = self
            .events
            .iter()
            .filter(|event| event.source != ObservationSource::SnapshotReconciliation)
            .flat_map(|event| {
                std::iter::once(event.path.clone()).chain(event.from_path.iter().cloned())
            })
            .collect();
        for event in events {
            if observed_paths.contains(&event.path) {
                continue;
            }
            self.record_event(event);
        }
        self.coverage.snapshot_reconciled = true;
        if final_snapshot.truncated {
            self.coverage.complete = false;
            self.coverage.note =
                "Final reconciliation was truncated; completeness cannot be claimed.".to_string();
        } else if final_snapshot.unreadable_dirs > 0 {
            self.coverage.complete = false;
            self.coverage.note = format!(
                "Final reconciliation could not read {} folder(s); completeness cannot be claimed.",
                final_snapshot.unreadable_dirs
            );
        }
    }

    pub fn finish(&mut self) {
        self.active = false;
        self.ended_unix = Some(now_unix());
    }

    pub fn counts(&self) -> FlightCounts {
        let mut counts = FlightCounts::default();
        for event in &self.events {
            counts.total += 1;
            match event.kind {
                ChangeKind::Created => {
                    counts.created += 1;
                    counts.bytes_added += event.size_after.unwrap_or(0);
                }
                ChangeKind::Modified => {
                    counts.modified += 1;
                    let before = event.size_before.unwrap_or(0);
                    let after = event.size_after.unwrap_or(0);
                    if after >= before {
                        counts.bytes_added += after - before;
                    } else {
                        counts.bytes_removed += before - after;
                    }
                }
                ChangeKind::Removed => {
                    counts.removed += 1;
                    counts.bytes_removed += event.size_before.unwrap_or(0);
                }
                ChangeKind::Renamed => {
                    counts.renamed += 1;
                }
            }
            match event.confidence {
                EvidenceConfidence::Observed => counts.observed += 1,
                EvidenceConfidence::Correlated => counts.correlated += 1,
                EvidenceConfidence::Unknown => counts.unknown += 1,
            }
        }
        counts
    }

    pub fn duration_secs(&self) -> u64 {
        self.ended_unix
            .unwrap_or_else(now_unix)
            .saturating_sub(self.started_unix)
    }
}

pub fn session_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-sessions")
}

/// Persist a session atomically. The file contains workspace-relative paths
/// and is local-only; it is never part of the case-file export.
pub fn save_session(data_dir: &Path, session: &FlightSession) -> io::Result<PathBuf> {
    let dir = session_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{}.json", session.id));
    let temp = dir.join(format!("{}.json.tmp", session.id));
    let json = serde_json::to_string_pretty(session)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(&temp, json)?;
    fs::rename(&temp, &target)?;
    Ok(target)
}

pub fn load_session(path: &Path) -> io::Result<FlightSession> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn snapshot(files: &[(&str, u64, u64)]) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            captured_unix: 100,
            files: files
                .iter()
                .map(|(path, size, modified)| FileSnapshot {
                    path: (*path).to_string(),
                    size: *size,
                    modified_unix: Some(*modified),
                })
                .collect(),
            truncated: false,
            unreadable_dirs: 0,
            skipped_links: 0,
            max_entries: MAX_BASELINE_FILES,
        }
    }

    #[test]
    fn capture_records_relative_paths_and_sizes() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), b"fn main() {}").unwrap();
        let snap = capture_workspace(dir.path(), 100).unwrap();
        assert_eq!(snap.files.len(), 1);
        assert_eq!(snap.files[0].path, "src/main.rs");
        assert_eq!(snap.files[0].size, 12);
        assert!(!snap.truncated);
    }

    #[test]
    fn capture_truncates_instead_of_sampling() {
        let dir = tempdir().unwrap();
        for i in 0..5 {
            fs::write(dir.path().join(format!("f{i}.txt")), b"x").unwrap();
        }
        let snap = capture_workspace(dir.path(), 2).unwrap();
        assert_eq!(snap.files.len(), 2);
        assert!(snap.truncated);
    }

    #[test]
    fn diff_reports_created_modified_and_removed() {
        let before = snapshot(&[("same", 1, 1), ("edit", 2, 2), ("gone", 3, 3)]);
        let after = snapshot(&[("same", 1, 1), ("edit", 4, 4), ("new", 5, 5)]);
        let events = diff_snapshots(
            &before,
            &after,
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        );
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .any(|e| e.kind == ChangeKind::Created && e.path == "new"));
        assert!(events
            .iter()
            .any(|e| e.kind == ChangeKind::Modified && e.path == "edit"));
        assert!(events
            .iter()
            .any(|e| e.kind == ChangeKind::Removed && e.path == "gone"));
    }

    #[test]
    fn changes_are_correlated_not_attributed() {
        let before = snapshot(&[]);
        let after = snapshot(&[("a.txt", 1, 1)]);
        let events = diff_snapshots(
            &before,
            &after,
            10,
            ObservationSource::DirectoryWatcher,
            &[AgentKind::OpenCode],
            false,
        );
        assert_eq!(events[0].confidence, EvidenceConfidence::Correlated);
        assert!(events[0].note.contains("not attribution"));
        assert_eq!(events[0].agents, vec![AgentKind::OpenCode]);
    }

    #[test]
    fn interrupted_session_is_recovered_as_incomplete() {
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        session.started_unix = 100;
        session.record_event(make_event(
            ChangeKind::Modified,
            "a.txt".into(),
            None,
            Some(1),
            Some(2),
            120,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));
        let recovered = session.recover_interrupted();
        assert!(!recovered.active);
        assert_eq!(recovered.ended_unix, Some(120));
        assert!(!recovered.coverage.complete);
        assert!(recovered.coverage.note.contains("Stop-time reconciliation"));

        let mut finished = FlightSession::new("s", "ws", &baseline, vec![], true);
        finished.active = false;
        finished.ended_unix = Some(150);
        finished.coverage.note = "Stopped cleanly.".into();
        let unchanged = finished.clone().recover_interrupted();
        assert_eq!(unchanged, finished);
    }

    #[test]
    fn repeated_modifications_dedupe() {
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        for at in [10, 11, 12] {
            let event = make_event(
                ChangeKind::Modified,
                "a.txt".into(),
                None,
                Some(1),
                Some(2),
                at,
                ObservationSource::DirectoryWatcher,
                &[],
                false,
            );
            assert!(session.record_event(event));
        }
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.events[0].at_unix, 12);
    }

    #[test]
    fn dedupe_keeps_confidence_consistent_with_the_merged_event() {
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        for (at, agents) in [(10, vec![AgentKind::OpenCode]), (11, vec![])] {
            session.record_event(make_event(
                ChangeKind::Modified,
                "a.txt".into(),
                None,
                Some(1),
                Some(2),
                at,
                ObservationSource::DirectoryWatcher,
                &agents,
                false,
            ));
        }
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.events[0].confidence, EvidenceConfidence::Observed);
        assert!(session.events[0].agents.is_empty());
        assert!(session.events[0].note.contains("No supported agent"));

        session.record_event(make_event(
            ChangeKind::Modified,
            "a.txt".into(),
            None,
            Some(1),
            Some(2),
            12,
            ObservationSource::DirectoryWatcher,
            &[AgentKind::CodexCli],
            false,
        ));
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.events[0].confidence, EvidenceConfidence::Correlated);
        assert_eq!(session.events[0].agents, vec![AgentKind::CodexCli]);
        assert!(session.events[0].note.contains("not attribution"));
    }

    #[test]
    fn merged_creation_keeps_a_note_that_matches_its_kind() {
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        session.record_event(make_event(
            ChangeKind::Created,
            "new.txt".into(),
            None,
            None,
            Some(1),
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));
        session.record_event(make_event(
            ChangeKind::Modified,
            "new.txt".into(),
            None,
            Some(1),
            Some(2),
            11,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.events[0].kind, ChangeKind::Created);
        assert!(session.events[0].note.starts_with("File creation observed"));
    }

    #[test]
    fn event_budget_is_bounded() {
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        for i in 0..MAX_FLIGHT_EVENTS + 10 {
            session.record_event(make_event(
                ChangeKind::Modified,
                format!("f{i}"),
                None,
                Some(0),
                Some(1),
                u64::from(i as u32) * 10,
                ObservationSource::DirectoryWatcher,
                &[],
                false,
            ));
        }
        assert_eq!(session.events.len(), MAX_FLIGHT_EVENTS);
        assert_eq!(session.coverage.dropped_events, 10);
        assert!(!session.coverage.complete);
    }

    #[test]
    fn truncation_marks_coverage_incomplete_and_events_unknown() {
        let mut baseline = snapshot(&[]);
        baseline.truncated = true;
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        let event = make_event(
            ChangeKind::Created,
            "a.txt".into(),
            None,
            None,
            Some(1),
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        );
        session.record_event(event);
        assert!(!session.coverage.complete);
        assert_eq!(session.events[0].confidence, EvidenceConfidence::Unknown);
    }

    #[test]
    fn counts_separate_observed_from_correlated() {
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        session.record_event(make_event(
            ChangeKind::Created,
            "a".into(),
            None,
            None,
            Some(10),
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));
        session.record_event(make_event(
            ChangeKind::Created,
            "b".into(),
            None,
            None,
            Some(20),
            10,
            ObservationSource::DirectoryWatcher,
            &[AgentKind::CodexCli],
            false,
        ));
        let counts = session.counts();
        assert_eq!(counts.observed, 1);
        assert_eq!(counts.correlated, 1);
        assert_eq!(counts.created, 2);
        assert_eq!(counts.net_bytes(), 30);
    }

    #[test]
    fn session_round_trips_on_disk() {
        let dir = tempdir().unwrap();
        let baseline = snapshot(&[]);
        let mut session = FlightSession::new("case1", "C:/work", &baseline, vec![], true);
        session.finish();
        let path = save_session(dir.path(), &session).unwrap();
        let loaded = load_session(&path).unwrap();
        assert_eq!(loaded, session);
    }

    #[test]
    fn reconcile_recovers_a_change_the_watcher_missed() {
        let baseline = snapshot(&[("old", 1, 1)]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        let final_snapshot = snapshot(&[("old", 1, 1), ("new", 2, 2)]);
        session.reconcile(&final_snapshot, &baseline);
        assert!(session
            .events
            .iter()
            .any(|e| e.path == "new" && e.source == ObservationSource::SnapshotReconciliation));
        assert!(session.coverage.snapshot_reconciled);
    }

    #[test]
    fn reconcile_marks_an_unreadable_final_snapshot_incomplete() {
        let baseline = snapshot(&[]);
        let mut final_snapshot = snapshot(&[]);
        final_snapshot.unreadable_dirs = 2;
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        session.reconcile(&final_snapshot, &baseline);
        assert!(!session.coverage.complete);
        assert!(session.coverage.note.contains("2 folder(s)"));
    }

    #[test]
    fn reconciliation_does_not_duplicate_watcher_events() {
        let baseline = snapshot(&[("keep.txt", 10, 1), ("old.txt", 11, 1), ("gone.txt", 11, 1)]);
        let mut session = FlightSession::new("s", "ws", &baseline, vec![], true);
        session.record_event(make_event(
            ChangeKind::Created,
            "new.txt".into(),
            None,
            None,
            Some(832),
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));
        session.record_event(make_event(
            ChangeKind::Modified,
            "keep.txt".into(),
            None,
            Some(10),
            Some(20),
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));
        session.record_rename(
            "old.txt".into(),
            "renamed.txt".into(),
            Some(11),
            Some(11),
            10,
            &[],
        );
        session.record_event(make_event(
            ChangeKind::Removed,
            "gone.txt".into(),
            None,
            Some(11),
            None,
            10,
            ObservationSource::DirectoryWatcher,
            &[],
            false,
        ));

        let final_snapshot = snapshot(&[
            ("keep.txt", 20, 2),
            ("new.txt", 832, 2),
            ("renamed.txt", 11, 2),
        ]);
        session.reconcile(&final_snapshot, &baseline);

        assert_eq!(session.events.len(), 4);
        let counts = session.counts();
        assert_eq!(counts.created, 1);
        assert_eq!(counts.modified, 1);
        assert_eq!(counts.removed, 1);
        assert_eq!(counts.renamed, 1);
        assert_eq!(counts.net_bytes(), 832 + 10 - 11);
    }
}
