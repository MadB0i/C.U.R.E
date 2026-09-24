//! Crash-safe, ACL-preserving quarantine (P0-4).
//!
//! Protocol (write-ahead intent, committed move):
//! 1. Capture the file's security snapshot (SDDL/attributes/timestamps),
//!    size, and SHA-256; durably store a **Pending** record first (atomic
//!    save: temp file + flush + fsync + replacing rename).
//! 2. Move the bytes (`MoveFileExW` with `COPY_ALLOWED | WRITE_THROUGH` on
//!    Windows; `rename` elsewhere; verified copy+delete fallback).
//! 3. Flip the record to **Committed** (atomic save again).
//!
//! Crash windows and their healing (see [`reconcile`], run at the start of
//! every quarantine/undo call):
//! - crash before step 1 completes → torn `records.json` impossible (atomic
//!   replace); at worst the previous generation survives.
//! - crash between 1 and 2 → Pending with the original still in place:
//!   intent dropped, original untouched.
//! - crash between 2 and 3 → Pending with the copy in quarantine and the
//!   original gone: flipped to Committed (fully undoable).
//! - crash during 3 → same as above (the flip is idempotent).
//!
//! Files in `quarantine/` with no record are **orphans**: reported by
//! [`list_orphans`], never deleted by any code path.
//!
//! Undo restores bytes first (integrity-checked against the archived
//! size/hash when present), then the security snapshot best-effort; fidelity
//! notes ride along in `record.security_notes` instead of failing the
//! restore — the bytes are authoritative.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::acl::FileSecurity;
use crate::model::PersistenceEntry;

/// Durability state of a quarantine record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordState {
    /// Intent stored, bytes not yet (durably) moved. Safe to drop while the
    /// original is still in place; healed to [`RecordState::Committed`] when
    /// the copy is already in quarantine and the original is gone.
    Pending,
    /// File is in quarantine; undoable.
    Committed,
}

fn default_committed() -> RecordState {
    RecordState::Committed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub id: String,
    pub name: String,
    pub source: String,
    pub original_path: PathBuf,
    pub quarantine_path: PathBuf,
    pub archived_at: DateTime<Utc>,
    /// Crash-safety protocol state. Absent in pre-P0-4 records → Committed
    /// (those records were only ever written after a successful move).
    #[serde(default = "default_committed")]
    pub state: RecordState,
    /// Byte length at quarantine time; integrity-checked on undo.
    #[serde(default)]
    pub file_size: Option<u64>,
    /// SHA-256 hex at quarantine time; integrity-checked on undo.
    #[serde(default)]
    pub sha256_hex: Option<String>,
    /// Security snapshot (SDDL/attributes/timestamps); `None` for pre-P0-4
    /// records, which restore bytes only (noted in `security_notes`).
    #[serde(default)]
    pub security: Option<FileSecurity>,
    /// Non-fatal fidelity notes from the last undo (ACL/timestamp restores).
    /// Informational only — the restored bytes are authoritative.
    #[serde(default)]
    pub security_notes: Vec<String>,
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
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Records::new()),
        Err(err) => Err(err),
    }
}

