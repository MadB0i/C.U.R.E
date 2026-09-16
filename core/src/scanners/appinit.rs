//! Read-only AppInit_DLLs audit.
//!
//! DLLS listed in `HKLM\...\Windows\AppInit_DLLs` load into every process
//! that links user32.dll when `LoadAppInit_DLLs` is set — a well-known
//! persistence hook. Detection only; values are never modified.
//!
//! Honest caveat (surfaced in reasons by the scorer via the raw values):
//! on modern Windows only signed DLLs load here and the mechanism is off
//! by default (`LoadAppInit_DLLs = 0`, Secure Boot). An entry is evidence,
//! not a verdict.

use crate::model::{PersistenceEntry, PersistenceSource};

pub const APPINIT_KEY: &str = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Windows";

#[cfg(windows)]
pub fn scan() -> Vec<PersistenceEntry> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    const SUBKEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Windows";
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(SUBKEY) else {
        return Vec::new();
    };
    let dlls: String = key.get_value("AppInit_DLLs").unwrap_or_default();
    let dlls = dlls.trim();
    if dlls.is_empty() {
        return Vec::new();
    }
    let load: u32 = key.get_value("LoadAppInit_DLLs").unwrap_or(0);
    let name = if load == 0 {
        "AppInit_DLLs (mechanism disabled)"
    } else {
        "AppInit_DLLs"
    };
    vec![PersistenceEntry::new(
        PersistenceSource::AppInitDlls,
        name,
        dlls,
        APPINIT_KEY.to_string(),
    )]
}

#[cfg(not(windows))]
pub fn scan() -> Vec<PersistenceEntry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_constant_shape() {
        assert!(APPINIT_KEY.contains(r"CurrentVersion\Windows"));
    }

    #[cfg(windows)]
    #[test]
    fn scan_never_panics() {
        for e in scan() {
            assert_eq!(e.source, PersistenceSource::AppInitDlls);
        }
    }
}
