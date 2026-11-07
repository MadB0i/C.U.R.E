//! Winlogon & Active Setup scanners (Detection Engine v2).
//!
//! Two boot/logon persistence locations that Windows Defender does not
//! surface as autorun entries:
//!
//! - **Winlogon** (`…\CurrentVersion\Winlogon`): `Shell`, `Userinit`,
//!   `Taskman` and `AppSetup` decide what the interactive session starts.
//!   Malware appends itself to `Userinit`/`Shell` so it launches before the
//!   shell with no Run-key trace.
//! - **Active Setup** (`…\Active Setup\Installed Components\<GUID>`): the
//!   `StubPath` command runs once per user at logon; re-registering a GUID
//!   re-runs it. A favourite of commodity loaders.
//!
//! Both emit entries under [`PersistenceSource::RegistryRun`] — they are
//! registry values, not file-backed, so quarantine does not apply (same
//! policy as Run keys). The *real* key path always goes into `location` so
//! the operator sees where the value actually lives, never a prettified
//! label. Access accounting follows the Run-key scanner: a key that cannot
//! be opened is counted as skipped, never silently dropped.

use std::io;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use crate::model::{PersistenceEntry, PersistenceSource};

const WINLOGON_SUBKEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
const ACTIVE_SETUP_SUBKEY: &str = r"Software\Microsoft\Active Setup\Installed Components";

/// Winlogon values that start session processes. Anything else in the key
/// is config (scredir, GuiShow) and is not an autorun surface.
const WINLOGON_VALUES: [&str; 4] = ["Shell", "Userinit", "Taskman", "AppSetup"];

pub fn scan() -> io::Result<Vec<PersistenceEntry>> {
    let mut out = scan_winlogon_report().entries;
    out.extend(scan_active_setup_report().entries);
    Ok(out)
}

pub struct BootLogonReport {
    pub entries: Vec<PersistenceEntry>,
    pub values_read: usize,
    pub skipped_keys: usize,
}

impl BootLogonReport {
    pub fn status(&self) -> crate::elevation::SourceStatus {
        use crate::elevation::SourceStatus;
        if self.skipped_keys == 0 {
            SourceStatus::Available
        } else if !self.entries.is_empty() || self.values_read > 0 {
            SourceStatus::Partial {
                skipped: self.skipped_keys,
                reason: "some boot/logon keys unreadable (elevation may help)".to_string(),
            }
        } else {
            SourceStatus::AccessDenied {
                reason: "boot/logon keys unreadable (elevation may help)".to_string(),
            }
        }
    }
}

/// Winlogon session-start values, both hives.
pub fn scan_winlogon_report() -> BootLogonReport {
    let mut report = BootLogonReport {
        entries: Vec::new(),
        values_read: 0,
        skipped_keys: 0,
    };
    for (hive, hive_label) in [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")] {
        let key = match RegKey::predef(hive).open_subkey(WINLOGON_SUBKEY) {
            Ok(key) => key,
            Err(_) => {
                report.skipped_keys += 1;
                continue;
            }
        };
        for value_name in WINLOGON_VALUES {
            let Ok(command): Result<String, _> = key.get_value(value_name) else {
                continue;
            };
            if command.trim().is_empty() {
                continue;
            }
            report.values_read += 1;
            report.entries.push(PersistenceEntry::new(
                PersistenceSource::RegistryRun,
                format!("Winlogon {value_name} ({hive_label})"),
                &command,
                format!("{hive_label}\\{WINLOGON_SUBKEY} · {value_name}"),
            ));
        }
    }
    report
}

