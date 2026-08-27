//! MITRE ATT&CK technique mapping for persistence scanners.
//!
//! Maps each C.U.R.E scanner source to its corresponding ATT&CK technique
//! ID and name, enabling security analysts to understand the threat context
//! of every persistence entry the tool discovers.

use serde::{Deserialize, Serialize};

/// A MITRE ATT&CK technique reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackTechnique {
    pub id: &'static str,
    pub name: &'static str,
    pub tactic: &'static str,
    pub url: &'static str,
}

/// Known ATT&CK techniques relevant to Windows persistence mechanisms.
pub const TECHNIQUE_STARTUP_FOLDER: AttackTechnique = AttackTechnique {
    id: "T1547.001",
    name: "Boot or Logon Autostart Execution: Startup Folder",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1547/001/",
};

pub const TECHNIQUE_REGISTRY_RUN: AttackTechnique = AttackTechnique {
    id: "T1547.001",
    name: "Boot or Logon Autostart Execution: Registry Run Keys",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1547/001/",
};

pub const TECHNIQUE_SCHEDULED_TASK: AttackTechnique = AttackTechnique {
    id: "T1053.005",
    name: "Scheduled Task/Job: Scheduled Task",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1053/005/",
};

/// Maps a scanner source identifier to its ATT&CK technique.
pub fn technique_for_source(source: &str) -> Option<&'static AttackTechnique> {
    match source {
        "StartupFolder" => Some(&TECHNIQUE_STARTUP_FOLDER),
        "RegistryRun" => Some(&TECHNIQUE_REGISTRY_RUN),
        "ScheduledTask" => Some(&TECHNIQUE_SCHEDULED_TASK),
        _ => None,
    }
}

/// Returns the technique ID string for a scanner source, or an empty string.
pub fn technique_id_for(source: &str) -> &'static str {
    technique_for_source(source)
        .map(|t| t.id)
        .unwrap_or("")
}

/// Returns the technique name for a scanner source, or an empty string.
pub fn technique_name_for(source: &str) -> &'static str {
    technique_for_source(source)
        .map(|t| t.name)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_folder_maps_to_t1547_001() {
        let t = technique_for_source("StartupFolder").unwrap();
        assert_eq!(t.id, "T1547.001");
        assert!(t.name.contains("Startup Folder"));
        assert_eq!(t.tactic, "Persistence");
    }

    #[test]
    fn registry_run_maps_to_t1547_001() {
        let t = technique_for_source("RegistryRun").unwrap();
        assert_eq!(t.id, "T1547.001");
        assert!(t.name.contains("Registry Run Keys"));
    }

    #[test]
    fn scheduled_task_maps_to_t1053_005() {
        let t = technique_for_source("ScheduledTask").unwrap();
        assert_eq!(t.id, "T1053.005");
        assert!(t.name.contains("Scheduled Task"));
    }

    #[test]
    fn unknown_source_returns_none() {
        assert!(technique_for_source("UnknownSource").is_none());
    }

    #[test]
    fn technique_id_returns_id_or_empty() {
        assert_eq!(technique_id_for("StartupFolder"), "T1547.001");
        assert_eq!(technique_id_for("Bogus"), "");
    }

    #[test]
    fn technique_name_returns_name_or_empty() {
        assert!(!technique_name_for("ScheduledTask").is_empty());
        assert_eq!(technique_name_for("Bogus"), "");
    }

    #[test]
    fn all_techniques_have_valid_urls() {
        let techniques = [
            &TECHNIQUE_STARTUP_FOLDER,
            &TECHNIQUE_REGISTRY_RUN,
            &TECHNIQUE_SCHEDULED_TASK,
        ];
        for t in &techniques {
            assert!(t.url.starts_with("https://attack.mitre.org/techniques/"));
        }
    }

    #[test]
    fn all_techniques_serialize() {
        let json = serde_json::to_string(&TECHNIQUE_STARTUP_FOLDER).unwrap();
        assert!(json.contains("T1547.001"));
    }
}
