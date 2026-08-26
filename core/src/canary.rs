//! Real-time ransomware canary detection engine.
//!
//! Pure state machine — no filesystem access, no OS calls, no clock reads.
//! The caller feeds [`FileEvent`]s (from `ReadDirectoryChangesW` or any
//! other watcher) plus synthetic timestamps, and the engine answers with
//! zero or more [`CanaryAlert`]s.

use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

// -- decoys -----------------------------------------------------------

pub const CANARY_STEM_MARKER: &str = "~cure-canary";

pub const DECOY_TEMPLATES: &[&str] = &[
    "~cure-canary-report.docx",
    "~cure-canary-invoice.xlsx",
    "~cure-canary-photos.zip",
    "~cure-canary-notes.txt",
    "~cure-canary-backup.pst",
    "~cure-canary-scan.pdf",
];

pub fn is_canary_decoy(file_name: &str) -> bool {
    file_name.to_ascii_lowercase().contains(CANARY_STEM_MARKER)
}

pub fn decoy_names(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            if i < DECOY_TEMPLATES.len() {
                DECOY_TEMPLATES[i].to_string()
            } else {
                let template = DECOY_TEMPLATES[i % DECOY_TEMPLATES.len()];
                let (stem, ext) = match template.rsplit_once('.') {
                    Some((s, e)) => (s, e),
                    None => (template, "bin"),
                };
                format!("{stem}-{i}.{ext}")
            }
        })
        .collect()
}

// -- events -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEvent {
    pub at_secs: u64,
    pub folder: String,
    pub name: String,
    pub kind: FileEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileEventKind {
    Added,
    Removed,
    Modified,
    RenamedOldName,
    RenamedNewName,
}

impl FileEventKind {
    fn counts_toward_burst(self) -> bool {
        matches!(
            self,
            FileEventKind::Added | FileEventKind::Modified | FileEventKind::RenamedNewName
        )
    }
}

// -- alerts -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanaryAlert {
    CanaryTamper {
        folder: String,
        file: String,
        action: &'static str,
        at_secs: u64,
    },
    BurstEncryption {
        folder: String,
        distinct_files: usize,
        window_secs: u64,
        at_secs: u64,
    },
    ExtensionRewrite {
        folder: String,
        extension: String,
        renamed_count: usize,
        at_secs: u64,
    },
}

impl CanaryAlert {
    pub fn severity(&self) -> u8 {
        match self {
            CanaryAlert::CanaryTamper { .. } => 3,
            CanaryAlert::ExtensionRewrite { .. } => 2,
            CanaryAlert::BurstEncryption { .. } => 1,
        }
    }

    fn cooldown_key(&self) -> String {
        match self {
            CanaryAlert::CanaryTamper { folder, .. } => format!("tamper|{folder}"),
            CanaryAlert::BurstEncryption { folder, .. } => format!("burst|{folder}"),
            CanaryAlert::ExtensionRewrite { folder, .. } => format!("rewrite|{folder}"),
        }
    }
}

// -- process tripwire -------------------------------------------------

struct TripwirePattern {
    phrase_halves: [&'static str; 2],
    reason: &'static str,
}

fn tripwire_patterns() -> &'static [TripwirePattern] {
    static PATTERNS: OnceLock<Vec<TripwirePattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            TripwirePattern {
                phrase_halves: ["delete ", "shadows"],
                reason: "recovery infrastructure is being destroyed",
            },
            TripwirePattern {
                phrase_halves: ["delete ", "catalog"],
                reason: "backup catalog is being destroyed",
            },
            TripwirePattern {
                phrase_halves: ["recoveryenabled ", "no"],
                reason: "system recovery is being disabled",
            },
            TripwirePattern {
                phrase_halves: ["shadowcopy ", "delete"],
                reason: "volume snapshots are being removed",
            },
        ]
    })
}

/// List of executable name suffixes that should be checked as tripwires.
fn tripwire_exe_suffixes() -> &'static [&'static str] {
    static SUFFIXES: OnceLock<Vec<&str>> = OnceLock::new();
    SUFFIXES.get_or_init(|| {
        vec![
            concat!("vss", "admin.exe"),
            concat!("disk", "shadow.exe"),
            concat!("wb", "admin.exe"),
            concat!("bcd", "edit.exe"),
            concat!("wm", "ic.exe"),
        ]
    })
}

