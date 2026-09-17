use std::io;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use crate::model::{PersistenceEntry, PersistenceSource};

const AUTORUN_SUBKEYS: [&str; 2] = [
    r"Software\Microsoft\Windows\CurrentVersion\Run",
    r"Software\Microsoft\Windows\CurrentVersion\RunOnce",
];

pub fn scan() -> io::Result<Vec<PersistenceEntry>> {
    Ok(scan_report().entries)
}

/// Scan result with access accounting: keys that cannot be opened
/// (elevation/ACL boundary) are counted, never silently dropped.
pub struct RegistryScanReport {
    pub entries: Vec<PersistenceEntry>,
    pub values_read: usize,
    pub skipped_keys: usize,
}

impl RegistryScanReport {
    pub fn status(&self) -> crate::elevation::SourceStatus {
        use crate::elevation::SourceStatus;
        if self.skipped_keys == 0 {
            SourceStatus::Available
        } else if !self.entries.is_empty() || self.values_read > 0 {
            SourceStatus::Partial {
                skipped: self.skipped_keys,
                reason: "some Run keys unreadable (elevation may help)".to_string(),
            }
        } else {
            SourceStatus::AccessDenied {
                reason: "Run keys unreadable (elevation may help)".to_string(),
            }
        }
    }
}

pub fn scan_report() -> RegistryScanReport {
    let mut report = RegistryScanReport {
        entries: Vec::new(),
        values_read: 0,
        skipped_keys: 0,
    };
    let scopes = [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")];
    for (hive, hive_label) in scopes {
        for subkey in AUTORUN_SUBKEYS {
            let key = match RegKey::predef(hive).open_subkey(subkey) {
                Ok(key) => key,
                Err(_) => {
                    report.skipped_keys += 1;
                    continue;
                }
            };
            for value_name in key.enum_values().flatten().map(|(name, _)| name) {
                let Ok(command): Result<String, _> = key.get_value(&value_name) else {
                    continue;
                };
                if command.trim().is_empty() {
                    continue;
                }
                report.values_read += 1;
                report.entries.push(PersistenceEntry::new(
                    PersistenceSource::RegistryRun,
                    &value_name,
                    &command,
                    format!("{hive_label}\\{subkey}"),
                ));
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_completes_without_panicking() {
        let _ = super::scan();
    }
}
