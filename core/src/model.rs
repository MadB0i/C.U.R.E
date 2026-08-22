use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PersistenceSource {
    RegistryRun,
    StartupFolder,
    ScheduledTask,
}

impl PersistenceSource {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::RegistryRun => "registry-run",
            Self::StartupFolder => "startup-folder",
            Self::ScheduledTask => "scheduled-task",
        }
    }
}

impl fmt::Display for PersistenceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceEntry {
    pub id: String,
    pub source: PersistenceSource,
    pub name: String,
    pub command: String,
    pub location: String,
}

impl PersistenceEntry {
    pub fn new(
        source: PersistenceSource,
        name: impl Into<String>,
        command: impl Into<String>,
        location: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let command = command.into();
        let id = make_id(&source, &name, &command);
        Self {
            id,
            source,
            name,
            command,
            location: location.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    Safe,
    Suspicious,
    HighRisk,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Safe => "SAFE      ",
            Self::Suspicious => "SUSPICIOUS",
            Self::HighRisk => "HIGH-RISK ",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoredEntry {
    pub entry: PersistenceEntry,
    pub score: i32,
    pub risk: RiskLevel,
    pub reasons: Vec<String>,
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const SEPARATOR: u8 = 0x1f;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub fn make_id(source: &PersistenceSource, name: &str, command: &str) -> String {
    let mut material = Vec::with_capacity(name.len() + command.len() + 24);
    material.extend_from_slice(source.tag().as_bytes());
    material.push(SEPARATOR);
    material.extend_from_slice(name.as_bytes());
    material.push(SEPARATOR);
    material.extend_from_slice(command.as_bytes());
    format!("{:016x}", fnv1a(&material))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_and_hex() {
        let a = make_id(&PersistenceSource::StartupFolder, "update.bat", "C:\\Temp\\update.bat");
        let b = make_id(&PersistenceSource::StartupFolder, "update.bat", "C:\\Temp\\update.bat");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn id_changes_when_any_field_changes() {
        let base = make_id(&PersistenceSource::StartupFolder, "x", "y");
        assert_ne!(base, make_id(&PersistenceSource::ScheduledTask, "x", "y"));
        assert_ne!(base, make_id(&PersistenceSource::StartupFolder, "z", "y"));
        assert_ne!(base, make_id(&PersistenceSource::StartupFolder, "x", "z"));
    }

    #[test]
    fn constructor_sets_matching_id() {
        let entry = PersistenceEntry::new(
            PersistenceSource::RegistryRun,
            "Sidecar",
            "C:\\sidecar.exe",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        );
        assert_eq!(
            entry.id,
            make_id(&PersistenceSource::RegistryRun, "Sidecar", "C:\\sidecar.exe")
        );
    }
}
