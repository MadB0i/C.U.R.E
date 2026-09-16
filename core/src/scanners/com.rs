//! Read-only per-user COM hijack audit.
//!
//! `HKCU\Software\Classes\CLSID\{...}\InprocServer32` (and `TreatAs`)
//! redirections load attacker DLLs into legitimate processes — a standard
//! persistence technique (ATT&CK T1546.015) that also fires right after
//! login when Explorer COM servers resolve. Only the per-user hive is
//! walked: HKLM is admin-owned, machine-wide, and far too large to score
//! per-scan; that boundary is documented, not silent.
//!
//! Detection only; values are never modified.

use crate::model::{PersistenceEntry, PersistenceSource};

pub const HKCU_CLASSES_CLSID: &str = r"Software\Classes\CLSID";

pub fn key_path(clsid: &str) -> String {
    format!(r"HKCU\{HKCU_CLASSES_CLSID}\{clsid}")
}

#[cfg(windows)]
pub fn scan() -> Vec<PersistenceEntry> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let mut entries = Vec::new();
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(HKCU_CLASSES_CLSID) else {
        return entries;
    };
    let subkeys: Vec<String> = root.enum_keys().filter_map(Result::ok).collect();
    for clsid in subkeys {
        let Ok(clsid_key) = root.open_subkey(&clsid) else {
            continue;
        };
        // TreatAs redirection first — it reroutes the whole class.
        if let Ok(treat_as) = clsid_key.open_subkey("TreatAs") {
            let target: String = treat_as.get_value("").unwrap_or_default();
            if !target.trim().is_empty() {
                entries.push(PersistenceEntry::new(
                    PersistenceSource::ComHijack,
                    format!("{clsid} TreatAs"),
                    target.trim(),
                    format!("{}\\TreatAs", key_path(&clsid)),
                ));
                continue;
            }
        }
        let Ok(server_key) = clsid_key.open_subkey("InprocServer32") else {
            continue;
        };
        let server: String = server_key.get_value("").unwrap_or_default();
        if server.trim().is_empty() {
            continue;
        }
        entries.push(PersistenceEntry::new(
            PersistenceSource::ComHijack,
            format!("{clsid} InprocServer32"),
            server.trim(),
            format!("{}\\InprocServer32", key_path(&clsid)),
        ));
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

#[cfg(not(windows))]
pub fn scan() -> Vec<PersistenceEntry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_path_shape() {
        assert_eq!(
            key_path("{00000000-0000-0000-0000-000000000000}"),
            r"HKCU\Software\Classes\CLSID\{00000000-0000-0000-0000-000000000000}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn scan_never_panics_and_reports_strings() {
        for e in scan() {
            assert_eq!(e.source, PersistenceSource::ComHijack);
            assert!(!e.command.trim().is_empty());
            assert!(e.location.starts_with(r"HKCU\Software\Classes\CLSID\"));
        }
    }
}