/// Atomic save: temp file in the same directory + flush + fsync + replacing
/// rename. A crash can leave a stale `records.json.tmp` (skipped by the
/// orphan scan) but never a torn `records.json`.
fn save_records(data_dir: &Path, records: &Records) -> io::Result<()> {
    fs::create_dir_all(quarantine_dir(data_dir))?;
    let json = serde_json::to_string_pretty(records)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let path = records_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.flush()?;
        f.sync_all()?;
    }
    replace_file(&tmp, &path)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let from_w = wide(from);
    let to_w = wide(to);
    // SAFETY: both NUL-terminated buffers outlive the call; no state held.
    unsafe {
        MoveFileExW(
            PCWSTR(from_w.as_ptr()),
            PCWSTR(to_w.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::from)
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    // Same-directory rename is atomic on POSIX/NTFS alike.
    fs::rename(from, to)
}

pub fn is_quarantined(data_dir: &Path, id: &str) -> bool {
    load_records(data_dir)
        .map(|records| {
            records
                .get(id)
                .is_some_and(|r| matches!(r.state, RecordState::Committed))
        })
        .unwrap_or(false)
}

pub fn list_records(data_dir: &Path) -> Vec<QuarantineRecord> {
    let mut records: Vec<QuarantineRecord> = load_records(data_dir)
        .map(|r| r.into_values().collect())
        .unwrap_or_default();
    records.sort_by_key(|a| a.archived_at);
    records
}

/// Outcome of [`reconcile`].
#[derive(Debug, Default)]
pub struct ReconcileReport {
    /// Pending intents voided (original still in place, or both sides gone).
    pub dropped_pending: usize,
    /// Pending intents healed to Committed (copy present, original gone).
    pub healed_committed: usize,
    /// Quarantine-dir files with no record (never deleted, only reported).
    pub orphans: Vec<PathBuf>,
}

/// Heal interrupted quarantines. Runs at the start of every
/// [`quarantine_entry`]/[`undo_scoped`] call ("on startup" of any mutating
/// flow). Best-effort: callers ignore the error and let the subsequent
/// operation surface the underlying problem.
pub fn reconcile(data_dir: &Path) -> io::Result<ReconcileReport> {
    let mut report = ReconcileReport::default();
    let mut records = load_records(data_dir)?;
    let mut changed = false;
    for record in records.values_mut() {
        if !matches!(record.state, RecordState::Pending) {
            continue;
        }
        let original_present = record.original_path.exists();
        let copy_present = record.quarantine_path.is_file();
        if !original_present && copy_present {
            // Crash landed between move and commit: the quarantine is real
            // and fully described — flip to Committed (idempotent).
            record.state = RecordState::Committed;
            report.healed_committed += 1;
            changed = true;
        }
        // Otherwise the intent is void (move never ran, or the operator
        // cleaned up both sides) and is dropped below. A leftover partial
        // copy, if any, surfaces as an orphan — never auto-deleted.
    }
    let stale: Vec<String> = records
        .iter()
        .filter(|(_, r)| matches!(r.state, RecordState::Pending))
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale {
        records.remove(&id);
        report.dropped_pending += 1;
        changed = true;
    }
    if changed {
        save_records(data_dir, &records)?;
    }
    report.orphans = list_orphans_with(&records, data_dir);
    Ok(report)
}

fn normalize_orphan_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn list_orphans_with(records: &Records, data_dir: &Path) -> Vec<PathBuf> {
    let known: HashSet<String> = records
        .values()
        .map(|r| normalize_orphan_key(&r.quarantine_path))
        .collect();
    let dir = quarantine_dir(data_dir);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut orphans: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name != "records.json" && !name.ends_with(".tmp")
        })
        .filter(|p| !known.contains(&normalize_orphan_key(p)))
        .collect();
    orphans.sort();
    orphans
}

/// Files in `quarantine/` with no record. Reported, never deleted.
pub fn list_orphans(data_dir: &Path) -> Vec<PathBuf> {
    load_records(data_dir)
        .map(|records| list_orphans_with(&records, data_dir))
        .unwrap_or_default()
}

fn move_file(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        // Single call, cross-volume capable, write-through. No
        // REPLACE_EXISTING: an occupied destination must fail (the caller
        // guards collisions explicitly).
        if move_file_ex(source, destination).is_ok() {
            return Ok(());
        }
    }
    #[cfg(not(windows))]
    {
        if fs::rename(source, destination).is_ok() {
            return Ok(());
        }
    }
    // Copy fallback (locked renames, exotic filesystems): copy, verify
    // length, then delete the source. A length mismatch cleans the partial
    // copy and fails — the source is never deleted on doubt.
    fs::copy(source, destination)?;
    let copied_len = fs::metadata(destination)?.len();
    let original_len = fs::metadata(source)?.len();
    if copied_len != original_len {
        let _ = fs::remove_file(destination);
        return Err(io::Error::other("copy verification failed during move"));
    }
    fs::remove_file(source)
}

#[cfg(windows)]
fn move_file_ex(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_COPY_ALLOWED, MOVEFILE_WRITE_THROUGH,
    };
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let source_w = wide(source);
    let dest_w = wide(destination);
    // SAFETY: both NUL-terminated buffers outlive the call; no state held.
    unsafe {
        MoveFileExW(
            PCWSTR(source_w.as_ptr()),
            PCWSTR(dest_w.as_ptr()),
            MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::from)
}