pub fn shadow_wipe_reason(image_name: &str, command_line: &str) -> Option<&'static str> {
    let image = image_name.to_ascii_lowercase();
    let line = command_line.to_ascii_lowercase();
    if line.is_empty() {
        return None;
    }
    let suffixes = tripwire_exe_suffixes();
    let is_target = suffixes.iter().any(|s| image.ends_with(s));
    if !is_target {
        return None;
    }
    for pat in tripwire_patterns() {
        let phrase = format!("{}{}", pat.phrase_halves[0], pat.phrase_halves[1]);
        if line.contains(&phrase) {
            return Some(pat.reason);
        }
    }
    None
}

// -- engine config ----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryConfig {
    pub burst_min_files: usize,
    pub burst_window_secs: u64,
    pub rewrite_min_renames: usize,
    pub alert_cooldown_secs: u64,
    pub max_tracked_events: usize,
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            burst_min_files: 8,
            burst_window_secs: 30,
            rewrite_min_renames: 5,
            alert_cooldown_secs: 120,
            max_tracked_events: 4096,
        }
    }
}

// -- engine -----------------------------------------------------------

#[derive(Debug)]
pub struct CanaryEngine {
    cfg: CanaryConfig,
    recent: VecDeque<FileEvent>,
    rewrites: VecDeque<(u64, String, String)>,
    cooldowns: HashMap<String, u64>,
}

impl CanaryEngine {
    pub fn new(cfg: CanaryConfig) -> Self {
        Self {
            cfg,
            recent: VecDeque::new(),
            rewrites: VecDeque::new(),
            cooldowns: HashMap::new(),
        }
    }

    pub fn observe(&mut self, ev: FileEvent) -> Vec<CanaryAlert> {
        let mut alerts = Vec::new();
        self.prune(ev.at_secs);

        if is_canary_decoy(&ev.name) && ev.kind != FileEventKind::Added {
            let action = match ev.kind {
                FileEventKind::Removed => "deleted",
                FileEventKind::Modified => "modified",
                FileEventKind::RenamedOldName | FileEventKind::RenamedNewName => "renamed",
                FileEventKind::Added => unreachable!(),
            };
            let alert = CanaryAlert::CanaryTamper {
                folder: ev.folder.clone(),
                file: ev.name.clone(),
                action,
                at_secs: ev.at_secs,
            };
            if !self.on_cooldown(&alert, ev.at_secs) {
                self.arm_cooldown(&alert, ev.at_secs);
                alerts.push(alert);
            }
        }

        if ev.kind.counts_toward_burst() {
            self.recent.push_back(ev.clone());
            while self.recent.len() > self.cfg.max_tracked_events {
                self.recent.pop_front();
            }
            alerts.extend(self.check_burst(&ev));
            if ev.kind == FileEventKind::RenamedNewName {
                if let Some(ext) = extension_of(&ev.name) {
                    self.rewrites
                        .push_back((ev.at_secs, ev.folder.clone(), ext));
                    while self.rewrites.len() > self.cfg.max_tracked_events {
                        self.rewrites.pop_front();
                    }
                    alerts.extend(self.check_rewrite(&ev));
                }
            }
        }

        alerts
    }

    fn check_burst(&mut self, ev: &FileEvent) -> Vec<CanaryAlert> {
        let mut seen = std::collections::HashSet::new();
        for e in self.recent.iter().filter(|e| {
            e.folder == ev.folder && e.at_secs + self.cfg.burst_window_secs > ev.at_secs
        }) {
            seen.insert(e.name.clone());
        }
        if seen.len() < self.cfg.burst_min_files {
            return Vec::new();
        }
        let alert = CanaryAlert::BurstEncryption {
            folder: ev.folder.clone(),
            distinct_files: seen.len(),
            window_secs: self.cfg.burst_window_secs,
            at_secs: ev.at_secs,
        };
        if self.on_cooldown(&alert, ev.at_secs) {
            return Vec::new();
        }
        self.arm_cooldown(&alert, ev.at_secs);
        vec![alert]
    }

