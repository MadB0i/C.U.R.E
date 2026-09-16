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
    CanaryAlert, CanaryConfig, CanaryEngine, FileEvent, FileEventKind, self as canary,
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
#[cfg(windows)]
pub fn run_dir_guard(dir: &Path, stop: &AtomicBool, on_alert: impl Fn(CanaryAlert)) {
    use windows::Win32::Foundation::{CloseHandle, BOOL, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
        FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
    };
    use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::core::PCWSTR;

    let dir_wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(dir.as_os_str())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };

    let h_dir = unsafe {
        CreateFileW(
            PCWSTR(dir_wide.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
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
            continue;
        }

        unsafe {
            let _ = GetOverlappedResult(h_dir, &overlapped, &mut bytes_returned, BOOL::from(false));
        }

        if bytes_returned == 0 {
            continue;
        }

        let ts = now_secs();
        let mut offset: usize = 0;
        let buf_end = bytes_returned as usize;

        while offset + 12 <= buf_end {
            let info =
                unsafe { &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION) };
            let name_len = info.FileNameLength as usize;
            // FileName starts at byte 12 (after NextEntryOffset + Action + FileNameLength)
            let name_byte_offset = offset + 12;
            if name_byte_offset + name_len > buf_end {
                break;
            }
            let name_bytes = unsafe {
                std::slice::from_raw_parts(buffer.as_ptr().add(name_byte_offset), name_len)
            };
            let name = String::from_utf16_lossy(
                name_bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect::<Vec<_>>()
                    .as_slice(),
            );

            let kind = if info.Action == FILE_ACTION_ADDED {
                FileEventKind::Added
            } else if info.Action == FILE_ACTION_REMOVED {
                FileEventKind::Removed
            } else if info.Action == FILE_ACTION_MODIFIED {
                FileEventKind::Modified
            } else if info.Action == FILE_ACTION_RENAMED_OLD_NAME {
                FileEventKind::RenamedOldName
            } else if info.Action == FILE_ACTION_RENAMED_NEW_NAME {
                FileEventKind::RenamedNewName
            } else {
                FileEventKind::Modified
            };

            let alerts = engine.observe(FileEvent {
                at_secs: ts,
                folder: folder_str.clone(),
                name,
                kind,
            });
            for alert in alerts {
                on_alert(alert);
            }

            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
