use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::PersistenceEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub id: String,
    pub name: String,
    pub source: String,
    pub original_path: PathBuf,
    pub quarantine_path: PathBuf,
    pub archived_at: DateTime<Utc>,
}

type Records = HashMap<String, QuarantineRecord>;

fn quarantine_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("quarantine")
}

fn records_path(data_dir: &Path) -> PathBuf {
    quarantine_dir(data_dir).join("records.json")
}

fn load_records(data_dir: &Path) -> io::Result<Records> {
    match fs::read_to_string(records_path(data_dir)) {
        Ok(raw) => {
            serde_json::from_str(&raw).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Records::new()),
        Err(err) => Err(err),
    }
}

fn save_records(data_dir: &Path, records: &Records) -> io::Result<()> {
    fs::create_dir_all(quarantine_dir(data_dir))?;
    let json = serde_json::to_string_pretty(records)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(records_path(data_dir), json)
}

pub fn is_quarantined(data_dir: &Path, id: &str) -> bool {
    load_records(data_dir)
        .map(|records| records.contains_key(id))
        .unwrap_or(false)
}

pub fn list_records(data_dir: &Path) -> Vec<QuarantineRecord> {
    let mut records: Vec<QuarantineRecord> = load_records(data_dir)
        .map(|r| r.into_values().collect())
        .unwrap_or_default();
    records.sort_by(|a, b| a.archived_at.cmp(&b.archived_at));
    records
}

fn move_file(source: &Path, destination: &Path) -> io::Result<()> {
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    fs::copy(source, destination)?;
    let copied_len = fs::metadata(destination)?.len();
    let original_len = fs::metadata(source)?.len();
    if copied_len != original_len {
        let _ = fs::remove_file(destination);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "copy verification failed during move",
        ));
    }
    fs::remove_file(source)
}

pub fn quarantine_entry(data_dir: &Path, entry: &PersistenceEntry) -> io::Result<QuarantineRecord> {
    let original = PathBuf::from(&entry.location);
    if !original.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry file not found: {}", original.display()),
        ));
    }
    fs::create_dir_all(quarantine_dir(data_dir))?;
    let file_name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unnamed.bin".to_string());
    let destination = quarantine_dir(data_dir).join(format!("{}_{}", entry.id, file_name));
    move_file(&original, &destination)?;
    let record = QuarantineRecord {
        id: entry.id.clone(),
        name: entry.name.clone(),
        source: entry.source.tag().to_string(),
        original_path: original,
        quarantine_path: destination,
        archived_at: Utc::now(),
    };
    let mut records = load_records(data_dir)?;
    records.insert(record.id.clone(), record.clone());
    save_records(data_dir, &records)?;
    Ok(record)
}

pub fn quarantine_file(data_dir: &Path, entry: &PersistenceEntry) -> io::Result<QuarantineRecord> {
    quarantine_entry(data_dir, entry)
}

pub fn undo(data_dir: &Path, id: &str) -> io::Result<QuarantineRecord> {
    let mut records = load_records(data_dir)?;
    let Some(record) = records.remove(id) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no quarantine record for id {id}"),
        ));
    };
    match restore(&record) {
        Ok(()) => {
            save_records(data_dir, &records)?;
            Ok(record)
        }
        Err(err) => {
            records.insert(record.id.clone(), record);
            Err(err)
        }
    }
}

fn restore(record: &QuarantineRecord) -> io::Result<()> {
    if !record.quarantine_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("quarantined file is gone: {}", record.quarantine_path.display()),
        ));
    }
    if record.original_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("original path is occupied: {}", record.original_path.display()),
        ));
    }
    if let Some(parent) = record.original_path.parent() {
        fs::create_dir_all(parent)?;
    }
    move_file(&record.quarantine_path, &record.original_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PersistenceSource;
    use tempfile::tempdir;

    fn payload() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice("héllo wörld\r\n".as_bytes());
        bytes.extend((0u8..=255).collect::<Vec<u8>>());
        bytes
    }

    fn setup_entry(dir: &Path, name: &str) -> (PersistenceEntry, PathBuf) {
        let file = dir.join(name);
        fs::write(&file, payload()).unwrap();
        let path_text = file.to_string_lossy().into_owned();
        (
            PersistenceEntry::new(PersistenceSource::StartupFolder, name, &path_text, &path_text),
            file,
        )
    }

    #[test]
    fn quarantine_moves_file_and_writes_metadata() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "a7x9k2p9.exe");

        let record = quarantine_entry(data.path(), &entry).unwrap();

        assert!(!original.exists());
        assert!(record.quarantine_path.is_file());
        assert_eq!(fs::read(&record.quarantine_path).unwrap(), payload());
        assert_eq!(record.original_path, original);
        assert_eq!(record.source, "startup-folder");
        assert!(is_quarantined(data.path(), &entry.id));

        let records = list_records(data.path());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, entry.id);
    }

    #[test]
    fn undo_restores_identical_bytes_and_clears_record() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "keep-me.dat");

        let quarantined = quarantine_entry(data.path(), &entry).unwrap();
        let restored = undo(data.path(), &entry.id).unwrap();

        assert_eq!(restored.id, quarantined.id);
        assert!(original.is_file());
        assert_eq!(fs::read(&original).unwrap(), payload());
        assert!(!quarantined.quarantine_path.exists());
        assert!(!is_quarantined(data.path(), &entry.id));
        assert!(list_records(data.path()).is_empty());
    }

    #[test]
    fn undo_recreates_missing_parent_directories() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let deep = user_land.path().join("gone").join("deep");
        fs::create_dir_all(&deep).unwrap();
        let file = deep.join("task.xml");
        fs::write(&file, b"<Task/>").unwrap();
        let path_text = file.to_string_lossy().into_owned();
        let entry = PersistenceEntry::new(
            PersistenceSource::ScheduledTask,
            "task.xml",
            &path_text,
            &path_text,
        );

        quarantine_entry(data.path(), &entry).unwrap();
        fs::remove_dir_all(user_land.path().join("gone")).unwrap();

        undo(data.path(), &entry.id).unwrap();

        assert_eq!(fs::read(&file).unwrap(), b"<Task/>");
    }

    #[test]
    fn unknown_id_and_double_quarantine_fail_cleanly() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, _original) = setup_entry(user_land.path(), "x.bat");

        let missing = undo(data.path(), "deadbeefdeadbeef").unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);

        quarantine_entry(data.path(), &entry).unwrap();
        let again = quarantine_entry(data.path(), &entry).unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn undo_fails_when_original_still_exists() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "clash.bin");
        let record = quarantine_entry(data.path(), &entry).unwrap();
        fs::write(&original, b"recreated by user").unwrap();

        let err = undo(data.path(), &entry.id).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(record.quarantine_path.is_file());
        assert!(is_quarantined(data.path(), &entry.id));
    }
}