pub fn quarantine_entry(data_dir: &Path, entry: &PersistenceEntry) -> io::Result<QuarantineRecord> {
    // Heal interrupted past first so stale intents never block this run.
    let _ = reconcile(data_dir);

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
    if destination.exists() {
        // Never silently overwrite a quarantine slot: a leftover here means
        // an interrupted earlier run (see reconcile/list_orphans).
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "quarantine slot occupied: {} (orphaned file from an interrupted run?)",
                destination.display()
            ),
        ));
    }

    let security = crate::acl::capture(&original);
    let file_size = fs::metadata(&original).ok().map(|m| m.len());
    let sha256_hex = crate::hash_intel::sha256_file_hex(&original);

    let record = QuarantineRecord {
        id: entry.id.clone(),
        name: entry.name.clone(),
        source: entry.source.tag().to_string(),
        original_path: original.clone(),
        quarantine_path: destination.clone(),
        archived_at: Utc::now(),
        state: RecordState::Pending,
        file_size,
        sha256_hex,
        security: Some(security),
        security_notes: Vec::new(),
    };

    // Step 1 — durable intent BEFORE touching the file. If this save fails,
    // nothing moved and nothing is recorded: return as-is.
    let mut records = load_records(data_dir)?;
    records.insert(record.id.clone(), record.clone());
    save_records(data_dir, &records)?;

    // Step 2 — move the bytes. On failure roll the intent back; if the
    // rollback save itself fails, the leftover Pending is healed by a later
    // reconcile (the original is still in place, so it is dropped).
    if let Err(err) = move_file(&original, &destination) {
        let mut records = load_records(data_dir).unwrap_or_default();
        records.remove(&record.id);
        let _ = save_records(data_dir, &records);
        return Err(err);
    }

    // Step 3 — flip to Committed. If this save fails the bytes ARE
    // quarantined; reconcile heals the Pending (original gone + copy
    // present). Report the failure honestly instead of claiming success.
    let mut committed = record.clone();
    committed.state = RecordState::Committed;
    let mut records = load_records(data_dir).unwrap_or_default();
    records.insert(committed.id.clone(), committed.clone());
    save_records(data_dir, &records)?;
    Ok(committed)
}

pub fn quarantine_file(data_dir: &Path, entry: &PersistenceEntry) -> io::Result<QuarantineRecord> {
    quarantine_entry(data_dir, entry)
}

pub fn undo(data_dir: &Path, id: &str) -> io::Result<QuarantineRecord> {
    undo_scoped(data_dir, id, None)
}

/// Restore a quarantined file, optionally constraining WHERE it may go.
///
/// `allowed_roots` (when `Some`) must contain the record's original path
/// (prefix match after normalization), otherwise restoration is refused.
/// Rationale: `records.json` lives next to the quarantine on portable
/// media; a planted records file pointing `original_path` at a system
/// location must not turn an elevated `undo` into an arbitrary file plant.
/// CLI/GUI pass their scan roots (startup/tasks); bare `undo` keeps the
/// historical behavior for tests and power users.
pub fn undo_scoped(
    data_dir: &Path,
    id: &str,
    allowed_roots: Option<&[PathBuf]>,
) -> io::Result<QuarantineRecord> {
    let _ = reconcile(data_dir);

    let mut records = load_records(data_dir)?;
    let Some(mut record) = records.remove(id) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no quarantine record for id {id}"),
        ));
    };
    // A Pending record is never restored: either its move never ran (nothing
    // to restore) or it should just have been healed to Committed above.
    if !matches!(record.state, RecordState::Committed) {
        records.insert(record.id.clone(), record);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("quarantine record for id {id} is not committed; nothing to restore"),
        ));
    }
    if let Some(roots) = allowed_roots {
        if !under_any_root(&record.original_path, roots) {
            records.insert(record.id.clone(), record);
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to restore outside the scanned startup/task roots \
                 (re-run with matching --startup-root/--tasks-root if the roots moved)",
            ));
        }
    }
    match restore(&record) {
        Ok(notes) => {
            record.security_notes = notes;
            // The restore already happened; a save failure here leaves a
            // stale Committed record whose file is gone (a later undo
            // reports NotFound). Surface the error honestly, as before.
            save_records(data_dir, &records)?;
            Ok(record)
        }
        Err(err) => {
            records.insert(record.id.clone(), record);
            Err(err)
        }
    }
}

fn normalize_rooted(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn under_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    let target = normalize_rooted(path);
    roots.iter().any(|root| {
        let base = normalize_rooted(root).trim_end_matches('/').to_string() + "/";
        target.starts_with(&base)
    })
}

