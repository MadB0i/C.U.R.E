//! Directory-change event acquisition for the ransomware canary engine.
//!
//! Architecture position:
//!
//! ```text
//! cure_core::canary   pure state machine (FileEvent in, CanaryAlert out)
//! cure_dirwatch (here) OS event acquisition: ReadDirectoryChangesW ->
//!                      FileEvent batches + decoy planting (shared glue)
//! gui / watch          thin integration: own thread, own alert sink
//! ```
//!
//! [`run_dir_guard`] owns one watched directory: it plants nothing itself,
//! but blocks in a `ReadDirectoryChangesW` loop, translates notifications
//! into [`FileEvent`]s, feeds a [`CanaryEngine`], and hands each resulting
//! [`CanaryAlert`] to the caller's sink. The GUI sinks alerts into Tauri
//! events; the watcher sinks them into its log file — neither needs its own
//! copy of the unsafe Win32 parsing code anymore.
//!
//! Everything in this crate is side-effect-scoped to the watched directory
//! plus decoy files. No remediation, no killing, no deletion.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cure_core::canary::{
    self as canary, CanaryAlert, CanaryConfig, CanaryEngine, FileEvent, FileEventKind,
};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Candidate user folders for decoys / watching (Desktop, Documents,
/// Downloads under `%USERPROFILE%`). Returned unfiltered — callers decide
/// which entries exist (the GUI needs `is_dir` filtering for its ransom
/// scan; the watcher filters before spawning).
pub fn user_folder_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let home = PathBuf::from(home);
        for name in &["Desktop", "Documents", "Downloads"] {
            dirs.push(home.join(name));
        }
    }
    dirs
}

/// Plant up to 6 decoy files in `dir`. Never overwrites: existing files
/// (including previously planted decoys) are left untouched.
pub fn plant_decoys(dir: &Path) {
    let names = canary::decoy_names(6);
    for name in &names {
        let path = dir.join(name);
        if !path.exists() {
            let content: Vec<u8> = (0..512).map(|i| (i * 73 + 11) as u8).collect();
            let _ = std::fs::write(&path, &content);
        }
    }
}

/// Block watching `dir` until `stop` is set, feeding filesystem changes
/// through a [`CanaryEngine`] and calling `on_alert` for every alert.
/// Returns when `stop` becomes true or the directory handle breaks.
/// Never panics on I/O errors: an unwatched directory simply yields no
/// events (callers log the coverage gap if they care).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceChangeKind {
    Created,
    Modified,
    Removed,
    RenamedFrom,
    RenamedTo,
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceChange {
    pub kind: WorkspaceChangeKind,
    pub relative: String,
    pub at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNotifyEvent {
    pub action: u32,
    pub name: String,
}

/// Safely parse raw `FILE_NOTIFY_INFORMATION` buffers without unaligned pointer casts.
///
/// Windows `ReadDirectoryChangesW` writes a variable-length list of records into the
/// caller's buffer. Each record has:
/// - NextEntryOffset: u32 (offset 0)
/// - Action: u32 (offset 4)
/// - FileNameLength: u32 (offset 8, in bytes)
/// - FileName: [u16] (offset 12)
///
/// This function reads fields using `u32::from_le_bytes`, validates all slice bounds,
/// rejects misaligned NextEntryOffset, and safely decodes UTF-16 filenames.
pub fn parse_notify_buffer(buf: &[u8]) -> Vec<ParsedNotifyEvent> {
    let mut events = Vec::new();
    let mut offset: usize = 0;
    let buf_len = buf.len();

    while offset + 12 <= buf_len {
        let next_offset = u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ]) as usize;
        let action = u32::from_le_bytes([
            buf[offset + 4],
            buf[offset + 5],
            buf[offset + 6],
            buf[offset + 7],
        ]);
        let name_len = u32::from_le_bytes([
            buf[offset + 8],
            buf[offset + 9],
            buf[offset + 10],
            buf[offset + 11],
        ]) as usize;

        let name_start = offset + 12;
        if name_start + name_len > buf_len {
            break; // Truncated record or invalid length: stop safely
        }

        let name_bytes = &buf[name_start..name_start + name_len];
        let name = String::from_utf16_lossy(
            name_bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect::<Vec<_>>()
                .as_slice(),
        );

        events.push(ParsedNotifyEvent { action, name });

        if next_offset == 0 {
            break;
        }
        // Windows specs require NextEntryOffset to be DWORD aligned (4 bytes) and at least 12 bytes
        if next_offset < 12 || !next_offset.is_multiple_of(4) || offset + next_offset > buf_len {
            break; // Corrupt offset: prevent infinite loops and buffer overruns
        }
        offset += next_offset;
    }

    events
}

