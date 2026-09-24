use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{PersistenceEntry, PersistenceSource};

pub fn default_startup_root() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(r"Microsoft\Windows\Start Menu\Programs\Startup")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".config/autostart")
    }
}

/// Machine-wide Startup folder (`%ProgramData%\…\Startup`). `None`
/// off-Windows or when `%ProgramData%` is unset — callers treat that as
/// "no common root" rather than an error.
pub fn default_common_startup_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .map(|base| common_startup_root_from(&base))
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn common_startup_root_from(base: &Path) -> PathBuf {
    base.join(r"Microsoft\Windows\Start Menu\Programs\Startup")
}

/// Scan the machine-wide Startup folder with the same file-only,
/// never-execute treatment as the per-user one. Empty when there is no
/// common root (non-Windows, unset `%ProgramData%`) or it is unreadable.
pub fn scan_common() -> Vec<PersistenceEntry> {
    match default_common_startup_root() {
        Some(root) => scan(&root),
        None => Vec::new(),
    }
}

pub fn scan(root: &Path) -> Vec<PersistenceEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = fs::read_dir(root) else {
        return entries;
    };
    let mut paths: Vec<PathBuf> = read_dir.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let command = path.to_string_lossy().into_owned();
        entries.push(PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            name,
            command.clone(),
            command,
        ));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::make_id;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn lists_files_as_startup_entries() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("legit-update.bat"),
            "@echo off\r\nrem ok\r\n",
        )
        .unwrap();
        fs::write(dir.path().join("a7x9k2p9.cmd"), "start evil.exe").unwrap();
        fs::create_dir(dir.path().join("subfolder")).unwrap();
        fs::write(dir.path().join("subfolder").join("nested.txt"), "skip me").unwrap();

        let entries = scan(dir.path());

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a7x9k2p9.cmd");
        assert_eq!(entries[1].name, "legit-update.bat");
        for e in &entries {
            assert_eq!(e.source, PersistenceSource::StartupFolder);
            assert_eq!(e.command, e.location);
            assert_eq!(
                e.id,
                make_id(&PersistenceSource::StartupFolder, &e.name, &e.command)
            );
        }

        let again = scan(dir.path());
        assert_eq!(entries, again);
    }

    #[test]
    fn missing_directory_yields_no_entries() {
        assert!(scan(Path::new("Z:/definitely/not/here")).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn common_root_lives_under_programdata() {
        let base = Path::new(r"C:\ProgramData");
        assert_eq!(
            common_startup_root_from(base),
            PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Startup")
        );
    }

    #[test]
    fn common_scan_never_panics_and_uses_startup_source() {
        // Live machine-wide folder when present; empty elsewhere. Either
        // way every row is a StartupFolder entry with command == location.
        for e in scan_common() {
            assert_eq!(e.source, PersistenceSource::StartupFolder);
            assert_eq!(e.command, e.location);
        }
    }
}