fn restore(record: &QuarantineRecord) -> io::Result<Vec<String>> {
    if !record.quarantine_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "quarantined file is gone: {}",
                record.quarantine_path.display()
            ),
        ));
    }
    if record.original_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "original path is occupied: {}",
                record.original_path.display()
            ),
        ));
    }
    // Integrity: the quarantined bytes must be what we archived. A mismatch
    // means the store was tampered with (or bit-rotted) — planting altered
    // bytes back onto the system is exactly what quarantine must not do.
    if let Some(size) = record.file_size {
        let actual = fs::metadata(&record.quarantine_path)?.len();
        if actual != size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "quarantined file changed since archival (size {actual} != {size}); refusing restore"
                ),
            ));
        }
    }
    if let Some(expected) = record.sha256_hex.as_deref() {
        let actual = crate::hash_intel::sha256_file_hex(&record.quarantine_path);
        if actual.as_deref() != Some(expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "quarantined file changed since archival (hash mismatch); refusing restore",
            ));
        }
    }
    if let Some(parent) = record.original_path.parent() {
        fs::create_dir_all(parent)?;
    }
    move_file(&record.quarantine_path, &record.original_path)?;
    // Security snapshot, best-effort: bytes are back regardless; fidelity
    // gaps ride along as notes instead of failing the restore.
    let mut notes = Vec::new();
    match &record.security {
        Some(snapshot) => notes.extend(crate::acl::restore(&record.original_path, snapshot)),
        None => notes.push(
            "no security snapshot (archived before ACL preservation); timestamps/ACLs not restored"
                .to_string(),
        ),
    }
    Ok(notes)
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
            PersistenceEntry::new(
                PersistenceSource::StartupFolder,
                name,
                &path_text,
                &path_text,
            ),
            file,
        )
    }

    /// Hand-craft a Pending record exactly as a crash between protocol steps
    /// 1 and 2 (or 2 and 3) would leave it.
    fn plant_pending(
        data: &Path,
        entry: &PersistenceEntry,
        original_present: bool,
        copy_present: bool,
    ) -> QuarantineRecord {
        let dir = quarantine_dir(data);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(format!(
            "{}_{}",
            entry.id,
            Path::new(&entry.location)
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
        if copy_present {
            fs::write(&dest, payload()).unwrap();
        }
        if !original_present {
            let _ = fs::remove_file(&entry.location);
        }
        let record = QuarantineRecord {
            id: entry.id.clone(),
            name: entry.name.clone(),
            source: entry.source.tag().to_string(),
            original_path: PathBuf::from(&entry.location),
            quarantine_path: dest,
            archived_at: Utc::now(),
            state: RecordState::Pending,
            file_size: Some(payload().len() as u64),
            sha256_hex: Some(crate::hash_intel::sha256_hex(&payload())),
            security: None,
            security_notes: Vec::new(),
        };
        let mut records = load_records(data).unwrap();
        records.insert(record.id.clone(), record.clone());
        save_records(data, &records).unwrap();
        record
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
        assert!(matches!(record.state, RecordState::Committed));
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

    #[test]
    fn scoped_undo_allows_restore_under_roots() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "ok.bin");

        quarantine_entry(data.path(), &entry).unwrap();
        let restored = undo_scoped(
            data.path(),
            &entry.id,
            Some(&[user_land.path().to_path_buf()]),
        )
        .unwrap();

        assert_eq!(restored.id, entry.id);
        assert!(original.is_file());
        assert!(!is_quarantined(data.path(), &entry.id));
    }

    #[test]
    fn scoped_undo_refuses_planted_destination_outside_roots() {
        // Threat model: a records.json planted on portable media pointing
        // original_path at a system location must not turn an elevated undo
        // into an arbitrary file plant.
        let user_land = tempdir().unwrap();
        let other_root = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, _original) = setup_entry(user_land.path(), "evil.bin");

        let record = quarantine_entry(data.path(), &entry).unwrap();
        assert!(record.original_path.starts_with(user_land.path()));

        let err = undo_scoped(
            data.path(),
            &entry.id,
            Some(&[other_root.path().to_path_buf()]),
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        // Record preserved, file stays quarantined — nothing moved.
        assert!(is_quarantined(data.path(), &entry.id));
        assert!(record.quarantine_path.is_file());
    }

    #[test]
    fn scoped_undo_rejects_sibling_prefix_trick() {
        // C:\foo must not authorize C:\foobar\… (prefix + separator check).
        let roots = [PathBuf::from(r"C:\foo")];
        assert!(under_any_root(Path::new(r"C:\foo\bar.exe"), &roots));
        assert!(under_any_root(Path::new(r"C:\FOO\bar.exe"), &roots));
        assert!(!under_any_root(Path::new(r"C:\foobar\bar.exe"), &roots));
        assert!(!under_any_root(Path::new(r"D:\foo\bar.exe"), &roots));
    }

    #[test]
    fn unscoped_undo_keeps_historical_behavior() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "legacy.bin");

        quarantine_entry(data.path(), &entry).unwrap();
        undo(data.path(), &entry.id).unwrap();

        assert!(original.is_file());
    }

    #[test]
    fn save_failure_leaves_original_untouched() {
        // Natural failure injection: a directory where records.json must go
        // makes the step-1 save fail — before anything moves.
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "precious.dll");

        fs::create_dir_all(quarantine_dir(data.path())).unwrap();
        fs::create_dir(records_path(data.path())).unwrap();

        let err = quarantine_entry(data.path(), &entry).unwrap_err();
        assert!(
            err.kind() == io::ErrorKind::PermissionDenied
                || err.kind() == io::ErrorKind::IsADirectory
                || err.kind() == io::ErrorKind::Other,
            "unexpected kind: {:?}",
            err.kind()
        );
        // Nothing moved, nothing half-recorded.
        assert_eq!(fs::read(&original).unwrap(), payload());
        assert!(list_records(data.path()).is_empty());
        assert!(!is_quarantined(data.path(), &entry.id));
    }

    #[test]
    fn crash_pending_without_move_is_dropped() {
        // Crash between protocol steps 1 and 2: intent stored, bytes never
        // moved. Reconcile must void the intent and touch nothing.
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "crash1.exe");

        plant_pending(data.path(), &entry, true, false);

        let report = reconcile(data.path()).unwrap();
        assert_eq!(report.dropped_pending, 1);
        assert_eq!(report.healed_committed, 0);
        assert_eq!(fs::read(&original).unwrap(), payload());
        assert!(list_records(data.path()).is_empty());
        // A later undo finds no record (NotFound), never a half-move.
        assert_eq!(
            undo(data.path(), &entry.id).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn crash_after_move_heals_to_committed_and_undoes() {
        // Crash between steps 2 and 3: bytes in quarantine, flip missing.
        // Reconcile heals to Committed; the file stays fully undoable.
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "crash2.exe");

        let planted = plant_pending(data.path(), &entry, false, true);

        let report = reconcile(data.path()).unwrap();
        assert_eq!(report.healed_committed, 1);
        assert!(is_quarantined(data.path(), &entry.id));

        let restored = undo(data.path(), &entry.id).unwrap();
        assert_eq!(restored.id, planted.id);
        assert_eq!(fs::read(&original).unwrap(), payload());
        assert!(list_records(data.path()).is_empty());
    }

    #[test]
    fn orphans_are_surfaced_and_never_deleted() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, _original) = setup_entry(user_land.path(), "host.exe");
        quarantine_entry(data.path(), &entry).unwrap();

        // A stray file with no record (interrupted run, operator copy, …).
        let stray = quarantine_dir(data.path()).join("stray-from-nowhere.bin");
        fs::write(&stray, b"mystery").unwrap();

        let report = reconcile(data.path()).unwrap();
        assert_eq!(report.orphans, vec![stray.clone()]);
        assert_eq!(list_orphans(data.path()), vec![stray.clone()]);
        // Surfaced — and still there after every operation.
        quarantine_entry(data.path(), &entry).unwrap_err(); // original gone
        assert!(stray.is_file());
        assert_eq!(fs::read(&stray).unwrap(), b"mystery");
    }

    #[test]
    fn destination_collision_is_refused() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "collide.exe");

        // Squat the deterministic quarantine slot first.
        fs::create_dir_all(quarantine_dir(data.path())).unwrap();
        let slot = quarantine_dir(data.path()).join(format!(
            "{}_{}",
            entry.id,
            Path::new(&entry.location)
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
        fs::write(&slot, b"squatter").unwrap();

        let err = quarantine_entry(data.path(), &entry).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&original).unwrap(), payload());
        assert_eq!(fs::read(&slot).unwrap(), b"squatter");
    }

    #[test]
    fn tampered_quarantine_copy_refuses_undo() {
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "tamper.exe");

        let record = quarantine_entry(data.path(), &entry).unwrap();
        fs::write(&record.quarantine_path, b"altered-by-attacker").unwrap();

        let err = undo(data.path(), &entry.id).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        // Record preserved, hostile bytes NOT planted back, original absent.
        assert!(is_quarantined(data.path(), &entry.id));
        assert!(!original.exists());
    }

    #[test]
    fn legacy_records_load_as_committed_and_undo() {
        // Pre-P0-4 records.json has none of the new fields; it must still
        // load (as Committed) and undo bytes-only with a fidelity note.
        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "legacy2.exe");

        let record = quarantine_entry(data.path(), &entry).unwrap();
        let legacy_json = format!(
            "{{\"{id}\":{{\"id\":\"{id}\",\"name\":\"{name}\",\"source\":\"startup-folder\",\
\"original_path\":{op:?},\"quarantine_path\":{qp:?},\
\"archived_at\":\"2026-01-01T00:00:00Z\"}}}}",
            id = entry.id,
            name = entry.name,
            op = record.original_path,
            qp = record.quarantine_path,
        );
        fs::write(records_path(data.path()), legacy_json).unwrap();

        let records = list_records(data.path());
        assert_eq!(records.len(), 1);
        assert!(matches!(records[0].state, RecordState::Committed));
        assert!(is_quarantined(data.path(), &entry.id));

        let restored = undo(data.path(), &entry.id).unwrap();
        assert_eq!(fs::read(&original).unwrap(), payload());
        assert!(
            restored
                .security_notes
                .iter()
                .any(|n| n.contains("no security snapshot")),
            "notes: {:?}",
            restored.security_notes
        );
    }

    /// A locked source fails the move; the Pending intent must be rolled
    /// back so no record survives and the original is untouched.
    #[cfg(windows)]
    #[test]
    fn move_failure_rolls_back_pending() {
        use std::os::windows::fs::OpenOptionsExt;

        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (entry, original) = setup_entry(user_land.path(), "locked.exe");

        // Exclusive lock: rename/copy of the source cannot proceed while
        // held (mimics an in-use binary).
        {
            let _lock = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&original)
                .expect("exclusive lock");
            quarantine_entry(data.path(), &entry).unwrap_err();
        }

        assert_eq!(fs::read(&original).unwrap(), payload());
        assert!(!is_quarantined(data.path(), &entry.id));
        assert!(list_records(data.path()).is_empty());
    }

    #[test]
    fn old_lnk_record_with_stale_command_id_still_undoes() {
        // ID compatibility: before F-LNK-1, .lnk entries had `command == location == lnk path`.
        // After, `command == resolved target + args` while `location` stays the lnk file.
        // `make_id` hashes `source.tag + name + command`, so ids change. Quarantine
        // records store the *old* id and `original_path` (= lnk file). Undo must
        // still work by old id even though a fresh scan now yields a different id.
        use crate::model::{make_id, PersistenceSource};

        let user_land = tempdir().unwrap();
        let data = tempdir().unwrap();
        let (old_entry, original) = setup_entry(user_land.path(), "oldlnk.lnk");
        // New-style command: resolved target (different string) => different id
        let new_command = r"C:\Windows\System32\notepad.exe --flag";
        let new_id = make_id(&PersistenceSource::StartupFolder, "oldlnk.lnk", new_command);
        let old_id = old_entry.id.clone();
        assert_ne!(
            new_id, old_id,
            "new target-based command must change id vs old lnk-path command"
        );

        // Quarantine the old-style entry (as if it were created before the fix)
        let record = quarantine_entry(data.path(), &old_entry).unwrap();
        assert_eq!(record.id, old_id);
        assert!(is_quarantined(data.path(), &old_id));
        assert!(!is_quarantined(data.path(), &new_id));

        // Undo by old id must still restore the .lnk file (original_path is the lnk file)
        let restored = undo(data.path(), &old_id).unwrap();
        assert_eq!(restored.id, old_id);
        assert!(original.is_file());
        assert!(!is_quarantined(data.path(), &old_id));
        assert!(!is_quarantined(data.path(), &new_id));
    }
}
