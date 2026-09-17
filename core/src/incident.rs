//! Post-login incident investigation (transient popup forensics).
//!
//! Correlates startup persistence findings with processes and windows
//! observed during a short, explicit observation window. Detection only:
//! nothing is installed, nothing is modified, nothing is executed.
//!
//! Methodology (deterministic — no "AI confidence"):
//!
//! - DIRECT: finding's executable path == observed executable path
//!   (normalized). "MATCHED PATH + PROCESS".
//! - STRONG: finding command and observed command line reference each
//!   other (executable substring either direction, or shared distinctive
//!   arguments ≥12 chars) with the launch inside the window.
//!   "MATCHED COMMAND/PATH + TIMING".
//! - PARTIAL: same executable file name, different paths (both shown).
//!   "MATCHED NAME ONLY".
//! - WEAK: same folder but different program, or a child of a correlated
//!   process. "TEMPORAL/STRUCTURAL PROXIMITY ONLY".
//! - NONE: no observed relationship.
//!
//! Verdicts: CAUSE IDENTIFIED (DIRECT + a transient window on the same
//! process), STRONG CORRELATION (DIRECT, or STRONG + same-process window),
//! REVIEW REQUIRED (anything weaker), NO DIRECT EVIDENCE (clean, usable
//! observation), INSUFFICIENT OBSERVATION (observation failed/unavailable).
//!
//! Correlation does not by itself establish malicious intent.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::elevation::SourceStatus;

// ---------------------------------------------------------------------------
// Types (all serializable — results cross the Tauri boundary and exports)
// ---------------------------------------------------------------------------

/// Minimal finding reference for correlation (from a fresh scan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupRef {
    pub id: String,
    pub source: String,
    pub name: String,
    pub command: String,
    pub location: String,
    /// Hosting PID when known (Windows services report their current
    /// process id). Lets service correlations require pid equality
    /// instead of matching every same-image instance.
    pub aux_pid: Option<u32>,
}