    fn check_rewrite(&mut self, ev: &FileEvent) -> Vec<CanaryAlert> {
        let mut per_ext: HashMap<String, usize> = HashMap::new();
        for (_at, _folder, ext) in self.rewrites.iter().filter(|(at, folder, _)| {
            *at + self.cfg.burst_window_secs > ev.at_secs && *folder == ev.folder
        }) {
            *per_ext.entry(ext.clone()).or_insert(0) += 1;
        }
        let Some((best_ext, count)) = per_ext.into_iter().max_by_key(|(_, n)| *n) else {
            return Vec::new();
        };
        if count < self.cfg.rewrite_min_renames || cure_core_boring_ext(&best_ext) {
            return Vec::new();
        }
        let alert = CanaryAlert::ExtensionRewrite {
            folder: ev.folder.clone(),
            extension: best_ext,
            renamed_count: count,
            at_secs: ev.at_secs,
        };
        if self.on_cooldown(&alert, ev.at_secs) {
            return Vec::new();
        }
        self.arm_cooldown(&alert, ev.at_secs);
        vec![alert]
    }

    fn prune(&mut self, now: u64) {
        let keep_from = now.saturating_sub(self.cfg.burst_window_secs * 2);
        while self.recent.front().map(|e| e.at_secs < keep_from) == Some(true) {
            self.recent.pop_front();
        }
        while self.rewrites.front().map(|(at, _, _)| *at < keep_from) == Some(true) {
            self.rewrites.pop_front();
        }
    }

    fn on_cooldown(&self, alert: &CanaryAlert, now: u64) -> bool {
        self.cooldowns
            .get(&alert.cooldown_key())
            .map(|&last| now < last.saturating_add(self.cfg.alert_cooldown_secs))
            .unwrap_or(false)
    }

    fn arm_cooldown(&mut self, alert: &CanaryAlert, now: u64) {
        self.cooldowns.insert(alert.cooldown_key(), now);
    }
}

fn extension_of(name: &str) -> Option<String> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    if ext.is_empty() {
        None
    } else {
        Some(ext)
    }
}