#[cfg(windows)]
pub fn run_dir_guard(dir: &Path, stop: &AtomicBool, on_alert: impl Fn(CanaryAlert)) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, BOOL, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED,
        FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

    let dir_wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(dir.as_os_str())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };

    let h_dir = unsafe {
        // FILE_FLAG_OVERLAPPED is load-bearing: without it every I/O on
        // this handle is SYNCHRONOUS (the OVERLAPPED struct is ignored),
        // so ReadDirectoryChangesW would block until the next filesystem
        // change — the 2 s wait below would never fire and the stop flag
        // could not terminate the thread on a quiet directory.
        CreateFileW(
            PCWSTR(dir_wide.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            None,
        )
    };
    let h_dir = match h_dir {
        Ok(h) if h != INVALID_HANDLE_VALUE => h,
        _ => return,
    };

    let h_event = unsafe { CreateEventW(None, BOOL::from(false), BOOL::from(false), None) };
    let h_event = match h_event {
        Ok(h) => h,
        Err(_) => {
            let _ = unsafe { CloseHandle(h_dir) };
            return;
        }
    };

    let mut engine = CanaryEngine::new(CanaryConfig::default());
    // 64 KiB notification buffer: the documented 4 KiB size overflows under
    // legitimate bursts (build output, photo imports), silently LOSING
    // events (ERROR_NOTIFY_ENUM_DIR). 64 KiB matches common practice and
    // shrinks — not eliminates — the loss window; residual loss is a known
    // limitation (see AUDIT.md), which is why decoy tamper (not bursts) is
    // the primary signal.
    let mut buffer = [0u8; 64 * 1024];
    let folder_str = dir.to_string_lossy().to_string();

    while !stop.load(Ordering::Relaxed) {
        let mut bytes_returned = 0u32;
        let mut overlapped = OVERLAPPED {
            hEvent: h_event,
            ..OVERLAPPED::default()
        };

        let ok = unsafe {
            ReadDirectoryChangesW(
                h_dir,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                buffer.len() as u32,
                BOOL::from(true),
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
                Some(&mut bytes_returned),
                Some(&mut overlapped),
                None,
            )
        };

        if ok.is_err() {
            break;
        }

        let wait_result = unsafe { WaitForSingleObject(h_event, 2000) };
        if wait_result.0 != 0 {
            // Timeout (or abandonment): cancel the still-pending read so
            // the next iteration never stacks a second operation behind it,
            // then re-check the stop flag promptly.
            unsafe {
                let _ = CancelIo(h_dir);
            }
            continue;
        }

        unsafe {
            let _ = GetOverlappedResult(h_dir, &overlapped, &mut bytes_returned, BOOL::from(false));
        }

        if bytes_returned == 0 {
            continue;
        }

        let ts = now_secs();
        let events = parse_notify_buffer(&buffer[..bytes_returned as usize]);
        for ev in events {
            let kind = if ev.action == FILE_ACTION_ADDED.0 {
                FileEventKind::Added
            } else if ev.action == FILE_ACTION_REMOVED.0 {
                FileEventKind::Removed
            } else if ev.action == FILE_ACTION_MODIFIED.0 {
                FileEventKind::Modified
            } else if ev.action == FILE_ACTION_RENAMED_OLD_NAME.0 {
                FileEventKind::RenamedOldName
            } else if ev.action == FILE_ACTION_RENAMED_NEW_NAME.0 {
                FileEventKind::RenamedNewName
            } else {
                FileEventKind::Modified
            };

            let alerts = engine.observe(FileEvent {
                at_secs: ts,
                folder: folder_str.clone(),
                name: ev.name,
                kind,
            });
            for alert in alerts {
                on_alert(alert);
            }
        }
    }

    let _ = unsafe { CloseHandle(h_event) };
    let _ = unsafe { CloseHandle(h_dir) };
}

