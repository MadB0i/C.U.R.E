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

pub const TECHNIQUE_WINDOWS_SERVICE: AttackTechnique = AttackTechnique {
    id: "T1543.003",
    name: "Create or Modify System Process: Windows Service",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1543/003/",
};

pub const TECHNIQUE_WMI_EVENT: AttackTechnique = AttackTechnique {
    id: "T1546.003",
    name: "Event Triggered Execution: Windows Management Instrumentation Event Subscription",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1546/003/",
};

pub const TECHNIQUE_IFEO: AttackTechnique = AttackTechnique {
    id: "T1546.012",
    name: "Event Triggered Execution: Image File Execution Options Injection",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1546/012/",
};

pub const TECHNIQUE_APPINIT: AttackTechnique = AttackTechnique {
    id: "T1546.010",
    name: "Event Triggered Execution: AppInit DLLs",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1546/010/",
};

pub const TECHNIQUE_COM_HIJACK: AttackTechnique = AttackTechnique {
    id: "T1546.015",
    name: "Event Triggered Execution: Component Object Model Hijacking",
    tactic: "Persistence",
    url: "https://attack.mitre.org/techniques/T1546/015/",
};

/// Typed entry point — prefer this over the string form so new
/// [`PersistenceSource`](crate::model::PersistenceSource) variants are a
/// compile error here until mapped.
pub fn technique_for(source: &crate::model::PersistenceSource) -> Option<&'static AttackTechnique> {
    use crate::model::PersistenceSource::*;
    Some(match source {
        StartupFolder => &TECHNIQUE_STARTUP_FOLDER,
        RegistryRun => &TECHNIQUE_REGISTRY_RUN,
        ScheduledTask => &TECHNIQUE_SCHEDULED_TASK,
        WindowsService => &TECHNIQUE_WINDOWS_SERVICE,
        WmiSubscription => &TECHNIQUE_WMI_EVENT,
        IfeoDebugger => &TECHNIQUE_IFEO,
        AppInitDlls => &TECHNIQUE_APPINIT,
        ComHijack => &TECHNIQUE_COM_HIJACK,
    })
}

/// String entry point for serialized payloads and UI tags. Accepts both
/// serialized variant names (`"WindowsService"`, as produced by serde) and
/// [`PersistenceSource`](crate::model::PersistenceSource) tag strings
/// (`"windows-service"`). Unknown strings map to `None` — callers must not
/// display an ATT&CK ID in that case.
pub fn technique_for_source(source: &str) -> Option<&'static AttackTechnique> {
    match source {
        "StartupFolder" | "startup-folder" => Some(&TECHNIQUE_STARTUP_FOLDER),
        "RegistryRun" | "registry-run" => Some(&TECHNIQUE_REGISTRY_RUN),
        "ScheduledTask" | "scheduled-task" => Some(&TECHNIQUE_SCHEDULED_TASK),
        "WindowsService" | "windows-service" => Some(&TECHNIQUE_WINDOWS_SERVICE),
        "WmiSubscription" | "wmi-subscription" => Some(&TECHNIQUE_WMI_EVENT),
        "IfeoDebugger" | "ifeo-debugger" => Some(&TECHNIQUE_IFEO),
        "AppInitDlls" | "appinit-dlls" => Some(&TECHNIQUE_APPINIT),
        "ComHijack" | "com-hijack" => Some(&TECHNIQUE_COM_HIJACK),
        _ => None,
    }
}

/// Returns the technique ID string for a scanner source, or an empty string.
pub fn technique_id_for(source: &str) -> &'static str {
    technique_for_source(source).map(|t| t.id).unwrap_or("")
}

/// Returns the technique name for a scanner source, or an empty string.
pub fn technique_name_for(source: &str) -> &'static str {
    technique_for_source(source).map(|t| t.name).unwrap_or("")
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
