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
        fs::write(dir.path().join("legit-update.bat"), "@echo off\r\nrem ok\r\n").unwrap();
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
}