impl From<&crate::model::PersistenceEntry> for StartupRef {
    fn from(e: &crate::model::PersistenceEntry) -> Self {
        Self {
            id: e.id.clone(),
            source: e.source.to_string(),
            name: e.name.clone(),
            command: e.command.clone(),
            location: e.location.clone(),
            aux_pid: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedProcess {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub exe_path: String,
    /// Fetched post-hoc for correlated processes only (bounded).
    pub command_line: Option<String>,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub exited: bool,
    /// First noticed via WMI creation event (vs snapshot diff).
    pub via_events: bool,
    /// Already running when observation started (first poll). These are
    /// baseline context, not "created" events — the timeline skips them
    /// so a fresh observation isn't 250 simultaneous births.
    pub pre_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedWindow {
    pub pid: u32,
    pub title: String,
    pub class_name: String,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub closed: bool,
    /// Already open when observation started (first poll) — context,
    /// not an "opened" event.
    pub pre_existing: bool,
}

impl ObservedWindow {
    pub fn lifetime_ms(&self) -> u64 {
        self.last_seen_ms.saturating_sub(self.first_seen_ms)
    }
    /// Closed during observation after a brief life — the popup shape.
    /// (Short lifetime alone means nothing about intent.)
    pub fn is_transient(&self) -> bool {
        self.closed && self.lifetime_ms() < 5000
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorrelationLevel {
    Direct,
    Strong,
    Partial,
    Weak,
    None,
}

impl CorrelationLevel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Direct => "DIRECT",
            Self::Strong => "STRONG",
            Self::Partial => "PARTIAL",
            Self::Weak => "WEAK",
            Self::None => "NONE",
        }
    }
    fn rank(&self) -> u8 {
        match self {
            Self::Direct => 4,
            Self::Strong => 3,
            Self::Partial => 2,
            Self::Weak => 1,
            Self::None => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correlation {
    pub process_pid: u32,
    pub process_name: String,
    pub finding_id: String,
    pub finding_name: String,
    pub finding_source: String,
    pub level: CorrelationLevel,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineKind {
    ObservationStarted,
    ObservationEnded,
    ProcessCreated,
    ProcessExited,
    WindowOpened,
    WindowClosed,
    CorrelationNoted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub t_ms: u64,
    pub wall_time: String,
    pub kind: TimelineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentVerdict {
    CauseIdentified,
    StrongCorrelation,
    ReviewRequired,
    NoDirectEvidence,
    InsufficientObservation,
}

impl IncidentVerdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::CauseIdentified => "CAUSE IDENTIFIED",
            Self::StrongCorrelation => "STRONG CORRELATION",
            Self::ReviewRequired => "REVIEW REQUIRED",
            Self::NoDirectEvidence => "NO DIRECT EVIDENCE",
            Self::InsufficientObservation => "INSUFFICIENT OBSERVATION",
        }
    }
    pub fn explanation(&self) -> &'static str {
        match self {
            Self::CauseIdentified => "A startup entry's exact executable launched and showed a transient window on the same process. This identifies the popup's source — not its intent.",
            Self::StrongCorrelation => "A startup entry matches an observed launch by path or command. Consistent with that entry causing the activity — correlation is not proof of malicious intent.",
            Self::ReviewRequired => "Only weak or partial relationships were found. Investigate the listed evidence manually.",
            Self::NoDirectEvidence => "Observation completed and nothing links the observed activity to startup persistence.",
            Self::InsufficientObservation => "Observation could not run or produced no usable data. No conclusion is possible — this is not a clean bill of health.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationResult {
    pub investigation_id: String,
    pub started_at: String,
    pub duration_secs: u64,
    pub elevated: bool,
    pub process_observation: SourceStatus,
    pub window_observation: SourceStatus,
    pub processes: Vec<ObservedProcess>,
    pub windows: Vec<ObservedWindow>,
    pub correlations: Vec<Correlation>,
    pub timeline: Vec<TimelineEvent>,
    pub verdict: IncidentVerdict,
    pub truncated: bool,
}

/// Allowed observation lengths (seconds). Anything else is rejected.
pub const ALLOWED_DURATIONS: [u64; 4] = [15, 30, 60, 120];

pub fn validate_duration(secs: u64) -> Result<u64, String> {
    if ALLOWED_DURATIONS.contains(&secs) {
        Ok(secs)
    } else {
        Err(format!(
            "observation must be one of {:?} seconds (got {secs})",
            ALLOWED_DURATIONS
        ))
    }
}

pub fn new_investigation_id() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            format!("{secs}")
        })
        .unwrap_or_else(|_| "unknown-time".to_string());
    format!("INC-{stamp}-{}", std::process::id())
}

pub fn wall_clock(start: SystemTime, t_ms: u64) -> String {
    let t = start + Duration::from_millis(t_ms);
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ms = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_millis())
        .unwrap_or(0);
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// Minimal RFC-3339 UTC timestamp (`2026-09-16T12:34:56Z`) from first
/// principles — keeps chrono out of small dependents.
pub fn rfc3339(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let sod = (secs % 86_400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod / 60) % 60,
        sod % 60
    )
}

// ---------------------------------------------------------------------------
// Pure correlation
// ---------------------------------------------------------------------------

fn normalize_path(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .trim()
        .to_ascii_lowercase()
        .replace('\\', "/")
}

fn basename_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn parent_dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// The executable a finding points at (first command token, normalized).
/// Empty when the command carries no resolvable program.
pub fn finding_exe(command: &str) -> String {
    normalize_path(crate::risk::extract_program_path(command))
}

/// Correlate observed processes against startup findings. Pure and
/// deterministic: same inputs, same output. Capped at 256 correlations.
pub fn correlate(findings: &[StartupRef], processes: &[ObservedProcess]) -> Vec<Correlation> {
    let mut out = Vec::new();
    // pid → best level, for WEAK child-of-correlated matching.
    let mut correlated_pids: HashMap<u32, CorrelationLevel> = HashMap::new();

    // Pass 1: path/command/name relationships.
    for proc_info in processes {
        let pexe = normalize_path(&proc_info.exe_path);
        let pname = basename_of(&pexe);
        for finding in findings {
            let fexe = finding_exe(&finding.command);
            let fname = basename_of(&fexe);
            let level;
            let mut evidence = Vec::new();
            if !fexe.is_empty() && !pexe.is_empty() && fexe == pexe {
                // Services share images (one svchost.exe hosts dozens), so
                // a path match alone would correlate every instance with
                // every service. With the service's reported PID, DIRECT
                // requires pid equality; otherwise STRONG with the caveat.
                // (StartupRef.source carries the tag string here.)
                if finding.source == "windows-service" {
                    match finding.aux_pid {
                        Some(spid) if spid == proc_info.pid => {
                            level = CorrelationLevel::Direct;
                            evidence.push(
                                "startup entry references this exact executable path".to_string(),
                            );
                            evidence.push(format!(
                                "service PID matches observed pid {}",
                                proc_info.pid
                            ));
                        }
                        _ => {
                            level = CorrelationLevel::Strong;
                            evidence.push("same service image, different instance".to_string());
                            evidence
                                .push("verify the hosting PID with `sc.exe queryex`".to_string());
                        }
                    }
                } else {
                    level = CorrelationLevel::Direct;
                    evidence
                        .push("startup entry references this exact executable path".to_string());
                    evidence.push(format!("observed launch at pid {}", proc_info.pid));
                }
            } else if command_matches(&finding.command, proc_info) {
                level = CorrelationLevel::Strong;
                evidence.push("process command line matches the startup command".to_string());
                evidence.push(format!(
                    "observed within the window (first seen +{} ms)",
                    proc_info.first_seen_ms
                ));
            } else if !fname.is_empty() && !pname.is_empty() && fname == pname {
                level = CorrelationLevel::Partial;
                evidence.push("same executable name, different path".to_string());
                evidence.push(format!("startup: {}", finding.command));
                evidence.push(format!(
                    "observed: {}",
                    if proc_info.exe_path.is_empty() {
                        "(path unknown — exited before identification)".to_string()
                    } else {
                        proc_info.exe_path.clone()
                    }
                ));
            } else if same_folder(&fexe, &pexe) {
                level = CorrelationLevel::Weak;
                evidence.push("same folder as a startup executable, different program".to_string());
                evidence.push(format!("folder: {}", parent_dir_of(&pexe)));
            } else {
                continue;
            }
            if let Some(existing) = correlated_pids.get(&proc_info.pid) {
                if existing.rank() >= level.rank() {
                    continue;
                }
            }
            // Keep the strongest correlation per (process, finding) pair;
            // multiple findings may still match one process.
            if out.len() >= 256 {
                break;
            }
            correlated_pids.insert(proc_info.pid, level);
            out.push(Correlation {
                process_pid: proc_info.pid,
                process_name: proc_info.name.clone(),
                finding_id: finding.id.clone(),
                finding_name: finding.name.clone(),
                finding_source: finding.source.clone(),
                level,
                evidence,
            });
        }
    }

    // Pass 2: children of correlated processes (WEAK, structural only).
    let parents: HashSet<u32> = correlated_pids.keys().copied().collect();
    for proc_info in processes {
        if correlated_pids.contains_key(&proc_info.pid) || proc_info.ppid == 0 {
            continue;
        }
        if parents.contains(&proc_info.ppid) {
            if out.len() >= 256 {
                break;
            }
            out.push(Correlation {
                process_pid: proc_info.pid,
                process_name: proc_info.name.clone(),
                finding_id: String::new(),
                finding_name: format!("child of correlated pid {}", proc_info.ppid),
                finding_source: String::new(),
                level: CorrelationLevel::Weak,
                evidence: vec![format!(
                    "child process of correlated pid {} (proximity only)",
                    proc_info.ppid
                )],
            });
        }
    }
    out
}

fn command_matches(finding_command: &str, proc_info: &ObservedProcess) -> bool {
    const MIN_TOKEN: usize = 8;
    const MIN_ARGS: usize = 12;
    let fexe = finding_exe(finding_command);
    let pexe = normalize_path(&proc_info.exe_path);
    // Either direction, with a length floor against trivial matches.
    // Additionally both sides must look like programs (basename with an
    // extension): an unquoted `C:\Program Files\...` command truncates to
    // the first token (`c:/program`), which would otherwise substring-match
    // EVERY process under Program Files. That class of match is rejected
    // here (extensionless binaries still correlate via DIRECT/PARTIAL).
    if fexe.len() >= MIN_TOKEN && looks_like_program(&fexe) && !pexe.is_empty() {
        if pexe.contains(&fexe) {
            return true;
        }
        if let Some(cmdline) = proc_info.command_line.as_deref() {
            let cline = normalize_path(cmdline);
            if cline.contains(&fexe) {
                return true;
            }
            // Same distinctive arguments on different paths (e.g. a task's
            // flags reappearing in the observed command line).
            let fargs = finding_args(finding_command);
            if fargs.len() >= MIN_ARGS && cline.contains(&fargs) {
                return true;
            }
        }
    }
    if pexe.len() >= MIN_TOKEN && looks_like_program(&pexe) {
        let fcmd = normalize_path(finding_command);
        if fcmd.contains(&pexe) {
            return true;
        }
    }
    false
}

/// Basename carries an extension (`x.exe`, `lib.dll`) — guards path
/// substring matching against truncated first-tokens like `c:/program`.
fn looks_like_program(path: &str) -> bool {
    let base = basename_of(path);
    base.len() > 4 && base.contains('.')
}

/// Argument portion of a finding command (everything after the program
/// token), normalized. Empty when the command is a bare path.
fn finding_args(command: &str) -> String {
    let trimmed = command.trim();
    let token = crate::risk::extract_program_path(trimmed);
    let rest = trimmed
        .strip_prefix('"')
        .and_then(|q| q.find('"').map(|i| &q[i + 1..]))
        .unwrap_or_else(|| trimmed.get(token.len()..).unwrap_or(""));
    normalize_path(rest)
}

fn same_folder(fexe: &str, pexe: &str) -> bool {
    if fexe.is_empty() || pexe.is_empty() {
        return false;
    }
    let fd = parent_dir_of(fexe);
    !fd.is_empty() && fd == parent_dir_of(pexe) && basename_of(fexe) != basename_of(pexe)
}

/// Assemble the incident timeline from captured data. No fabricated
/// events: every entry traces to an observation, a correlation, or the
/// observation lifecycle itself.
pub fn build_timeline(
    start: SystemTime,
    duration_secs: u64,
    processes: &[ObservedProcess],
    windows: &[ObservedWindow],
    correlations: &[Correlation],
) -> Vec<TimelineEvent> {
    let mut timeline = Vec::new();
    timeline.push(TimelineEvent {
        t_ms: 0,
        wall_time: wall_clock(start, 0),
        kind: TimelineKind::ObservationStarted,
        text: "Login observation started".to_string(),
    });
    for p in processes {
        // Pre-existing processes are baseline context, not births — the
        // timeline would otherwise open with hundreds of simultaneous
        // "created" events on every run.
        if !p.pre_existing {
            timeline.push(TimelineEvent {
                t_ms: p.first_seen_ms,
                wall_time: wall_clock(start, p.first_seen_ms),
                kind: TimelineKind::ProcessCreated,
                text: format!(
                    "Process created: {} (pid {}{})",
                    if p.name.is_empty() { "?" } else { &p.name },
                    p.pid,
                    if p.exe_path.is_empty() {
                        ", exited before identification".to_string()
                    } else {
                        format!(" — {}", p.exe_path)
                    }
                ),
            });
        }
        if p.exited {
            timeline.push(TimelineEvent {
                t_ms: p.last_seen_ms,
                wall_time: wall_clock(start, p.last_seen_ms),
                kind: TimelineKind::ProcessExited,
                text: format!("Process exited: {} (pid {})", p.name, p.pid),
            });
        }
    }
    for w in windows {
        if !w.pre_existing {
            timeline.push(TimelineEvent {
                t_ms: w.first_seen_ms,
                wall_time: wall_clock(start, w.first_seen_ms),
                kind: TimelineKind::WindowOpened,
                text: format!(
                    "Window opened: \"{}\" (pid {}, {})",
                    if w.title.is_empty() {
                        "(no title)"
                    } else {
                        &w.title
                    },
                    w.pid,
                    w.class_name
                ),
            });
        }
        if w.closed {
            timeline.push(TimelineEvent {
                t_ms: w.last_seen_ms,
                wall_time: wall_clock(start, w.last_seen_ms),
                kind: TimelineKind::WindowClosed,
                text: format!(
                    "Window closed: \"{}\" after {} ms",
                    if w.title.is_empty() {
                        "(no title)"
                    } else {
                        &w.title
                    },
                    w.lifetime_ms()
                ),
            });
        }
    }
    for c in correlations
        .iter()
        .filter(|c| c.level == CorrelationLevel::Direct || c.level == CorrelationLevel::Strong)
    {
        timeline.push(TimelineEvent {
            t_ms: processes
                .iter()
                .find(|p| p.pid == c.process_pid)
                .map(|p| p.first_seen_ms)
                .unwrap_or(0),
            wall_time: String::new(),
            kind: TimelineKind::CorrelationNoted,
            text: format!(
                "{} correlation: {} ↔ {} ({})",
                c.level.label(),
                c.process_name,
                c.finding_name,
                c.finding_source
            ),
        });
    }
    for e in timeline.iter_mut().filter(|e| e.wall_time.is_empty()) {
        e.wall_time = wall_clock(start, e.t_ms);
    }
    timeline.sort_by_key(|e| (e.t_ms, e.text.clone()));
    timeline.push(TimelineEvent {
        t_ms: duration_secs * 1000,
        wall_time: wall_clock(start, duration_secs * 1000),
        kind: TimelineKind::ObservationEnded,
        text: "Observation window ended".to_string(),
    });
    if timeline.len() > 2000 {
        timeline.truncate(2000);
    }
    timeline
}

/// Decide the incident verdict from correlations, windows, and the
/// usability of the observation itself.
pub fn decide_verdict(
    correlations: &[Correlation],
    windows: &[ObservedWindow],
    process_observation: &SourceStatus,
) -> IncidentVerdict {
    if !process_observation.is_usable() {
        return IncidentVerdict::InsufficientObservation;
    }
    let transient_pids: HashSet<u32> = windows
        .iter()
        .filter(|w| w.is_transient())
        .map(|w| w.pid)
        .collect();
    let has_direct = correlations
        .iter()
        .any(|c| c.level == CorrelationLevel::Direct);
    let direct_with_window = correlations
        .iter()
        .any(|c| c.level == CorrelationLevel::Direct && transient_pids.contains(&c.process_pid));
    if direct_with_window {
        return IncidentVerdict::CauseIdentified;
    }
    let strong_with_window = correlations
        .iter()
        .any(|c| c.level == CorrelationLevel::Strong && transient_pids.contains(&c.process_pid));
    if has_direct || strong_with_window {
        return IncidentVerdict::StrongCorrelation;
    }
    let any_related = correlations
        .iter()
        .any(|c| c.level != CorrelationLevel::None);
    if any_related {
        return IncidentVerdict::ReviewRequired;
    }
    IncidentVerdict::NoDirectEvidence
}

// ---------------------------------------------------------------------------
// Live observation (Windows; read-only snapshots + metadata)
// ---------------------------------------------------------------------------

pub const OBS_POLL_MS: u64 = 500;
const MAX_TRACKED_PROCS: usize = 4096;
const MAX_TRACKED_WINDOWS: usize = 1024;
const EXIT_CONFIRM_POLLS: u32 = 2;
const MAX_CMDLINE_PIDS: usize = 16;

pub struct LiveObservation {
    pub processes: Vec<ObservedProcess>,
    pub windows: Vec<ObservedWindow>,
    pub process_observation: SourceStatus,
    pub window_observation: SourceStatus,
    pub truncated: bool,
}

/// Observe process starts/exits and window open/close for `duration`.
/// `progress(polls, procs, windows)` is called per poll for UI feedback.
/// No injection, no hooks, no execution, no screenshots, no keystrokes —
/// ToolHelp snapshots, WMI creation events (best-effort), and EnumWindows
/// metadata only.
#[cfg(windows)]
pub fn observe_blocking(
    duration: Duration,
    progress: impl Fn(usize, usize, usize),
) -> LiveObservation {
    use std::collections::HashMap;

    let start = Instant::now();
    let mut procs: HashMap<u32, ObservedProcess> = HashMap::new();
    let mut missing_polls: HashMap<u32, u32> = HashMap::new();
    let mut wins: HashMap<(u32, String, String), ObservedWindow> = HashMap::new();
    let mut win_missing: HashMap<(u32, String, String), u32> = HashMap::new();
    let mut truncated = false;

    // Event-driven creation notices where available; snapshots always run
    // (they also supply paths, exits, and the fallback when WMI refuses).
    let mut event_watch = WmiCreationWatch::start();
    let events_ok = event_watch.is_some();
    let mut event_pids: HashSet<u32> = HashSet::new();

    let mut polls = 0usize;
    while start.elapsed() < duration {
        let now_ms = start.elapsed().as_millis() as u64;
        let first_poll = polls == 0;

        // 1. Drain creation events (short timeout — never blocks the poll).
        if let Some(watch) = event_watch.as_mut() {
            for pid in watch.drain() {
                event_pids.insert(pid);
            }
            if watch.broken() {
                event_watch = None;
            }
        }

        // 2. Snapshot diff for paths, exits, and event-missed births.
        let mut seen_pids = HashSet::new();
        for info in crate::process_scan::enumerate_processes() {
            seen_pids.insert(info.pid);
            missing_polls.remove(&info.pid);
            if procs.len() >= MAX_TRACKED_PROCS {
                truncated = true;
            }
            procs
                .entry(info.pid)
                .and_modify(|p| {
                    p.last_seen_ms = now_ms;
                    if p.name.is_empty() {
                        p.name = info.name.clone();
                    }
                    if p.exe_path.is_empty() {
                        p.exe_path = info.exe_path.clone();
                    }
                })
                .or_insert_with(|| ObservedProcess {
                    pid: info.pid,
                    ppid: 0,
                    name: info.name.clone(),
                    exe_path: info.exe_path.clone(),
                    command_line: None,
                    first_seen_ms: now_ms,
                    last_seen_ms: now_ms,
                    exited: false,
                    via_events: event_pids.contains(&info.pid),
                    pre_existing: first_poll,
                });
        }
        // PPIDs come from a second cheap pass (ToolHelp parent ids) — folded
        // into the same snapshot to avoid extra syscalls per process.
        for (pid, ppid) in snapshot_ppid_map() {
            if let Some(p) = procs.get_mut(&pid) {
                if p.ppid == 0 {
                    p.ppid = ppid;
                }
            }
        }
        // Exits: absent for N consecutive polls (PID reuse can resurrect a
        // pid later — that arrival is recorded as a NEW observation).
        let gone: Vec<u32> = procs
            .keys()
            .filter(|pid| !seen_pids.contains(pid))
            .copied()
            .collect();
        for pid in gone {
            let count = missing_polls.entry(pid).or_insert(0);
            *count += 1;
            if *count >= EXIT_CONFIRM_POLLS {
                if let Some(p) = procs.get_mut(&pid) {
                    p.exited = true;
                }
                missing_pids_cleanup(&mut missing_polls, pid);
            }
        }

        // 3. Window metadata (titles + classes only — never contents).
        let mut seen_wins = HashSet::new();
        for (pid, title, class) in snapshot_windows() {
            let key = (pid, title.clone(), class.clone());
            seen_wins.insert(key.clone());
            win_missing.remove(&key);
            if wins.len() >= MAX_TRACKED_WINDOWS {
                truncated = true;
            }
            wins.entry(key)
                .and_modify(|w| w.last_seen_ms = now_ms)
                .or_insert_with(|| ObservedWindow {
                    pid,
                    title,
                    class_name: class,
                    first_seen_ms: now_ms,
                    last_seen_ms: now_ms,
                    closed: false,
                    pre_existing: first_poll,
                });
        }
        let gone_wins: Vec<_> = wins
            .keys()
            .filter(|k| !seen_wins.contains(*k))
            .cloned()
            .collect();
        for key in gone_wins {
            let count = win_missing.entry(key.clone()).or_insert(0);
            *count += 1;
            if *count >= EXIT_CONFIRM_POLLS {
                if let Some(w) = wins.get_mut(&key) {
                    w.closed = true;
                }
                win_missing.remove(&key);
            }
        }

        polls += 1;
        progress(polls, procs.len(), wins.len());
        let elapsed = start.elapsed();
        if elapsed < duration {
            std::thread::sleep(
                (Duration::from_millis(OBS_POLL_MS)).saturating_sub(Duration::from_millis(50)),
            );
        }
    }

    let process_observation = if events_ok || !procs.is_empty() {
        SourceStatus::Available
    } else {
        SourceStatus::Partial {
            skipped: 0,
            reason: "creation events unavailable; snapshot diff only (sub-500 ms processes may be missed)".to_string(),
        }
    };
    // If the event watcher broke mid-run but snapshots still produced data,
    // that is still usable observation — flagged, not failed.
    let process_observation = match (&process_observation, event_watch.is_none()) {
        (SourceStatus::Available, true) if polls > 2 => SourceStatus::Partial {
            skipped: 0,
            reason: "creation events unavailable; snapshot diff only (sub-500 ms processes may be missed)".to_string(),
        },
        _ => process_observation,
    };

    let mut processes: Vec<ObservedProcess> = procs.into_values().collect();
    processes.sort_by_key(|p| (p.first_seen_ms, p.pid));
    let mut windows: Vec<ObservedWindow> = wins.into_values().collect();
    windows.sort_by_key(|w| (w.first_seen_ms, w.pid));
    LiveObservation {
        processes,
        windows,
        process_observation,
        window_observation: SourceStatus::Available,
        truncated,
    }
}

#[cfg(not(windows))]
pub fn observe_blocking(
    _duration: Duration,
    _progress: impl Fn(usize, usize, usize),
) -> LiveObservation {
    LiveObservation {
        processes: Vec::new(),
        windows: Vec::new(),
        process_observation: SourceStatus::Unavailable {
            reason: "Windows-only observation".to_string(),
        },
        window_observation: SourceStatus::Unavailable {
            reason: "Windows-only observation".to_string(),
        },
        truncated: false,
    }
}

#[cfg(windows)]
fn missing_pids_cleanup(map: &mut HashMap<u32, u32>, pid: u32) {
    map.remove(&pid);
}

/// PID → PPID map from one ToolHelp snapshot (cheap; no path resolution).
#[cfg(windows)]
fn snapshot_ppid_map() -> Vec<(u32, u32)> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..std::mem::zeroed()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                out.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    out
}

/// Visible top-level windows: (pid, title, class). Titles/classes only.
#[cfg(windows)]
fn snapshot_windows() -> Vec<(u32, String, String)> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };
    struct Acc {
        out: Vec<(u32, String, String)>,
    }
    unsafe extern "system" fn collect(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            let acc = &mut *(lparam.0 as *mut Acc);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return true.into();
            }
            let mut title = [0u16; 256];
            let tlen = GetWindowTextW(hwnd, &mut title);
            let mut class = [0u16; 256];
            let clen = GetClassNameW(hwnd, &mut class);
            // Skip untitled system chrome with no identity value.
            if tlen == 0 && clen == 0 {
                return true.into();
            }
            acc.out.push((
                pid,
                String::from_utf16_lossy(&title[..tlen as usize])
                    .trim()
                    .to_string(),
                String::from_utf16_lossy(&class[..clen as usize])
                    .trim()
                    .to_string(),
            ));
            if acc.out.len() >= MAX_TRACKED_WINDOWS {
                return false.into();
            }
        }
        true.into()
    }
    let mut acc = Acc { out: Vec::new() };
    let ptr = &mut acc as *mut Acc as isize;
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(ptr));
    }
    acc.out
}

