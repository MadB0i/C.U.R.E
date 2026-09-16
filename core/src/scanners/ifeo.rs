//! Read-only IFEO debugger audit.
//!
//! `HKLM\...\Image File Execution Options\<exe>` with a `Debugger` value
//! redirects every launch of `<exe>` into the debugger command — a classic
//! persistence/defense-evasion hook and a prime suspect when odd windows
//! appear right after login. Detection only; values are never modified.
//!
//! A clean machine normally has no `Debugger` values here at all, so any
//! finding deserves review — but review is not a verdict: accessibility
//! tools and some AV products use this key legitimately.

use crate::model::{PersistenceEntry, PersistenceSource};

pub const IFEO_ROOT: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";

pub fn key_path(exe_name: &str) -> String {
    format!(r"HKLM\{IFEO_ROOT}\{exe_name}")
}

#[cfg(windows)]
pub fn scan() -> Vec<PersistenceEntry> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let mut entries = Vec::new();
    let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(IFEO_ROOT) else {
        return entries; // missing/unreadable: no data, not an error
    };
    let subkeys: Vec<String> = root.enum_keys().filter_map(Result::ok).collect();
    for sub in subkeys {
        let Ok(key) = root.open_subkey(&sub) else {
            continue;
        };
        let Ok(debugger): Result<String, _> = key.get_value("Debugger") else {
            continue;
        };
        if debugger.trim().is_empty() {
            continue;
        }
        entries.push(PersistenceEntry::new(
            PersistenceSource::IfeoDebugger,
            &sub,
            debugger.trim(),
            key_path(&sub),
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
            key_path("notepad.exe"),
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\notepad.exe"
        );
    }

    #[cfg(windows)]
    #[test]
    fn scan_never_panics_and_reports_strings() {
        // Live registry read; asserts shape only, never contents.
        for e in scan() {
            assert_eq!(e.source, PersistenceSource::IfeoDebugger);
            assert!(!e.command.trim().is_empty());
            assert!(e.location.starts_with(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\"));
        }
    }
}