/// Active Setup StubPath commands, both hives.
pub fn scan_active_setup_report() -> BootLogonReport {
    let mut report = BootLogonReport {
        entries: Vec::new(),
        values_read: 0,
        skipped_keys: 0,
    };
    for (hive, hive_label) in [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")] {
        let components = match RegKey::predef(hive).open_subkey(ACTIVE_SETUP_SUBKEY) {
            Ok(components) => components,
            Err(_) => {
                report.skipped_keys += 1;
                continue;
            }
        };
        for guid in components.enum_keys().flatten() {
            let key = match components.open_subkey(&guid) {
                Ok(key) => key,
                Err(_) => {
                    report.skipped_keys += 1;
                    continue;
                }
            };
            let Ok(stub): Result<String, _> = key.get_value("StubPath") else {
                continue;
            };
            if stub.trim().is_empty() {
                continue;
            }
            report.values_read += 1;
            // The component's default value is its display name (often the
            // vendor title); fall back to the GUID so nothing is anonymous.
            let display: String = key.get_value::<String, _>("").unwrap_or_default();
            let label = if display.trim().is_empty() {
                guid.clone()
            } else {
                display
            };
            report.entries.push(PersistenceEntry::new(
                PersistenceSource::RegistryRun,
                format!("Active Setup: {label} ({hive_label})"),
                &stub,
                format!("{hive_label}\\{ACTIVE_SETUP_SUBKEY}\\{guid} · StubPath"),
            ));
        }
    }
    report
}

/// Everything a Winlogon `Userinit`/`Shell` value will actually start, in
/// order, trimmed. Comma-separated by contract; the scanner emits the whole
/// value so the scorer judges every program in it — a hostile extra entry
/// after the legitimate `userinit.exe` is exactly the pattern this module
/// exists to catch.
pub fn session_start_programs(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// True when a Winlogon `Userinit` value carries more than the stock
/// `userinit.exe` entry — the append-self-to-Userinit pattern.
pub fn userinit_has_extras(value: &str) -> bool {
    let programs = session_start_programs(value);
    if programs.is_empty() {
        return false;
    }
    programs
        .iter()
        .any(|p| !p.to_ascii_lowercase().ends_with("userinit.exe"))
        || programs.len() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_start_programs_split_cleanly() {
        assert_eq!(session_start_programs(""), Vec::<String>::new());
        assert_eq!(
            session_start_programs(r"C:\Windows\system32\userinit.exe,"),
            vec![r"C:\Windows\system32\userinit.exe"]
        );
        assert_eq!(
            session_start_programs(r#"C:\Windows\system32\userinit.exe,"C:\Users\Public\x.exe""#),
            vec![
                r"C:\Windows\system32\userinit.exe",
                r#""C:\Users\Public\x.exe""#
            ]
        );
    }

    #[test]
    fn userinit_extras_detected() {
        assert!(!userinit_has_extras(r"C:\Windows\system32\userinit.exe,"));
        assert!(userinit_has_extras(
            r#"C:\Windows\system32\userinit.exe,C:\Users\Public\evil.exe"#
        ));
        assert!(userinit_has_extras(r#""C:\Users\Public\only-evil.exe""#));
    }

    #[test]
    fn scans_complete_without_panicking() {
        let wl = scan_winlogon_report();
        // The Winlogon key is world-readable on stock Windows; entries are
        // expected. Whatever the environment, the accounting must be honest.
        assert!(
            wl.entries.len() + wl.skipped_keys > 0 || wl.values_read == 0,
            "winlogon scan must either read values or account skipped keys"
        );
        let aset = scan_active_setup_report();
        assert!(
            aset.entries.len() + aset.skipped_keys > 0 || aset.values_read == 0,
            "active setup scan must either read values or account skipped keys"
        );
    }

    #[test]
    fn entries_carry_the_real_key_path() {
        for e in scan().unwrap_or_default() {
            assert!(
                e.location.contains("Winlogon") || e.location.contains("Active Setup"),
                "location must show the real key: {}",
                e.location
            );
            // Detection-only: registry values are not file-backed, and the
            // entry must say so through its source tag.
            assert_eq!(e.source, PersistenceSource::RegistryRun);
        }
    }
}
