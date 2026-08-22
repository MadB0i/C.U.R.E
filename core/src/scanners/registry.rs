use std::io;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use crate::model::{PersistenceEntry, PersistenceSource};

const AUTORUN_SUBKEYS: [&str; 2] = [
    r"Software\Microsoft\Windows\CurrentVersion\Run",
    r"Software\Microsoft\Windows\CurrentVersion\RunOnce",
];

pub fn scan() -> io::Result<Vec<PersistenceEntry>> {
    let mut entries = Vec::new();
    let scopes = [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")];
    for (hive, hive_label) in scopes {
        for subkey in AUTORUN_SUBKEYS {
            let key = match RegKey::predef(hive).open_subkey(subkey) {
                Ok(key) => key,
                Err(_) => continue,
            };
            for value_name in key.enum_values().flatten().map(|(name, _)| name) {
                let Ok(command): Result<String, _> = key.get_value(&value_name) else {
                    continue;
                };
                if command.trim().is_empty() {
                    continue;
                }
                entries.push(PersistenceEntry::new(
                    PersistenceSource::RegistryRun,
                    &value_name,
                    &command,
                    format!("{hive_label}\\{subkey}"),
                ));
            }
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_completes_without_panicking() {
        let _ = super::scan();
    }
}