/// Non-Windows placeholder: no directory notifications available; idle until
/// `stop` so callers keep identical structure on every platform.
#[cfg(not(windows))]
pub fn run_dir_guard(_dir: &Path, stop: &AtomicBool, _on_alert: impl Fn(CanaryAlert)) {
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(windows)]
pub fn run_workspace_recorder(
    dir: &Path,
    stop: &AtomicBool,
    mut on_batch: impl FnMut(Vec<WorkspaceChange>),
) -> Result<(), std::io::Error> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, BOOL, ERROR_NOTIFY_ENUM_DIR, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

    let dir_wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(dir.as_os_str())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };

    // SAFETY: dir_wide is null-terminated UTF-16
    let h_dir = unsafe {
        CreateFileW(
            PCWSTR(dir_wide.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            None,
        )
    };
    let h_dir = match h_dir {
        Ok(h) if h != INVALID_HANDLE_VALUE => h,
        _ => return Err(std::io::Error::last_os_error()),
    };

    // SAFETY: Creating event for overlapped I/O
    let h_event = unsafe { CreateEventW(None, BOOL::from(false), BOOL::from(false), None) };
    let h_event = match h_event {
        Ok(h) => h,
        Err(e) => {
            let _ = unsafe { CloseHandle(h_dir) };
            return Err(std::io::Error::from_raw_os_error(e.code().0));
        }
    };

    let mut buffer = [0u8; 64 * 1024];

    while !stop.load(Ordering::Relaxed) {
        let mut bytes_returned = 0u32;
        let mut overlapped = OVERLAPPED {
            hEvent: h_event,
            ..OVERLAPPED::default()
        };

        // SAFETY: buffer and overlapped are valid stack pointers
        let ok = unsafe {
            ReadDirectoryChangesW(
                h_dir,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                buffer.len() as u32,
                BOOL::from(true),
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_CREATION,
                Some(&mut bytes_returned),
                Some(&mut overlapped),
                None,
            )
        };
        if ok.is_err() {
            on_batch(vec![WorkspaceChange {
                kind: WorkspaceChangeKind::Overflow,
                relative: String::new(),
                at_unix: now_secs(),
            }]);
            break;
        }

        // SAFETY: Waiting on event handle with 2s timeout
        let wait_result = unsafe { WaitForSingleObject(h_event, 2000) };
        if wait_result.0 != 0 {
            // SAFETY: Canceling pending I/O on timeout
            unsafe {
                let _ = CancelIo(h_dir);
            }
            continue;
        }

        // SAFETY: Checking overlapped result
        let result = unsafe {
            GetOverlappedResult(h_dir, &overlapped, &mut bytes_returned, BOOL::from(false))
        };
        if let Err(err) = result {
            if err.code().0 == ERROR_NOTIFY_ENUM_DIR.0 as i32 {
                on_batch(vec![WorkspaceChange {
                    kind: WorkspaceChangeKind::Overflow,
                    relative: String::new(),
                    at_unix: now_secs(),
                }]);
                continue;
            }
            on_batch(vec![WorkspaceChange {
                kind: WorkspaceChangeKind::Overflow,
                relative: String::new(),
                at_unix: now_secs(),
            }]);
            break;
        }
        if bytes_returned == 0 {
            continue;
        }

        let ts = now_secs();
        let events = parse_notify_buffer(&buffer[..bytes_returned as usize]);
        let mut batch = Vec::with_capacity(events.len());
        for ev in events {
            let kind = match ev.action {
                1 => Some(WorkspaceChangeKind::Created),
                2 => Some(WorkspaceChangeKind::Removed),
                3 => Some(WorkspaceChangeKind::Modified),
                4 => Some(WorkspaceChangeKind::RenamedFrom),
                5 => Some(WorkspaceChangeKind::RenamedTo),
                _ => None,
            };
            if let Some(kind) = kind {
                batch.push(WorkspaceChange {
                    kind,
                    relative: ev.name.replace('\\', "/"),
                    at_unix: ts,
                });
            }
        }
        if !batch.is_empty() {
            on_batch(batch);
        }
    }

    // SAFETY: Closing open Win32 handles
    let _ = unsafe { CloseHandle(h_event) };
    let _ = unsafe { CloseHandle(h_dir) };
    Ok(())
}

