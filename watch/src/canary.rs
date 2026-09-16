//! Canary guard for cure-watch — plants decoys and monitors user folders.
//! Directory watching itself lives in `cure_dirwatch` (shared with the GUI);
//! this module keeps the watcher's alert sink (log file) and its
//! shadow-wipe process tripwire.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cure_core::canary::{self, CanaryAlert};

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
        cure_dirwatch::run_dir_guard(&dir, &stop, |alert| {
            crate::logger::log(
                "canary",
                &format!("[{}] {}", alert_tag(&alert), alert_detail(&alert)),
            );
        });
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
/// start directory watchers and a tripwire poller.
/// Returns a stop handle that will shut everything down when set to true.
pub fn start() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));

    let dirs = cure_dirwatch::user_folder_candidates();
    for dir in &dirs {
        if dir.is_dir() {
            crate::logger::log(
                "canary",
                &format!("planting decoys in {}", dir.display()),
            );
            cure_dirwatch::plant_decoys(dir);
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
