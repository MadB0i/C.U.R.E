//! Canary guard for cure-watch — plants decoys and monitors user folders
//! via ReadDirectoryChangesW + polls shadow-wipe processes. All alerts are
//! written to the watcher log file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cure_core::canary::{
    self, CanaryAlert, CanaryConfig, CanaryEngine, FileEvent, FileEventKind,
};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn user_folder_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let home = PathBuf::from(home);
        for name in &["Desktop", "Documents", "Downloads"] {
            dirs.push(home.join(name));
        }
    }
    dirs
}

fn plant_decoys(dir: &std::path::Path) {
    let names = canary::decoy_names(6);
    for name in &names {
        let path = dir.join(name);
        if !path.exists() {
            let content: Vec<u8> = (0..512).map(|i| (i * 73 + 11) as u8).collect();
            let _ = std::fs::write(&path, &content);
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn alert_tag(alert: &CanaryAlert) -> &'static str {
    match alert {
        CanaryAlert::CanaryTamper { .. } => "TAMPER",
        CanaryAlert::BurstEncryption { .. } => "BURST",
        CanaryAlert::ExtensionRewrite { .. } => "REWRITE",
    }
}

fn alert_detail(alert: &CanaryAlert) -> String {
    match alert {
        CanaryAlert::CanaryTamper { folder, file, action, .. } => {
            format!("{folder}\\{file}: {action}")
        }
        CanaryAlert::BurstEncryption { folder, distinct_files, window_secs, .. } => {
            format!("{folder}: {distinct_files} files in {window_secs}s")
        }
        CanaryAlert::ExtensionRewrite { folder, extension, renamed_count, .. } => {
            format!("{folder}: {renamed_count} files -> .{extension}")
        }
    }
}

#[cfg(windows)]
fn enumerate_process_names() -> Vec<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
        PROCESSENTRY32W,
    };
    use windows::Win32::Foundation::CloseHandle;

    let mut names = Vec::new();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snap = match snap {
        Ok(h) => h,
        Err(_) => return names,
    };
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
        loop {
            let name = String::from_utf16_lossy(
                &entry.szExeFile[..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)],
            );
            names.push(name);
            if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snap) };
    names
}

#[cfg(not(windows))]
fn enumerate_process_names() -> Vec<String> {
    Vec::new()
}

fn spawn_dir_watcher(dir: PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, BOOL};
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, OPEN_EXISTING,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE,
            ReadDirectoryChangesW,
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
            FILE_ACTION_RENAMED_OLD_NAME, FILE_ACTION_RENAMED_NEW_NAME,
            FILE_NOTIFY_INFORMATION,
        };
        use windows::Win32::System::IO::{OVERLAPPED, GetOverlappedResult};
        use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

        let dir_wide = to_wide(&dir.to_string_lossy());

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
        let mut buffer = [0u8; 4096];
        let folder_str = dir.to_string_lossy().to_string();

        while !stop.load(Ordering::Relaxed) {
            let mut bytes_returned = 0u32;
            let mut overlapped = OVERLAPPED { hEvent: h_event, ..OVERLAPPED::default() };

            let ok = unsafe {
                ReadDirectoryChangesW(
                    h_dir,
                    buffer.as_mut_ptr() as *mut core::ffi::c_void,
                    buffer.len() as u32,
                    BOOL::from(true),
                    FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE,
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
                let info = unsafe {
                    &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION)
                };
                let name_len = info.FileNameLength as usize;
                let name_byte_offset = offset + 12;
                if name_byte_offset + name_len > buf_end {
                    break;
                }
                let name_bytes = unsafe {
                    std::slice::from_raw_parts(buffer.as_ptr().add(name_byte_offset), name_len)
                };
                let name = String::from_utf16_lossy(
                    name_bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
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

                let ev = FileEvent {
                    at_secs: ts,
                    folder: folder_str.clone(),
                    name,
                    kind,
                };

                let alerts = engine.observe(ev);
                for alert in &alerts {
                    crate::logger::log(
                        "canary",
                        &format!("[{}] {}", alert_tag(alert), alert_detail(alert)),
                    );
                }

                if info.NextEntryOffset == 0 {
                    break;
                }
                offset += info.NextEntryOffset as usize;
            }
        }

        let _ = unsafe { CloseHandle(h_event) };
        let _ = unsafe { CloseHandle(h_dir) };
    });
}

#[cfg(not(windows))]
fn spawn_dir_watcher(_dir: PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

fn spawn_tripwire_poller(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let names = enumerate_process_names();
            for name in &names {
                if let Some(reason) = canary::shadow_wipe_reason(name, "") {
                    crate::logger::log(
                        "canary",
                        &format!("[SHADOW-WIPE] {name}: {reason}"),
                    );
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

/// Start the canary guard: plant decoys in Desktop/Documents/Downloads,
/// start ReadDirectoryChangesW watchers and a tripwire poller.
/// Returns a stop handle that will shut everything down when set to true.
pub fn start() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));

    let dirs = user_folder_candidates();
    for dir in &dirs {
        if dir.is_dir() {
            crate::logger::log(
                "canary",
                &format!("planting decoys in {}", dir.display()),
            );
            plant_decoys(dir);
        }
    }

    let watch_dirs: Vec<PathBuf> = dirs.into_iter().filter(|d| d.is_dir()).collect();
    crate::logger::log(
        "canary",
        &format!("guard started, watching {} dirs", watch_dirs.len()),
    );

    for dir in &watch_dirs {
        spawn_dir_watcher(dir.clone(), stop.clone());
    }
    spawn_tripwire_poller(stop.clone());

    stop
}