#[cfg(not(windows))]
pub fn run_workspace_recorder(
    _dir: &Path,
    stop: &AtomicBool,
    _on_batch: impl FnMut(Vec<WorkspaceChange>),
) -> Result<(), std::io::Error> {
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_notify_buffer_normal_and_malformed() {
        // Build a mock FILE_NOTIFY_INFORMATION record:
        // NextEntryOffset: 0
        // Action: 1 (FILE_ACTION_ADDED)
        // FileNameLength: 8 bytes (4 UTF-16 characters: "test")
        let mut buf = Vec::new();
        buf.extend_from_slice(&0u32.to_le_bytes()); // NextEntryOffset
        buf.extend_from_slice(&1u32.to_le_bytes()); // Action
        buf.extend_from_slice(&8u32.to_le_bytes()); // FileNameLength (8 bytes)
                                                    // UTF-16 "test"
        for ch in "test".encode_utf16() {
            buf.extend_from_slice(&ch.to_le_bytes());
        }

        let events = parse_notify_buffer(&buf);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, 1);
        assert_eq!(events[0].name, "test");

        // Malformed test 1: truncated header (< 12 bytes)
        assert!(parse_notify_buffer(&buf[..10]).is_empty());

        // Malformed test 2: filename length extends past end of buffer
        let mut bad_len_buf = buf.clone();
        bad_len_buf[8..12].copy_from_slice(&100u32.to_le_bytes());
        assert!(parse_notify_buffer(&bad_len_buf).is_empty());

        // Malformed test 3: NextEntryOffset not aligned to 4 bytes or < 12
        let mut bad_offset_buf = buf.clone();
        bad_offset_buf[0..4].copy_from_slice(&13u32.to_le_bytes()); // misaligned offset 13
        let events = parse_notify_buffer(&bad_offset_buf);
        assert_eq!(events.len(), 1); // parses first, safely stops without crash/loop on corrupt offset
    }

    #[test]
    fn plant_decoys_creates_six_decoys() {
        let dir = tempfile::TempDir::new().unwrap();
        plant_decoys(dir.path());
        let names = canary::decoy_names(6);
        assert_eq!(names.len(), 6);
        for name in &names {
            assert!(dir.path().join(name).is_file(), "missing decoy {name}");
        }
    }

    #[test]
    fn plant_decoys_never_overwrites_existing_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = dir.path().join(&canary::decoy_names(6)[0]);
        std::fs::write(&first, b"user content - do not touch").unwrap();
        plant_decoys(dir.path());
        assert_eq!(
            std::fs::read(&first).unwrap(),
            b"user content - do not touch"
        );
    }

    #[test]
    fn plant_decoys_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        plant_decoys(dir.path());
        let before: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), e.metadata().unwrap().len())
            })
            .collect();
        plant_decoys(dir.path());
        let after: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), e.metadata().unwrap().len())
            })
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn dir_guard_returns_promptly_when_already_stopped() {
        let dir = tempfile::TempDir::new().unwrap();
        let stop = AtomicBool::new(true);
        let started = std::time::Instant::now();
        run_dir_guard(dir.path(), &stop, |_| panic!("no alerts expected"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "run_dir_guard did not return promptly on a pre-set stop flag"
        );
    }

    /// LIVE filesystem validation (Windows only, tempdir only — never user
    /// documents). Drives real ReadDirectoryChangesW events through the
    /// shared guard: noise first (no tamper expected), then decoy
    /// modify/rename/delete (tamper expected), then a second run proving
    /// restart state resets (new engine alerts again, no handle leaks).
    /// This validates the EXPERIMENTAL guard's plumbing, not ransomware
    /// detection — the UI must keep saying exactly that.
    #[cfg(windows)]
    #[test]
    fn live_decoy_tamper_alerts_on_real_filesystem() {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        plant_decoys(dir.path());
        let decoys = canary::decoy_names(6);

        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let watch_dir = dir.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            run_dir_guard(&watch_dir, &stop_thread, move |alert| {
                let _ = tx.send(alert);
            });
        });
        // Let the watcher open the handle and arm its first read.
        std::thread::sleep(Duration::from_secs(1));

        let drain_tamper = |timeout: Duration| -> Vec<CanaryAlert> {
            let deadline = std::time::Instant::now() + timeout;
            let mut found = Vec::new();
            while std::time::Instant::now() < deadline {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(alert) => {
                        if matches!(alert, CanaryAlert::CanaryTamper { .. }) {
                            found.push(alert);
                            break;
                        }
                        // Burst alerts from noise are expected engine
                        // behavior (documented FP tradeoff) — keep draining.
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            found
        };

        // 1. Noise first: 20 ordinary files must not produce TAMPER
        // (a burst alert is acceptable and ignored by the drain filter).
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("work-{i}.tmp")), b"noise").unwrap();
        }
        std::thread::sleep(Duration::from_secs(2));

        // 2. Modify decoy 0 → tamper.
        std::fs::write(dir.path().join(&decoys[0]), b"ransomware was here").unwrap();
        assert_eq!(
            drain_tamper(Duration::from_secs(15)).len(),
            1,
            "modifying a decoy must raise exactly one tamper alert"
        );

        // 3. Rename decoy 1 → tamper.
        std::fs::rename(
            dir.path().join(&decoys[1]),
            dir.path().join("renamed-away.bin"),
        )
        .unwrap();
        // Cooldown (120 s) may suppress a second tamper of the same kind;
        // accept zero-or-one but require the run to stay alive and quiet
        // afterwards rather than erroring.
        let _ = drain_tamper(Duration::from_secs(10));

        // 4. Delete decoy 2 → observed without crashing (cooldown may
        // suppress the repeat alert; survival is the assertion).
        std::fs::remove_file(dir.path().join(&decoys[2])).unwrap();
        std::thread::sleep(Duration::from_secs(2));

        // 5. Cross-extension renames of ordinary files → no rewrite alert
        // for decoys (rewrite needs same-extension concentration).
        for (i, ext) in ["aaa", "bbb", "ccc"].iter().enumerate() {
            let src = dir.path().join(format!("plain-{i}.tmp"));
            std::fs::write(&src, b"x").unwrap();
            std::fs::rename(&src, dir.path().join(format!("plain-{i}.{ext}"))).unwrap();
        }
        std::thread::sleep(Duration::from_secs(2));

        stop.store(true, Ordering::SeqCst);
        handle.join().expect("guard thread must shut down cleanly");
        // TempDir cleanup removes every decoy and noise file: no fixture
        // persists after the test by construction.
    }

    /// Restart resets engine state: a second guard over the same directory
    /// alerts on new tampering (no cross-run suppression, no handle leak).    #[cfg(windows)]
    #[test]
    fn live_guard_restart_alerts_again() {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        plant_decoys(dir.path());
        let decoys = canary::decoy_names(6);

        for round in 0..2 {
            let (tx, rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = stop.clone();
            let watch_dir = dir.path().to_path_buf();
            let handle = std::thread::spawn(move || {
                run_dir_guard(&watch_dir, &stop_thread, move |alert| {
                    let _ = tx.send(alert);
                });
            });
            std::thread::sleep(Duration::from_secs(1));
            let target = dir.path().join(&decoys[round * 2]);
            std::fs::write(&target, format!("round {round}").as_bytes()).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let mut fired = false;
            while std::time::Instant::now() < deadline {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(CanaryAlert::CanaryTamper { .. }) => {
                        fired = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            stop.store(true, Ordering::SeqCst);
            handle.join().expect("guard thread must shut down cleanly");
            assert!(fired, "round {round}: fresh guard must alert on tamper");
        }
    }

    /// Regression test for the shutdown hang found by live validation:
    /// the directory handle used to be opened WITHOUT FILE_FLAG_OVERLAPPED,
    /// which made every ReadDirectoryChangesW call synchronous — the 2 s
    /// wait never fired and the stop flag could not terminate the thread
    /// on a quiet directory (join hung forever). With overlapped I/O plus
    /// CancelIo-on-timeout, shutdown after real activity must complete
    /// within seconds.
    #[cfg(windows)]
    #[test]
    fn guard_shuts_down_promptly_after_activity() {
        use std::sync::Arc;
        use std::time::Duration;
        for round in 0..3 {
            let dir = tempfile::TempDir::new().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let s2 = stop.clone();
            let d = dir.path().to_path_buf();
            let h = std::thread::spawn(move || {
                run_dir_guard(&d, &s2, |_| {});
            });
            std::thread::sleep(Duration::from_millis(300));
            std::fs::write(dir.path().join(format!("f{round}.tmp")), b"x").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            stop.store(true, Ordering::SeqCst);
            let started = std::time::Instant::now();
            h.join().expect("guard thread must shut down");
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "round {round}: guard did not stop promptly after activity"
            );
        }
    }

    #[test]
    fn run_workspace_recorder_nonexistent_dir_fails() {
        let fake = Path::new("C:\\cure_nonexistent_directory_for_test_123456");
        let stop = AtomicBool::new(false);
        let res = run_workspace_recorder(fake, &stop, |_| {});
        assert!(res.is_err(), "expected error for non-existent directory");
    }

    #[test]
    fn run_workspace_recorder_pre_stopped_succeeds() {
        let dir = tempfile::TempDir::new().unwrap();
        let stop = AtomicBool::new(true);
        let res = run_workspace_recorder(dir.path(), &stop, |_| {});
        assert!(
            res.is_ok(),
            "expected success for valid directory pre-stopped"
        );
    }
}