fn cure_core_boring_ext(ext: &str) -> bool {
    crate::ransom_detect::is_boring_extension(ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(at: u64, folder: &str, name: &str, kind: FileEventKind) -> FileEvent {
        FileEvent { at_secs: at, folder: folder.to_string(), name: name.to_string(), kind }
    }
    fn eng() -> CanaryEngine { CanaryEngine::new(CanaryConfig::default()) }
    fn feed(e: &mut CanaryEngine, evs: Vec<FileEvent>) -> Vec<CanaryAlert> {
        evs.into_iter().flat_map(|x| e.observe(x)).collect()
    }

    #[test]
    fn decoy_detection_case_insensitive() {
        assert!(is_canary_decoy("~cure-canary-report.docx"));
        assert!(is_canary_decoy("~CURE-CANARY-INVOICE.XLSX"));
        assert!(!is_canary_decoy("vacation-photo.jpg"));
    }

    #[test]
    fn decoy_names_cycle_with_suffixes() {
        let names = decoy_names(DECOY_TEMPLATES.len() + 2);
        assert_eq!(names.len(), DECOY_TEMPLATES.len() + 2);
        assert_eq!(names[0], DECOY_TEMPLATES[0]);
        assert!(names[DECOY_TEMPLATES.len()].starts_with("~cure-canary-"));
        for n in &names { assert!(is_canary_decoy(n)); }
    }

    #[test]
    fn tamper_fires_on_modify_rename_and_delete() {
        for kind in [FileEventKind::Modified, FileEventKind::Removed,
                     FileEventKind::RenamedNewName, FileEventKind::RenamedOldName] {
            let mut e = eng();
            let a = e.observe(ev(100, r"C:\Docs", DECOY_TEMPLATES[0], kind));
            assert_eq!(a.len(), 1, "expected tamper for {kind:?}");
            assert_eq!(a[0].severity(), 3);
        }
    }

    #[test]
    fn decoy_creation_does_not_fire() {
        let mut e = eng();
        assert!(e.observe(ev(100, r"C:\Docs", DECOY_TEMPLATES[0], FileEventKind::Added)).is_empty());
    }

    #[test]
    fn tamper_respects_cooldown() {
        let mut e = eng();
        assert_eq!(e.observe(ev(100, r"C:\Docs", DECOY_TEMPLATES[0], FileEventKind::Modified)).len(), 1);
        assert!(e.observe(ev(150, r"C:\Docs", DECOY_TEMPLATES[1], FileEventKind::Modified)).is_empty());
        assert_eq!(e.observe(ev(221, r"C:\Docs", DECOY_TEMPLATES[2], FileEventKind::Modified)).len(), 1);
    }

    #[test]
    fn single_file_change_never_alerts() {
        let mut e = eng();
        assert!(feed(&mut e, vec![ev(10, r"C:\Docs", "thesis.docx", FileEventKind::Modified)]).is_empty());
    }

    #[test]
    fn burst_below_threshold_is_quiet() {
        let mut e = eng();
        let evs: Vec<FileEvent> = (0..7).map(|i| ev(i, r"C:\Docs", &format!("f{i}.txt"), FileEventKind::Modified)).collect();
        assert!(feed(&mut e, evs).is_empty());
    }

    #[test]
    fn burst_at_threshold_fires_once() {
        let mut e = eng();
        let mut evs: Vec<FileEvent> = (0..8).map(|i| ev(i, r"C:\Docs", &format!("f{i}.txt"), FileEventKind::Modified)).collect();
        evs.push(ev(9, r"C:\Docs", "extra.bin", FileEventKind::Modified));
        let a = feed(&mut e, evs);
        assert_eq!(a.len(), 1);
        if let CanaryAlert::BurstEncryption { distinct_files, .. } = &a[0] { assert_eq!(*distinct_files, 8); } else { panic!("wrong"); }
    }

    #[test]
    fn burst_counts_distinct_files_not_repeats() {
        let mut e = eng();
        let evs: Vec<FileEvent> = (0..12).map(|i| ev(i % 3, r"C:\Docs", "same.log", FileEventKind::Modified)).collect();
        assert!(feed(&mut e, evs).is_empty());
    }

    #[test]
    fn burst_respects_sliding_window() {
        let mut e = eng();
        let mut evs: Vec<FileEvent> = (0..7).map(|i| ev(i, r"C:\Docs", &format!("f{i}.txt"), FileEventKind::Modified)).collect();
        evs.push(ev(500, r"C:\Docs", "late.txt", FileEventKind::Modified));
        assert!(feed(&mut e, evs).is_empty());
        let late: Vec<FileEvent> = (600..608).map(|t| ev(t, r"C:\Docs", &format!("g{}", t-600), FileEventKind::Modified)).collect();
        assert_eq!(feed(&mut e, late).len(), 1);
    }

    #[test]
    fn bursts_are_per_folder() {
        let mut e = eng();
        let a: Vec<FileEvent> = (0..8).map(|i| ev(i, r"C:\A", &format!("f{i}.txt"), FileEventKind::Modified)).collect();
        assert_eq!(feed(&mut e, a).len(), 1);
        let b: Vec<FileEvent> = (0..8).map(|i| ev(i, r"C:\B", &format!("f{i}.txt"), FileEventKind::Modified)).collect();
        assert_eq!(feed(&mut e, b).len(), 1);
    }

    #[test]
    fn burst_cooldown_suppresses_then_recovers() {
        let mut e = eng();
        let mk = |off: u64, pfx: &str| -> Vec<FileEvent> {
            (0..8).map(|i| ev(off+i, r"C:\Docs", &format!("{pfx}{i}.txt"), FileEventKind::Modified)).collect()
        };
        assert_eq!(feed(&mut e, mk(0, "a")).len(), 1);
        assert!(feed(&mut e, mk(20, "b")).is_empty());
        assert_eq!(feed(&mut e, mk(200, "c")).len(), 1);
    }

    #[test]
    fn rewrite_below_threshold_is_quiet() {
        let mut e = eng();
        let evs: Vec<FileEvent> = (0..4).map(|i| ev(i, r"C:\Docs", &format!("f{i}.locked"), FileEventKind::RenamedNewName)).collect();
        assert!(feed(&mut e, evs).is_empty());
    }

    #[test]
    fn rewrite_onto_boring_extension_never_fires() {
        let mut e = eng();
        let evs: Vec<FileEvent> = (0..4).map(|i| ev(i, r"C:\Docs", &format!("f{i}.txt"), FileEventKind::RenamedNewName)).collect();
        assert!(feed(&mut e, evs).is_empty());
    }

    #[test]
    fn rewrite_reports_dominant_uncommon_extension() {
        let mut e = eng();
        let mut evs: Vec<FileEvent> = (0..5).map(|i| ev(i, r"C:\Docs", &format!("f{i}.lockbit"), FileEventKind::RenamedNewName)).collect();
        evs.push(ev(5, r"C:\Docs", "decoy.weird", FileEventKind::RenamedNewName));
        let a = feed(&mut e, evs);
        assert_eq!(a.len(), 1);
        if let CanaryAlert::ExtensionRewrite { extension, renamed_count, .. } = &a[0] {
            assert_eq!(extension, "lockbit");
            assert_eq!(*renamed_count, 5);
        } else { panic!("wrong"); }
    }

    #[test]
    fn rewrite_and_burst_cool_down_independently() {
        let mut e = eng();
        let mut evs: Vec<FileEvent> = (0..5).map(|i| ev(i, r"C:\Work", &format!("f{i}.crypt"), FileEventKind::RenamedNewName)).collect();
        evs.extend((0..8).map(|i| ev(i, r"C:\Work", &format!("m{i}.txt"), FileEventKind::Modified)));
        assert_eq!(feed(&mut e, evs).len(), 2);
    }

    #[test]
    fn tiny_ring_buffer_drops_old_events() {
        let mut e = CanaryEngine::new(CanaryConfig { burst_min_files: 3, burst_window_secs: 30, rewrite_min_renames: 3, alert_cooldown_secs: 120, max_tracked_events: 4 });
        let old: Vec<FileEvent> = (0..50).map(|i| ev(i * 100, r"C:\X", &format!("old{i}.tmp"), FileEventKind::Modified)).collect();
        assert!(feed(&mut e, old).is_empty());
        let nc: Vec<FileEvent> = (0..2).map(|i| ev(10000+i, r"C:\X", &format!("n{i}.bin"), FileEventKind::Modified)).collect();
        assert!(feed(&mut e, nc).is_empty());
        assert_eq!(feed(&mut e, vec![ev(10002, r"C:\X", "z.bin", FileEventKind::Modified)]).len(), 1);
    }

    #[test]
    fn shadow_wipe_matches_known_patterns() {
        let re = shadow_wipe_reason;
        assert!(re("vssadmin.exe", &["delete ", "shadows"].concat()).is_some());
        assert!(re("diskshadow.exe", &["delete ", "shadows"].concat()).is_some());
        assert!(re("wbadmin.exe", &["delete ", "catalog"].concat()).is_some());
        assert!(re("bcdedit.exe", &["recoveryenabled ", "no"].concat()).is_some());
        assert!(re("wmic.exe", &["shadowcopy ", "delete"].concat()).is_some());
    }

    #[test]
    fn benign_process_never_trips_shadow_wipe() {
        assert!(shadow_wipe_reason("explorer.exe", "").is_none());
        assert!(shadow_wipe_reason("vssadmin.exe", "").is_none());
        assert!(shadow_wipe_reason("vssadmin.exe", "vssadmin list shadows").is_none());
        assert!(shadow_wipe_reason("notepad.exe", "delete shadows").is_none());
    }

    #[test]
    fn boring_extensions_shared_with_ransom_module() {
        assert!(cure_core_boring_ext("txt"));
        assert!(cure_core_boring_ext("exe"));
        assert!(!cure_core_boring_ext("lockbit"));
        assert!(crate::ransom_detect::is_boring_extension("JPEG"));
    }
}