/// Best-effort command lines for a bounded set of processes.
/// `requests` carries (pid, observed process name) pairs: a command line is
/// attached ONLY when the live process name still matches the observed one
/// (PID-reuse guard — otherwise the pid was recycled and the command line
/// belongs to a different process). Each failure yields nothing — never an
/// error.
/// NOTE: command lines can contain third-party secrets (tokens in args);
/// callers show them as evidence and must note export handling.
#[cfg(windows)]
pub fn fetch_command_lines(requests: &[(u32, String)]) -> HashMap<u32, String> {
    use windows::core::BSTR;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED, EOLE_AUTHENTICATION_CAPABILITIES, RPC_C_AUTHN_LEVEL_CALL,
        RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::Wmi::{
        WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    };

    let mut out = HashMap::new();
    if requests.is_empty() {
        return out;
    }
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() {
            return out;
        }
        let result = (|| -> windows::core::Result<()> {
            let locator: windows::Win32::System::Wmi::IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let server = locator.ConnectServer(
                &BSTR::from("ROOT\\CIMV2"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )?;
            CoSetProxyBlanket(
                &server,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOLE_AUTHENTICATION_CAPABILITIES(0),
            )?;
            for (pid, observed_name) in requests.iter().take(MAX_CMDLINE_PIDS) {
                let query =
                    format!("SELECT Name,CommandLine FROM Win32_Process WHERE Handle='{pid}'");
                let Ok(en) = server.ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from(query.as_str()),
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                ) else {
                    continue;
                };
                let mut objects = [None];
                let mut returned = 0u32;
                if en.Next(2000, &mut objects, &mut returned).is_err() || returned == 0 {
                    continue;
                }
                if let Some(obj) = objects[0].take() {
                    if let Ok(text) = obj.GetObjectText(0) {
                        let text = text.to_string();
                        let live_name =
                            crate::scanners::wmi::mof_prop(&text, "Name").unwrap_or_default();
                        if !cmdline_owner_matches(observed_name, &live_name) {
                            continue;
                        }
                        if let Some(cmd) = crate::scanners::wmi::mof_prop(&text, "CommandLine") {
                            let cmd = cmd.trim().to_string();
                            if !cmd.is_empty() {
                                out.insert(*pid, cmd);
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        let _ = result;
        if init == windows::Win32::Foundation::S_OK {
            CoUninitialize();
        }
    }
    out
}

#[cfg(not(windows))]
pub fn fetch_command_lines(_requests: &[(u32, String)]) -> HashMap<u32, String> {
    HashMap::new()
}

/// PID-reuse guard for command-line attribution: attach only on
/// case-insensitive name equality (both non-empty). Pure and unit-tested.
pub fn cmdline_owner_matches(observed_name: &str, live_name: &str) -> bool {
    !observed_name.is_empty()
        && !live_name.is_empty()
        && observed_name.eq_ignore_ascii_case(live_name)
}

/// WMI process-creation event watcher (best-effort companion to snapshot
/// diffing). Forward-only `__InstanceCreationEvent` subscription with
/// short drains; any failure marks it broken (caller degrades to
/// snapshots with a documented reason — never an error).
#[cfg(windows)]
struct WmiCreationWatch {
    enumerator: windows::Win32::System::Wmi::IEnumWbemClassObject,
    broken: bool,
}

#[cfg(windows)]
impl WmiCreationWatch {
    fn start() -> Option<Self> {
        unsafe {
            use windows::core::BSTR;
            use windows::core::PCWSTR;
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED, EOLE_AUTHENTICATION_CAPABILITIES, RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
            };
            use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
            use windows::Win32::System::Wmi::{
                WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
            };
            // NOTE: no CoInitializeEx here — the observer runs this on a
            // thread the caller owns; fetch_command_lines initializes per
            // call. Event subscription needs its own init: attempt it, and
            // treat S_FALSE (already initialized) as fine.
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            if init.is_err() {
                return None;
            }
            // Intentionally never uninitialized: the thread may outlive us
            // within the observation; COM per-thread init is idempotent.
            let locator: windows::Win32::System::Wmi::IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
            let server = locator
                .ConnectServer(
                    &BSTR::from("ROOT\\CIMV2"),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .ok()?;
            CoSetProxyBlanket(
                &server,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOLE_AUTHENTICATION_CAPABILITIES(0),
            )
            .ok()?;
            let enumerator = server
                .ExecNotificationQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from("SELECT * FROM __InstanceCreationEvent WITHIN 1 WHERE TargetInstance ISA 'Win32_Process'"),
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
                .ok()?;
            Some(Self {
                enumerator,
                broken: false,
            })
        }
    }

    fn broken(&self) -> bool {
        self.broken
    }

    /// Drain pending creation PIDs (Handle="N" parsed from the event's
    /// TargetInstance reference). Bounded per call.
    fn drain(&mut self) -> Vec<u32> {
        let mut pids = Vec::new();
        if self.broken {
            return pids;
        }
        unsafe {
            for _ in 0..32 {
                let mut objects = [None];
                let mut returned = 0u32;
                // 250 ms: prompt enough to keep the poll cadence, short
                // enough to never stall it.
                if self
                    .enumerator
                    .Next(250, &mut objects, &mut returned)
                    .is_err()
                    || returned == 0
                {
                    break;
                }
                if let Some(obj) = objects[0].take() {
                    let text = obj
                        .GetObjectText(0)
                        .map(|b| b.to_string())
                        .unwrap_or_default();
                    if let Some(pid) = parse_event_pid(&text) {
                        pids.push(pid);
                    }
                }
            }
        }
        pids
    }
}

/// `TargetInstance = "\\PC\ROOT\CIMV2:Win32_Process.Handle=\"1234\""`
/// → 1234. Pure string parsing on our own query output. The value quote
/// may itself be MOF-escaped (`\"`), which is skipped.
fn parse_event_pid(mof: &str) -> Option<u32> {
    let key = "Win32_Process.Handle=";
    let start = mof.find(key)? + key.len();
    let rest = mof[start..].trim_start_matches('\\');
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // All fixtures synthetic: CURE-SYNTH names, example.invalid-free paths
    // under a fictional user. No real processes, no real findings.

    fn finding(id: &str, source: &str, name: &str, command: &str) -> StartupRef {
        StartupRef {
            id: id.to_string(),
            source: source.to_string(),
            name: name.to_string(),
            command: command.to_string(),
            location: format!(r"C:\cure-synth\{name}"),
            aux_pid: None,
        }
    }

    fn proc(pid: u32, name: &str, exe: &str, t_first: u64) -> ObservedProcess {
        ObservedProcess {
            pid,
            ppid: 1000,
            name: name.to_string(),
            exe_path: exe.to_string(),
            command_line: None,
            first_seen_ms: t_first,
            last_seen_ms: t_first + 4000,
            exited: true,
            via_events: false,
            pre_existing: false,
        }
    }

    fn win(pid: u32, title: &str, t0: u64, life_ms: u64, closed: bool) -> ObservedWindow {
        ObservedWindow {
            pid,
            title: title.to_string(),
            class_name: "CURESynthWindow".to_string(),
            first_seen_ms: t0,
            last_seen_ms: t0 + life_ms,
            closed,
            pre_existing: false,
        }
    }

    fn start() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    /// A: Run key → observed process → matching executable = DIRECT.
    #[test]
    fn fixture_a_run_key_to_process_is_direct() {
        let findings = vec![finding(
            "a1",
            "StartupFolder",
            "updater.lnk",
            r"C:\Users\CURE-SYNTH\AppData\Local\Foo\updater.exe",
        )];
        let procs = vec![proc(
            8412,
            "updater.exe",
            r"C:\Users\CURE-SYNTH\AppData\Local\Foo\updater.exe",
            2421,
        )];
        let out = correlate(&findings, &procs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Direct);
        assert!(out[0]
            .evidence
            .iter()
            .any(|e| e.contains("exact executable")));
    }

    /// B: Scheduled task → observed process → matching command = STRONG.
    #[test]
    fn fixture_b_task_to_process_command_is_strong() {
        let findings = vec![finding(
            "b1",
            "ScheduledTask",
            "CURE-SYNTH\\Nightly",
            r"C:\cure-synth\tools\nightly.exe --full --target=example",
        )];
        let mut p = proc(8436, "nightly.exe", r"C:\other\bin\nightly.exe", 3100);
        p.command_line = Some(r"C:\other\bin\nightly.exe --full --target=example".to_string());
        let out = correlate(&findings, &[p]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Strong);
    }

    /// C: Service image → observed process = DIRECT.
    #[test]
    fn fixture_c_service_to_process_is_direct() {
        let findings = vec![finding(
            "c1",
            "WindowsService",
            "CURE-SYNTH-Agent",
            r#""C:\Program Files\CURE-SYNTH\agent.exe" --service"#,
        )];
        let procs = vec![proc(
            900,
            "agent.exe",
            r"C:\Program Files\CURE-SYNTH\agent.exe",
            1200,
        )];
        let out = correlate(&findings, &procs);
        // Note: service command carries args, so the full-path comparison
        // uses the extracted program token — still DIRECT.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Direct);
    }

    /// D: Unrelated process with the same name, no path relation → PARTIAL max.
    #[test]
    fn fixture_d_same_name_only_is_partial() {
        let findings = vec![finding(
            "d1",
            "StartupFolder",
            "updater.lnk",
            r"C:\Users\CURE-SYNTH\AppData\Local\Foo\updater.exe",
        )];
        let procs = vec![proc(9001, "updater.exe", r"D:\Games\Bar\updater.exe", 2500)];
        let out = correlate(&findings, &procs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Partial);
        assert!(out[0].evidence.iter().any(|e| e.contains("different path")));
    }

    /// E: Same name, different path — evidence shows BOTH paths.
    #[test]
    fn fixture_e_partial_evidence_shows_both_paths() {
        let findings = vec![finding(
            "e1",
            "RegistryRun",
            "CURE-SYNTH-Updater",
            r#""C:\Program Files\Foo\updater.exe" /bg"#,
        )];
        let procs = vec![proc(
            9002,
            "updater.exe",
            r"C:\Users\CURE-SYNTH\evil\updater.exe",
            2600,
        )];
        let out = correlate(&findings, &procs);
        assert_eq!(out[0].level, CorrelationLevel::Partial);
        let joined = out[0].evidence.join("\n");
        assert!(
            joined.contains(r"C:\Program Files\Foo\updater.exe"),
            "{joined}"
        );
        assert!(
            joined.contains(r"C:\Users\CURE-SYNTH\evil\updater.exe"),
            "{joined}"
        );
    }

    /// F: Temporal proximity without any relationship = NONE.
    #[test]
    fn fixture_f_unrelated_process_is_none() {
        let findings = vec![finding(
            "f1",
            "StartupFolder",
            "updater.lnk",
            r"C:\Users\CURE-SYNTH\AppData\Local\Foo\updater.exe",
        )];
        let procs = vec![proc(
            9100,
            "spotify.exe",
            r"C:\Program Files\Music\spotify.exe",
            2400,
        )];
        assert!(correlate(&findings, &procs).is_empty());
    }

    /// REGRESSION (found live): an UNQUOTED `C:\Program Files\…` finding
    /// command truncates to the first token (`c:/program`), which used to
    /// substring-match every process under Program Files (e.g. an HP
    /// autorun "correlating" with ChatGPT). Extensionless truncated tokens
    /// must never drive a STRONG match.
    #[test]
    fn unquoted_program_files_prefix_never_matches() {
        let findings = vec![finding(
            "hp1",
            "RegistryRun",
            "CURE-SYNTH-HPSEU",
            r"C:\Program Files\HP\HP System Event Utility\Host Launcher\HpseuHostLauncher.exe",
        )];
        let procs = vec![proc(
            9101,
            "ChatGPT.exe",
            r"C:\Program Files\WindowsApps\CURE-SYNTH\ChatGPT.exe",
            2400,
        )];
        assert!(
            correlate(&findings, &procs).is_empty(),
            "truncated first-token must not correlate unrelated Program Files processes"
        );
        assert!(!looks_like_program("c:/program"));
        assert!(looks_like_program("c:/windows/system32/svchost.exe"));
    }

    /// G: Process exits before identification (no exe path) = NONE, honest.
    #[test]
    fn fixture_g_unidentified_short_lived_process_is_none() {
        let findings = vec![finding(
            "g1",
            "StartupFolder",
            "updater.lnk",
            r"C:\Users\CURE-SYNTH\AppData\Local\Foo\updater.exe",
        )];
        let mut p = proc(9200, "", "", 500);
        p.name = String::new();
        p.exited = true;
        assert!(correlate(&findings, &[p]).is_empty());
    }

    /// H/I: failed or unavailable observation → INSUFFICIENT OBSERVATION.
    #[test]
    fn fixture_h_access_denied_is_insufficient() {
        let v = decide_verdict(&[], &[], &SourceStatus::AccessDenied { reason: "t".into() });
        assert_eq!(v, IncidentVerdict::InsufficientObservation);
    }

    #[test]
    fn fixture_i_unavailable_is_insufficient() {
        let v = decide_verdict(&[], &[], &SourceStatus::Unavailable { reason: "t".into() });
        assert_eq!(v, IncidentVerdict::InsufficientObservation);
    }

    /// J: WMI enumeration failure does NOT poison a usable process
    /// observation — the verdict still decides on what was observed.
    #[test]
    fn fixture_j_mixed_status_still_decides() {
        // decide_verdict gates only on process observation usability; a
        // failed WMI check reported elsewhere must not force INSUFFICIENT.
        let v = decide_verdict(&[], &[], &SourceStatus::Available);
        assert_eq!(v, IncidentVerdict::NoDirectEvidence);
    }

    /// K: transient window on a DIRECT process → CAUSE IDENTIFIED;
    /// without the window → STRONG CORRELATION.
    #[test]
    fn fixture_k_transient_window_decides_cause() {
        let findings = vec![finding(
            "k1",
            "ScheduledTask",
            "CURE-SYNTH\\Foo",
            r"C:\cure-synth\foo.exe",
        )];
        let procs = vec![proc(8124, "foo.exe", r"C:\cure-synth\foo.exe", 2481)];
        let corrs = correlate(&findings, &procs);
        assert_eq!(corrs[0].level, CorrelationLevel::Direct);
        let wins = vec![win(8124, "Application Error", 2500, 742, true)];
        assert!(wins[0].is_transient());
        assert_eq!(
            decide_verdict(&corrs, &wins, &SourceStatus::Available),
            IncidentVerdict::CauseIdentified
        );
        assert_eq!(
            decide_verdict(&corrs, &[], &SourceStatus::Available),
            IncidentVerdict::StrongCorrelation
        );
    }

    #[test]
    fn verdict_priority_order() {
        // STRONG alone (no window) → review, not strong-correlation.
        let strong = Correlation {
            process_pid: 1,
            process_name: "a".into(),
            finding_id: "f".into(),
            finding_name: "g".into(),
            finding_source: "s".into(),
            level: CorrelationLevel::Strong,
            evidence: vec![],
        };
        assert_eq!(
            decide_verdict(std::slice::from_ref(&strong), &[], &SourceStatus::Available),
            IncidentVerdict::ReviewRequired
        );
        // PARTIAL alone → review.
        let mut partial = strong.clone();
        partial.level = CorrelationLevel::Partial;
        assert_eq!(
            decide_verdict(&[partial], &[], &SourceStatus::Available),
            IncidentVerdict::ReviewRequired
        );
        // Quiet usable observation → no direct evidence (not insufficient).
        assert_eq!(
            decide_verdict(&[], &[], &SourceStatus::Available),
            IncidentVerdict::NoDirectEvidence
        );
    }

    #[test]
    fn timeline_is_ordered_and_complete() {
        let procs = vec![proc(8124, "foo.exe", r"C:\cure-synth\foo.exe", 3000)];
        let wins = vec![win(8124, "Foo Update", 3050, 742, true)];
        let corrs = vec![Correlation {
            process_pid: 8124,
            process_name: "foo.exe".into(),
            finding_id: "k1".into(),
            finding_name: "Foo".into(),
            finding_source: "ScheduledTask".into(),
            level: CorrelationLevel::Direct,
            evidence: vec![],
        }];
        let tl = build_timeline(start(), 30, &procs, &wins, &corrs);
        let kinds: Vec<TimelineKind> = tl.iter().map(|e| e.kind).collect();
        assert_eq!(kinds.first(), Some(&TimelineKind::ObservationStarted));
        assert_eq!(kinds.last(), Some(&TimelineKind::ObservationEnded));
        assert!(kinds.contains(&TimelineKind::ProcessCreated));
        assert!(kinds.contains(&TimelineKind::ProcessExited));
        assert!(kinds.contains(&TimelineKind::WindowOpened));
        assert!(kinds.contains(&TimelineKind::WindowClosed));
        assert!(kinds.contains(&TimelineKind::CorrelationNoted));
        let mut last = 0u64;
        for e in &tl[..tl.len() - 1] {
            assert!(e.t_ms >= last, "timeline out of order at {}", e.text);
            last = e.t_ms;
        }
    }

    #[test]
    fn empty_observation_still_marks_lifecycle() {
        // Zero events is data, not absence: a quiet-but-usable observation
        // yields exactly the Started/Ended bookends (never an empty
        // timeline that could read as "nothing happened").
        let tl = build_timeline(start(), 30, &[], &[], &[]);
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0].kind, TimelineKind::ObservationStarted);
        assert_eq!(tl[1].kind, TimelineKind::ObservationEnded);
        assert_eq!(tl[1].t_ms, 30_000);
    }

    #[test]
    fn window_lifetime_math() {
        let w = win(1, "X", 1000, 742, true);
        assert_eq!(w.lifetime_ms(), 742);
        assert!(w.is_transient());
        let long = win(1, "X", 1000, 60_000, true);
        assert!(!long.is_transient());
        let open = win(1, "X", 1000, 60_000, false);
        assert!(!open.is_transient());
    }

    #[test]
    fn durations_validated() {
        assert_eq!(validate_duration(30), Ok(30));
        assert!(validate_duration(45).is_err());
        assert!(validate_duration(0).is_err());
        assert!(validate_duration(3600).is_err());
    }

    #[test]
    fn event_pid_parses() {
        assert_eq!(
            parse_event_pid(
                "TargetInstance = \"\\\\PC\\ROOT\\CIMV2:Win32_Process.Handle=\\\"8412\\\"\""
            ),
            Some(8412)
        );
        assert_eq!(parse_event_pid("no handle here"), None);
    }

    /// LIVE short-lived process test (Windows only, SAFE fixtures).
    ///
    /// Two markers inside a ~4 s observation: (1) a uniquely-named copy of
    /// cmd.exe living ~4 s (snapshot diff across 500 ms polls must catch a
    /// process alive for many polls, with its exact path); (2) the
    /// fake-overlay fixture (~1.2 s, known pid) for the window path.
    /// An instant `cmd /C exit` exercises the opportunistic path with no
    /// assertions - sub-500 ms detection is NOT guaranteed. Everything is
    /// killed and waited before any assertion so no fixture process
    /// outlives the test. (System notepad cannot be used: single-instance
    /// hosting breaks pid attribution, and a copied notepad
    /// self-terminates on Win11.)
    #[cfg(windows)]
    #[test]
    fn live_short_lived_marker_is_observed() {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let system32 = std::path::Path::new(&system_root).join("System32");
        if !system32.join("cmd.exe").is_file() {
            return;
        }
        let overlay_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testing/fake-overlay/target/release/fake-overlay.exe");
        let overlay_available = overlay_bin.is_file();

        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("CURE-SYNTH-popup-test.exe");
        std::fs::copy(system32.join("cmd.exe"), &marker).expect("copy marker binary");

        let (observed, overlay_pid) = std::thread::scope(|scope| {
            let handle = scope.spawn(|| observe_blocking(Duration::from_secs(4), |_, _, _| {}));
            // Let the observer take its baseline poll, then show markers.
            std::thread::sleep(Duration::from_millis(200));
            let mut runner = Command::new(&marker);
            // ping-based dwell: console-independent (timeout.exe needs
            // stdin and dies instantly headless).
            runner.args(["/C", "ping", "-n", "5", "127.0.0.1"]);
            runner.creation_flags(CREATE_NO_WINDOW);
            let mut runner = runner.spawn().expect("spawn marker");
            let mut overlay = overlay_available
                .then(|| Command::new(&overlay_bin).spawn().ok())
                .flatten();
            // An instant process alongside (opportunistic path, no asserts).
            let _ = Command::new(system32.join("cmd.exe"))
                .args(["/C", "exit", "0"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            std::thread::sleep(Duration::from_millis(1200));
            let overlay_pid = overlay.as_mut().map(|c| c.id());
            if let Some(c) = overlay.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
            let _ = runner.wait();
            drop(runner);
            (handle.join().expect("observer thread"), overlay_pid)
        });

        // Marker 1 (unique name, ~2 s): exactly one observation, exact path,
        // spanning multiple polls.
        let found: Vec<_> = observed
            .processes
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case("CURE-SYNTH-popup-test.exe"))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "marker process must be observed exactly once (pids seen: {})",
            observed.processes.len()
        );
        assert_eq!(
            found[0].exe_path,
            marker.to_string_lossy().as_ref(),
            "marker path must match exactly"
        );
        assert!(
            found[0].last_seen_ms > found[0].first_seen_ms,
            "marker must span multiple polls"
        );
        // Overlay window assertions: only when the fixture binary exists
        // (built on demand) and the box shows any desktop windows at all.
        if overlay_available && !observed.windows.is_empty() {
            let overlay_pid = overlay_pid.expect("overlay spawned, so its pid is known");
            let wins: Vec<_> = observed
                .windows
                .iter()
                .filter(|w| w.pid == overlay_pid)
                .collect();
            assert!(
                !wins.is_empty(),
                "overlay window must be observed on a desktop box"
            );
            let win = wins[0];
            assert!(
                win.title.contains("CURE TEST FIXTURE"),
                "overlay window must carry its fixture title, got {:?}",
                win.title
            );
            assert!(
                win.closed,
                "overlay was killed mid-observation so its window must read closed"
            );
            assert!(
                win.lifetime_ms() < 5000,
                "overlay window must classify transient"
            );
            assert!(win.is_transient());
        }

        // Observation itself must be usable (never INSUFFICIENT here).
        assert!(observed.process_observation.is_usable());
        let verdict = decide_verdict(&[], &observed.windows, &observed.process_observation);
        assert_eq!(verdict, IncidentVerdict::NoDirectEvidence);
    }

    #[test]
    fn verdict_labels_and_explanations() {
        assert_eq!(IncidentVerdict::CauseIdentified.label(), "CAUSE IDENTIFIED");
        assert!(IncidentVerdict::CauseIdentified
            .explanation()
            .contains("not its intent"));
        assert!(IncidentVerdict::StrongCorrelation
            .explanation()
            .contains("not proof"));
    }

    #[test]
    fn cmdline_owner_match_is_case_insensitive_and_strict() {
        assert!(cmdline_owner_matches("foo.exe", "foo.exe"));
        assert!(cmdline_owner_matches("Foo.EXE", "foo.exe"));
        assert!(!cmdline_owner_matches("foo.exe", "bar.exe"));
        assert!(!cmdline_owner_matches("", "foo.exe"));
        assert!(!cmdline_owner_matches("foo.exe", ""));
        assert!(!cmdline_owner_matches("", ""));
    }

    #[test]
    fn reused_pid_does_not_merge_lifecycles() {
        // Same pid, two different programs at different times: correlation
        // treats each observation independently — names/paths never merge.
        let old = proc(5000, "old-tool.exe", r"C:\cure-synth\old-tool.exe", 1000);
        let new = proc(5000, "new-tool.exe", r"C:\cure-synth\new-tool.exe", 9000);
        let findings = vec![finding(
            "r1",
            "StartupFolder",
            "old-tool",
            r"C:\cure-synth\old-tool.exe",
        )];
        // Without command lines: old lifecycle DIRECT, new lifecycle NONE.
        let out = correlate(&findings, &[old.clone(), new.clone()]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Direct);
        // If a STALE command line (previous lifecycle) were attached to the
        // new observation, it would fabricate a STRONG correlation — which
        // is exactly why fetch_command_lines requires a name match first.
        // (Per-pid best-only dedup keeps the DIRECT row either way.)
        let mut poisoned = new.clone();
        poisoned.command_line = Some(r"C:\cure-synth\old-tool.exe --flag".to_string());
        let out = correlate(&findings, &[old, poisoned]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Direct);
        assert!(!cmdline_owner_matches("new-tool.exe", "old-tool.exe"));
        assert!(cmdline_owner_matches("new-tool.exe", "new-tool.exe"));
    }

    #[test]
    fn service_pid_match_is_direct_mismatch_is_strong() {
        // svchost.exe hosts dozens of services: the path alone must not
        // DIRECT-correlate every instance with every service finding.
        let mut svc = finding(
            "s1",
            "windows-service",
            "CURE-SYNTH-Svc",
            r"C:\Windows\System32\svchost.exe -k CURE-SYNTH",
        );
        svc.aux_pid = Some(4242);
        let same_exe_other_pid = proc(
            9999,
            "svchost.exe",
            r"C:\Windows\System32\svchost.exe",
            1500,
        );
        let out = correlate(&[svc.clone()], &[same_exe_other_pid]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Strong);
        assert!(out[0]
            .evidence
            .iter()
            .any(|e| e.contains("different instance")));

        let hosted = proc(
            4242,
            "svchost.exe",
            r"C:\Windows\System32\svchost.exe",
            1500,
        );
        let out = correlate(&[svc], &[hosted]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, CorrelationLevel::Direct);
    }

    #[test]
    fn pre_existing_observations_skip_birth_events() {
        let mut old = proc(100, "old.exe", r"C:\cure-synth\old.exe", 0);
        old.pre_existing = true;
        old.exited = false;
        let mut old_win = win(100, "Old", 0, 9000, false);
        old_win.pre_existing = true;
        let tl = build_timeline(start(), 30, &[old], &[old_win], &[]);
        let kinds: Vec<TimelineKind> = tl.iter().map(|e| e.kind).collect();
        assert!(!kinds.contains(&TimelineKind::ProcessCreated));
        assert!(!kinds.contains(&TimelineKind::WindowOpened));
        assert_eq!(kinds.first(), Some(&TimelineKind::ObservationStarted));
    }
}
