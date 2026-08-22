use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{PersistenceEntry, ScoredEntry};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub saved_at: DateTime<Utc>,
    pub entries: Vec<PersistenceEntry>,
}

pub fn save(path: &Path, entries: &[PersistenceEntry]) -> io::Result<Baseline> {
    let baseline = Baseline {
        saved_at: Utc::now(),
        entries: entries.to_vec(),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(&baseline)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, json)?;
    Ok(baseline)
}

pub fn save_baseline(path: &Path, entries: &[PersistenceEntry]) -> io::Result<Baseline> {
    save(path, entries)
}

pub fn load(path: &Path) -> io::Result<Baseline> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub fn diff(current: &[ScoredEntry], baseline: &Baseline) -> Vec<ScoredEntry> {
    let seen: HashSet<&str> = baseline.entries.iter().map(|e| e.id.as_str()).collect();
    current
        .iter()
        .filter(|s| !seen.contains(s.entry.id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PersistenceSource, RiskLevel};
    use tempfile::tempdir;

    fn sample(name: &str) -> PersistenceEntry {
        PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            name,
            format!(r"C:\Temp\{name}"),
            format!(r"C:\Temp\{name}"),
        )
    }

    fn scored(entry: PersistenceEntry, score: i32) -> ScoredEntry {
        ScoredEntry {
            risk: crate::risk::risk_level(score),
            entry,
            score,
            reasons: vec![],
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        let entries = vec![sample("one.bat"), sample("two.cmd")];

        let saved = save(&path, &entries).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded, saved);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].name, "one.bat");
    }

    #[test]
    fn diff_returns_only_new_ids() {
        let dir = tempdir().unwrap();
        let old = save(&dir.path().join("b.json"), &[sample("old.bat")]).unwrap();

        let current = vec![
            scored(sample("old.bat"), 0),
            scored(sample("fresh.exe"), 60),
        ];

        let new_entries = diff(&current, &old);

        assert_eq!(new_entries.len(), 1);
        assert_eq!(new_entries[0].entry.name, "fresh.exe");
        assert_eq!(new_entries[0].risk, RiskLevel::HighRisk);
    }

    #[test]
    fn diff_empty_when_nothing_changed() {
        let dir = tempdir().unwrap();
        let baseline = save(&dir.path().join("b.json"), &[sample("same.bat")]).unwrap();
        let current = vec![scored(sample("same.bat"), 5)];
        assert!(diff(&current, &baseline).is_empty());
    }

    #[test]
    fn loading_missing_baseline_is_not_found() {
        let dir = tempdir().unwrap();
        let err = load(&dir.path().join("nope.json")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
