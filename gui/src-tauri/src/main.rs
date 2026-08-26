#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager, State};

use cure_core::{baseline, quarantine, risk, scanners};
use cure_core::cleanup as disk_cleanup;
use cure_core::overlay::{self, WindowDesc};
use cure_core::model::{PersistenceEntry, PersistenceSource, RiskLevel, ScoredEntry};
use cure_core::signature::SignatureStatus;
use cure_core::process_scan::{self, ProcessInfo, ProcessScore};
use cure_core::ransom_detect::{self, RansomFinding as RansomFindingCore};
use cure_core::canary::{
    CanaryEngine, CanaryConfig, CanaryAlert, FileEvent, FileEventKind,
    decoy_names, shadow_wipe_reason,
};

#[derive(Debug, Clone, Serialize)]
struct ProgressEvent {
    stage: String,
    message: String,
}

#[derive(Serialize)]
struct ScanSummary {
    total: usize,
    high_risk_cleaned: Vec<ScoredEntry>,
    suspicious_for_review: Vec<ScoredEntry>,
    safe: usize,
    process_findings: Vec<ProcessFinding>,
    ransom_findings: Vec<RansomFinding>,
}

#[derive(Serialize)]
struct ProcessFinding {
    name: String,
    pid: u32,
    exe_path: String,
    score: i32,
    risk: String,
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct RansomFinding {
    finding_type: String,
    path: String,
    detail: String,
    suspected_family: Option<String>,
    nomoreransom_url: Option<String>,
}

#[derive(Serialize)]
struct KillReport {
    killed: Vec<ProcessFinding>,
    failed: Vec<String>,
}

fn emit_stage(app: &AppHandle, stage: &str, message: impl Into<String>) {
    let _ = app.emit(
        "scan-progress",
        ProgressEvent {
            stage: stage.to_string(),
            message: message.into(),
        },
    );
}

fn arg_value(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|pos| args.get(pos + 1))
        .cloned()
}

const RESURFACE_DELAY_MS: u64 = 1200;

fn launched_by_watcher() -> bool {
    arg_value("--data-dir").is_some()
}

fn surface_above_overlays(handle: &AppHandle) {
    if let Some(window) = handle.get_webview_window("main") {
        let _ = window.set_always_on_top(true);
        let _ = window.set_focus();
    }
}

fn resolve_data_dir() -> PathBuf {
    arg_value("--data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

fn startup_root() -> PathBuf {
    arg_value("--startup-root")
        .map(PathBuf::from)
        .unwrap_or_else(scanners::startup::default_startup_root)
}

fn tasks_root() -> PathBuf {
    arg_value("--tasks-root")
        .map(PathBuf::from)
        .unwrap_or_else(scanners::scheduled_tasks::default_tasks_root)
}

const SCAN_TARGET_TOTAL_MS: u64 = 5000;
const SCAN_MIN_PER_ITEM_MS: u64 = 15;
const SCAN_MAX_PER_ITEM_MS: u64 = 250;

// ---------------------------------------------------------------------------
// Live process enumeration (windows-only OS glue)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn enumerate_running_processes() -> Vec<ProcessInfo> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_NAME_WIN32};

    let mut results = Vec::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Ok(snapshot) = snapshot else { return results };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..unsafe { std::mem::zeroed() }
    };

    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
        return results;
    }

    loop {
        let pid = entry.th32ProcessID;
        let exe_wide: Vec<u16> = entry
            .szExeFile
            .iter()
            .take_while(|&&c| c != 0)
            .copied()
            .collect();
        let name = String::from_utf16_lossy(&exe_wide);

        // Resolve full exe path for scoring
        let mut exe_path = String::new();
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut size = buf.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    exe_path = String::from_utf16_lossy(&buf[..size as usize]).to_string();
                }
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }

        // Heuristic: does this process own a visible window?
        // (Checked later in the scoring pipeline, default to false here)
        let from_user_profile = exe_path
            .to_ascii_lowercase()
            .contains("appdata")
            || exe_path
                .to_ascii_lowercase()
                .contains("users");

        results.push(ProcessInfo {
            name,
            pid,
            exe_path,
            has_visible_window: false,
            from_user_profile,
        });

        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };

    // Mark processes that have visible windows
    mark_visible_processes(&mut results);

    results
}

/// Cross-reference our process list against visible windows from the overlay module.
#[cfg(windows)]
fn mark_visible_processes(processes: &mut [ProcessInfo]) {
    // Collect pids of processes that own a visible window
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};
    use windows::Win32::Foundation::{HWND, LPARAM};

    let mut visible_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    unsafe extern "system" fn collect_visible(hwnd: HWND, lparam: LPARAM) -> windows::Win32::Foundation::BOOL {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let set = &mut *(lparam.0 as *mut std::collections::HashSet<u32>);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 {
                    set.insert(pid);
                }
            }
        }
        true.into()
    }

    let set_ptr = &mut visible_pids as *mut _ as isize;
    unsafe { let _ = EnumWindows(Some(collect_visible), LPARAM(set_ptr)); }

    for p in processes.iter_mut() {
        if visible_pids.contains(&p.pid) {
            p.has_visible_window = true;
        }
    }
}

#[cfg(not(windows))]
fn enumerate_running_processes() -> Vec<ProcessInfo> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// Ransom-detection folder scanning (OS glue: read-only directory walks)
// ---------------------------------------------------------------------------

fn user_folder_candidates() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        let home = PathBuf::from(home);
        for sub in &["Desktop", "Documents", "Downloads"] {
            let p = home.join(sub);
            if p.is_dir() {
                folders.push(p);
            }
        }
    }
    folders
}

fn read_dir_entries(folder: &Path) -> Vec<cure_core::ransom_detect::DirEntry> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::fs::read_dir(folder)
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let age_days = e
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| (now.saturating_sub(d.as_secs())) / 86_400)
                        .unwrap_or(0);
                    Some(cure_core::ransom_detect::DirEntry { name, age_days })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Kill a process by PID.  Returns Ok(()) on success.
#[cfg(windows)]
fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle =
            OpenProcess(PROCESS_TERMINATE, false, pid).map_err(|e| format!("OpenProcess failed: {e}"))?;
        let _ = TerminateProcess(handle, 1);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
    }
    Ok(())
}

#[cfg(not(windows))]
fn kill_process_by_pid(_pid: u32) -> Result<(), String> {
    Err("process killing not supported on this platform".to_string())
}

#[derive(Debug, Clone, Serialize)]
struct ItemScannedEvent {
    stage: String,
    name: String,
    source: String,
    location: String,
    risk: String,
    score: i32,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessFlaggedEvent {
    stage: String,
    name: String,
    pid: u32,
    risk: String,
    score: i32,
}

#[derive(Debug, Clone, Serialize)]
struct RansomFoundEvent {
    stage: String,
    finding_type: String,
    path: String,
    detail: String,
}

#[tauri::command]
async fn run_auto_scan(app: AppHandle) -> Result<ScanSummary, String> {
    let data_dir = resolve_data_dir();
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("cannot create data dir: {e}"))?;

    emit_stage(&app, "registry", "Reading Run / RunOnce autoruns");
    #[allow(unused_mut)]
    let mut entries: Vec<PersistenceEntry> = Vec::new();
    #[cfg(windows)]
    entries.extend(scanners::registry::scan().unwrap_or_default());

    emit_stage(&app, "startup", "Walking the per-user Startup folder");
    entries.extend(scanners::startup::scan(&startup_root()));

    emit_stage(&app, "tasks", "Parsing scheduled task definitions");
    entries.extend(scanners::scheduled_tasks::scan(&tasks_root()));

    let count = entries.len();
    emit_stage(
        &app,
        "scoring",
        format!(
            "Risk-scoring {} persistence entr{}",
            count,
            if count == 1 { "y" } else { "ies" }
        ),
    );
    let scored: Vec<ScoredEntry> = entries
        .iter()
        .map(|e| {
            let exe_path = cure_core::signature::resolve_executable_path(&e.command);
            risk::score_entry(e, exe_path.as_deref())
        })
        .collect();

    emit_stage(&app, "item-scan", "Inspecting each persistence entry");

    let per_item_ms = (SCAN_TARGET_TOTAL_MS / std::cmp::max(count, 1) as u64)
        .clamp(SCAN_MIN_PER_ITEM_MS, SCAN_MAX_PER_ITEM_MS);

    let mut high_risk_cleaned = Vec::new();
    let mut suspicious_for_review = Vec::new();
    let mut safe = 0usize;

    for s in scored.iter() {
        let _ = app.emit(
            "scan-progress",
            ItemScannedEvent {
                stage: "item-scanned".to_string(),
                name: s.entry.name.clone(),
                source: s.entry.source.tag().to_string(),
                location: s.entry.location.clone(),
                risk: format!("{:?}", s.risk),
                score: s.score,
            },
        );
        tokio::time::sleep(std::time::Duration::from_millis(per_item_ms)).await;

        match s.risk {
            RiskLevel::HighRisk => match s.entry.source {
                PersistenceSource::RegistryRun => suspicious_for_review.push(s.clone()),
                _ => {
                    emit_stage(&app, "cleaning", format!("Auto-cleaning {}", s.entry.name));
                    match quarantine::quarantine_file(&data_dir, &s.entry) {
                        Ok(_) => high_risk_cleaned.push(s.clone()),
                        Err(err) => {
                            let mut flagged = s.clone();
                            flagged.reasons.push(format!("auto-clean failed: {err}"));
                            suspicious_for_review.push(flagged);
                        }
                    }
                }
            },
            RiskLevel::Suspicious => suspicious_for_review.push(s.clone()),
            RiskLevel::Safe => safe += 1,
        }
    }

    // ── live process scan ──────────────────────────────────────────────

    emit_stage(&app, "process-scan", "Enumerating running processes");
    let processes = enumerate_running_processes();
    let mut process_findings: Vec<ProcessFinding> = Vec::new();

    if !processes.is_empty() {
        emit_stage(
            &app,
            "process-scan",
            format!("Checking {} running processes", processes.len()),
        );

        // Scoring hits the disk (WinVerifyTrust + SHA-256 per exe), so run it
        // in blocking batches and surface progress between them — otherwise a
        // few hundred processes freeze the UI with no feedback for minutes.
        const BATCH: usize = 24;
        let mut scored_procs: Vec<(ProcessInfo, ProcessScore)> = Vec::new();
        for chunk in processes.chunks(BATCH) {
            let chunk = chunk.to_vec();
            let done = scored_procs.len();
            let total = processes.len();
            emit_stage(
                &app,
                "process-scan",
                format!("Scanning running processes {}–{} of {}", done + 1, done + chunk.len(), total),
            );
            let batch = tokio::task::spawn_blocking(move || {
                chunk
                    .into_iter()
                    .map(|p| {
                        let sig =
                            cure_core::signature::check_signature(std::path::Path::new(&p.exe_path));
                        let hash =
                            cure_core::hash_intel::check_hash(std::path::Path::new(&p.exe_path));
                        let ps = process_scan::score_process(&p, &sig, hash.as_deref());
                        (p, ps)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            match batch {
                Ok(part) => scored_procs.extend(part),
                Err(e) => return Err(format!("process scan failed: {e}")),
            }
        }

        let suspicious_idx = process_scan::pick_suspicious_processes(&scored_procs);
        for &idx in &suspicious_idx {
            let (info, ps) = &scored_procs[idx];
            let finding = ProcessFinding {
                name: info.name.clone(),
                pid: info.pid,
                exe_path: info.exe_path.clone(),
                score: ps.score,
                risk: format!("{:?}", ps.risk),
                reasons: ps.reasons.clone(),
            };
            let _ = app.emit(
                "scan-progress",
                ProcessFlaggedEvent {
                    stage: "process-flagged".to_string(),
                    name: info.name.clone(),
                    pid: info.pid,
                    risk: format!("{:?}", ps.risk),
                    score: ps.score,
                },
            );
            process_findings.push(finding);
        }
    }

    // ── ransom detection ───────────────────────────────────────────────

    emit_stage(&app, "ransom-detect", "Checking for ransom notes and mass encryption");
    let folders: Vec<(PathBuf, Vec<cure_core::ransom_detect::DirEntry>)> =
        user_folder_candidates()
            .into_iter()
            .map(|f| (f.clone(), read_dir_entries(&f)))
            .collect();

    let ransom_core_findings = ransom_detect::scan_folders(&folders);
    let mut ransom_findings: Vec<RansomFinding> = Vec::new();

    for f in &ransom_core_findings {
        let rf = match f {
            RansomFindingCore::Note(note) => {
                let snippet = ransom_detect::load_note_content(&note.path, 4096);
                let family = ransom_detect::guess_family(&snippet);
                let url = family.map(|_| "https://www.nomoreransom.org/".to_string());
                RansomFinding {
                    finding_type: "ransom-note".to_string(),
                    path: note.path.to_string_lossy().to_string(),
                    detail: if snippet.is_empty() {
                        format!("Matched pattern: {}", note.matched_stem)
                    } else {
                        format!("Matched pattern: {} — \"{}\"", note.matched_stem, &snippet[..snippet.len().min(120)])
                    },
                    suspected_family: family.map(|s| s.to_string()),
                    nomoreransom_url: url,
                }
            }
            RansomFindingCore::BulkEncryption(cluster) => RansomFinding {
                finding_type: "bulk-encryption".to_string(),
                path: cluster.folder.to_string_lossy().to_string(),
                detail: format!(
                    "{} files with unusual extension \".{}\" (avg age {} days)",
                    cluster.file_count, cluster.extension, cluster.avg_age_days
                ),
                suspected_family: None,
                nomoreransom_url: None,
            },
        };

        let _ = app.emit(
            "scan-progress",
            RansomFoundEvent {
                stage: "ransom-found".to_string(),
                finding_type: rf.finding_type.clone(),
                path: rf.path.clone(),
                detail: rf.detail.clone(),
            },
        );
        ransom_findings.push(rf);
    }

    emit_stage(&app, "done", "Saving baseline");
    baseline::save_baseline(&data_dir.join("baseline.json"), &entries)
        .map_err(|e| format!("cannot write baseline: {e}"))?;

    let total = high_risk_cleaned.len() + suspicious_for_review.len() + safe;
    Ok(ScanSummary {
        total,
        high_risk_cleaned,
        suspicious_for_review,
        safe,
        process_findings,
        ransom_findings,
    })
}

fn find_current_entry(id: &str) -> Option<PersistenceEntry> {
    scanners::collect_all(&startup_root(), &tasks_root())
        .into_iter()
        .find(|entry| entry.id == id)
}

#[tauri::command]
fn quarantine_entry(id: String, name: String, command: String) -> Result<String, String> {
    let data_dir = resolve_data_dir();
    let entry = find_current_entry(&id).unwrap_or_else(|| {
        PersistenceEntry::new(PersistenceSource::StartupFolder, &name, &command, &command)
    });
    let record = quarantine::quarantine_file(&data_dir, &entry).map_err(|e| e.to_string())?;
    Ok(format!(
        "moved {} -> {}",
        record.original_path.display(),
        record.quarantine_path.display()
    ))
}

#[tauri::command]
fn undo_entry(id: String) -> Result<(), String> {
    quarantine::undo(&resolve_data_dir(), &id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn kill_high_risk_processes(processes: Vec<(String, u32)>) -> Result<KillReport, String> {
    let mut killed = Vec::new();
    let mut failed = Vec::new();

    for (name, pid) in processes {
        match kill_process_by_pid(pid) {
            Ok(()) => {
                killed.push(ProcessFinding {
                    name: name.clone(),
                    pid,
                    exe_path: String::new(),
                    score: 0,
                    risk: "HighRisk".to_string(),
                    reasons: vec!["killed by user request".to_string()],
                });
            }
            Err(e) => {
                failed.push(format!("{name} (pid {pid}): {e}"));
            }
        }
    }

    Ok(KillReport { killed, failed })
}

#[tauri::command]
fn open_quarantine_folder() -> Result<String, String> {
    let dir = resolve_data_dir().join("quarantine");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create quarantine folder: {e}"))?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("cannot open explorer: {e}"))?;
    Ok(dir.to_string_lossy().into_owned())
}

#[tauri::command]
fn view_log() -> Result<String, String> {
    let path = resolve_data_dir().join("baseline.json");
    if !path.exists() {
        return Err("No scan log yet — run a scan first".to_string());
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| format!("cannot open log viewer: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn exit_app(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------
// disk cleanup
// ---------------------------------------------------------------------------

const CLEANUP_DOWNLOADS_AGE_DAYS: u32 = 30;

#[derive(Serialize)]
struct CleanupCategorySummary {
    key: String,
    label: String,
    item_count: usize,
    total_bytes: u64,
}

#[derive(Serialize)]
struct CleanupDownloadItem {
    path: String,
    name: String,
    size_bytes: u64,
    age_days: u64,
}

#[derive(Serialize)]
struct CleanupScanSummary {
    categories: Vec<CleanupCategorySummary>,
    downloads: Vec<CleanupDownloadItem>,
    total_bytes: u64,
}

fn downloads_age_days(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|md| md.modified().ok())
        .and_then(|m| std::time::SystemTime::now().duration_since(m).ok())
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

fn scan_cleanup_candidates() -> (Vec<disk_cleanup::CleanupCandidate>, Vec<disk_cleanup::CleanupCandidate>) {
    let safe = disk_cleanup::scan_all();
    let downloads = disk_cleanup::scan_old_downloads(CLEANUP_DOWNLOADS_AGE_DAYS);
    (safe, downloads)
}

#[tauri::command]
fn scan_cleanup() -> Result<CleanupScanSummary, String> {
    let (safe, downloads) = scan_cleanup_candidates();

    let categories = disk_cleanup::summarize(&safe)
        .into_iter()
        .filter(|row| row.category != disk_cleanup::CleanupCategory::DownloadsInstaller)
        .map(|row| CleanupCategorySummary {
            key: row.category.key().to_string(),
            label: row.category.label().to_string(),
            item_count: row.item_count,
            total_bytes: row.total_bytes,
        })
        .collect();

    let download_items: Vec<CleanupDownloadItem> = downloads
        .iter()
        .map(|candidate| CleanupDownloadItem {
            path: candidate.path.to_string_lossy().into_owned(),
            name: candidate
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            size_bytes: candidate.size_bytes,
            age_days: downloads_age_days(&candidate.path),
        })
        .collect();

    let total_bytes = safe.iter().map(|c| c.size_bytes).sum::<u64>()
        + downloads.iter().map(|c| c.size_bytes).sum::<u64>();

    Ok(CleanupScanSummary {
        categories,
        downloads: download_items,
        total_bytes,
    })
}

#[derive(Serialize)]
struct CleanupResultDto {
    attempted: usize,
    deleted: usize,
    failed: usize,
    bytes_freed: u64,
    failures: Vec<CleanupFailureDto>,
}

#[derive(Serialize)]
struct CleanupFailureDto {
    path: String,
    reason: String,
}

#[tauri::command]
fn run_cleanup(
    categories: Vec<String>,
    download_paths: Vec<String>,
) -> Result<CleanupResultDto, String> {
    let (safe, downloads) = scan_cleanup_candidates();

    let mut targets: Vec<disk_cleanup::CleanupCandidate> = safe
        .into_iter()
        .filter(|c| categories.iter().any(|key| key == c.category.key()))
        .collect();
    targets.extend(downloads.into_iter().filter(|candidate| {
        let as_str = candidate.path.to_string_lossy();
        download_paths.iter().any(|p| p == &as_str)
    }));

    let result = disk_cleanup::delete_candidates(&targets);
    Ok(CleanupResultDto {
        attempted: result.attempted,
        deleted: result.deleted,
        failed: result.failed,
        bytes_freed: result.bytes_freed,
        failures: result
            .failures
            .into_iter()
            .map(|f| CleanupFailureDto {
                path: f.path.to_string_lossy().into_owned(),
                reason: f.reason,
            })
            .collect(),
    })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            run_auto_scan,
            dismiss_overlays,
            kill_high_risk_processes,
            quarantine_entry,
            undo_entry,
            open_quarantine_folder,
            view_log,
            exit_app,
            scan_cleanup,
            run_cleanup,
            start_canary_guard,
            stop_canary_guard,
            canary_status
        ])
        .manage(CanaryState::new())
        .setup(|app| {
            if launched_by_watcher() {
                let handle = app.handle().clone();
                surface_above_overlays(&handle);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(RESURFACE_DELAY_MS));
                    surface_above_overlays(&handle);
                });
            }
            maybe_start_e2e_driver(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running cure-gui");
}

// ---------------------------------------------------------------------------
// suspicious-overlay dismissal (runs when the user presses Start Rescue)
//
// OS glue only: enumeration, style/process interrogation, close/terminate.
// The DECISION (which windows deserve closing) lives in
// cure_core::overlay and is unit-tested there.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ClosedOverlay {
    title: String,
    process: String,
    signature: String,
    /// true = WM_CLOSE was ignored and the process had to be terminated.
    terminated: bool,
}

#[derive(Serialize)]
struct DismissReport {
    checked: usize,
    closed: Vec<ClosedOverlay>,
}

#[cfg(windows)]
fn is_under_windows_dir(path: &Path) -> bool {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
    path.as_os_str()
        .to_string_lossy()
        .to_ascii_lowercase()
        .starts_with(&windir.to_ascii_lowercase())
}

#[cfg(windows)]
fn overlay_log_path() -> PathBuf {
    resolve_data_dir().join("overlay-dismissal.log")
}

#[cfg(windows)]
fn log_overlay_action(line: &str) {
    use std::io::Write as _;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(overlay_log_path())
    {
        let _ = writeln!(f, "{secs} [overlay] {line}");
    }
}

/// Enumerate visible top-level windows and describe each one. Pure glue:
/// every judgement call is delegated to cure_core::overlay.
#[cfg(windows)]
fn collect_window_candidates()
    -> Result<Vec<(isize, WindowDesc, SignatureStatus)>, String>
{
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE, WS_CAPTION,
        WS_EX_TOPMOST,
    };

    let mut out: Vec<(isize, WindowDesc, SignatureStatus)> = Vec::new();
    let own_exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());

    extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let list = lparam.0 as *mut Vec<isize>;
            if IsWindowVisible(hwnd).as_bool() {
                (*list).push(hwnd.0 as isize);
            }
        }
        true.into()
    }

    let mut hwnds: Vec<isize> = Vec::new();
    let list_ptr = &mut hwnds as *mut _ as isize;
    unsafe {
        EnumWindows(Some(callback), LPARAM(list_ptr))
            .map_err(|e| format!("EnumWindows failed: {e}"))?;
    }

    for raw in hwnds {
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        unsafe {
            let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let is_topmost = exstyle & WS_EX_TOPMOST.0 != 0;
            let is_borderless = style & WS_CAPTION.0 == 0;

            let len = GetWindowTextLengthW(hwnd);
            let mut buf = vec![0u16; (len + 1) as usize];
            GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..len as usize]);

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                continue;
            }
            let process_path = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(handle) => {
                    let mut buf = [0u16; 1024];
                    let mut size = buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buf.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok()
                    {
                        PathBuf::from(String::from_utf16_lossy(&buf[..size as usize]))
                    } else {
                        continue; // cannot attribute the window -> leave it alone
                    }
                }
                Err(_) => continue, // protected process -> leave it alone
            };

            let canonical = std::fs::canonicalize(&process_path).unwrap_or_else(|_| process_path.clone());
            let is_own = own_exe.as_ref() == Some(&canonical);
            let is_system = is_under_windows_dir(&process_path);
            let signature = cure_core::signature::check_signature(&process_path);

            out.push((
                raw,
                WindowDesc {
                    title,
                    process_path,
                    is_topmost,
                    is_borderless,
                    is_own_process: is_own,
                    is_system_window: is_system,
                },
                signature,
            ));
        }
    }
    Ok(out)
}

/// WM_CLOSE first; if the window survives 500ms, terminate its process.
#[cfg(windows)]
fn close_overlay(hwnd_raw: isize) -> bool {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_TERMINATE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_CLOSE};
    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return true; // already gone
        }
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        std::thread::sleep(std::time::Duration::from_millis(500));
        if IsWindow(hwnd).as_bool() {
            let mut pid = 0u32;
            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                hwnd,
                Some(&mut pid),
            );
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(handle, 1);
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return true;
            }
            return false;
        }
        true
    }
}

#[tauri::command]
fn dismiss_overlays() -> Result<DismissReport, String> {
    #[cfg(not(windows))]
    {
        return Ok(DismissReport {
            checked: 0,
            closed: Vec::new(),
        });
    }

    #[cfg(windows)]
    {
        let candidates = collect_window_candidates()?;
        let checked = candidates.len();
        let picks = overlay::pick_overlays(
            &candidates
                .iter()
                .map(|(_, desc, sig)| (desc.clone(), sig.clone()))
                .collect::<Vec<_>>(),
        );

        let mut closed = Vec::new();
        for idx in picks {
            let (hwnd_raw, desc, sig) = &candidates[idx];
            let process = desc.process_name();
            let sig_text = match sig {
                SignatureStatus::ValidSigned => "signed",
                SignatureStatus::Invalid => "INVALID signature",
                SignatureStatus::Unsigned => "unsigned",
                SignatureStatus::Unknown => "unverifiable",
            }
            .to_string();
            let went_away = close_overlay(*hwnd_raw);
            let terminated = !went_away;
            log_overlay_action(&format!(
                "closed window {:?} (process {}, {}{})",
                desc.title,
                desc.process_path.display(),
                sig_text,
                if terminated { "; process TERMINATED after WM_CLOSE was ignored" } else { "" }
            ));
            closed.push(ClosedOverlay {
                title: desc.title.clone(),
                process,
                signature: sig_text,
                terminated,
            });
        }
        Ok(DismissReport { checked, closed })
    }
}

// ---------------------------------------------------------------------------
// E2E driver (test-only, inert unless CURE_E2E_CLEANUP is set)
//
// Drives the REAL webview UI (real invoke() plumbing, real backend deletes)
// through one full disk-cleanup flow, then emits the outcome as an event and
// exits. Never active in normal launches; exists to close the audit gap of
// cleanup being verified mock-only.
// ---------------------------------------------------------------------------

const E2E_RUNNER_JS: &str = r##"(async () => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, t = 60000) => {
    for (let i = 0; i < t / 100; i++) { if (f()) return true; await wait(100); }
    return false;
  };
  const emit = (payload) => window.__TAURI__.event.emit("e2e-done", JSON.stringify(payload));
  try {
    if (!await waitFor(() => document.getElementById("start-rescue-btn") !== null)) {
      throw new Error("start-rescue button never appeared");
    }
    await wait(300);
    document.getElementById("start-rescue-btn").click();
    if (!await waitFor(() => document.getElementById("results-view") !== null)) {
      throw new Error("app DOM never became ready");
    }
    if (!await waitFor(() => !document.getElementById("results-view").classList.contains("hidden"))) {
      throw new Error("security results never appeared");
    }
    await wait(600);
    document.getElementById("open-cleanup").click();
    if (!await waitFor(() =>
      !document.getElementById("cleanup-view").classList.contains("hidden") &&
      !document.getElementById("cleanup-idle").classList.contains("hidden"))) {
      throw new Error("cleanup view never showed idle state");
    }
    document.getElementById("cleanup-scan-btn").click();
    if (!await waitFor(() =>
      !document.getElementById("cleanup-body").classList.contains("hidden"))) {
      throw new Error("cleanup scan never loaded");
    }
    await wait(400);
    const boxes = document.querySelectorAll("#cleanup-dl-list input[type=checkbox]");
    if (boxes.length) boxes[0].click();
    const btn = document.getElementById("cleanup-btn");
    btn.click();
    await wait(150);
    btn.click();
    if (!await waitFor(() => document.getElementById("cleanup-status").textContent.startsWith("Freed"), 60000)) {
      throw new Error("cleanup status never showed Freed");
    }
    await wait(300);
    emit({
      ok: true,
      status: document.getElementById("cleanup-status").textContent,
      pill: document.getElementById("cleanup-status-text").textContent,
      downloadsTicked: boxes.length > 0,
      tossSeen: window.__cureTossSeen === true,
      failures: Array.from(document.querySelectorAll("#cleanup-failures li")).map((li) => li.textContent),
    });
  } catch (err) {
    emit({ ok: false, error: String(err) });
  }
})();"##;

fn maybe_start_e2e_driver(handle: AppHandle) {
    if std::env::var("CURE_E2E_CLEANUP").is_err() {
        return;
    }
    let out_path = std::env::var("CURE_E2E_OUT")
        .unwrap_or_else(|_| "e2e-result.json".to_string());
    let listen_handle = handle.clone();
    handle.listen("e2e-done", move |event| {
        let _ = std::fs::write(&out_path, event.payload());
        listen_handle.exit(0);
    });
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.eval(E2E_RUNNER_JS);
        }
    });
}

// ---------------------------------------------------------------------------
// Ransomware canary guard — real-time folder monitoring + process tripwire
// ---------------------------------------------------------------------------

struct CanaryGuard {
    stop: Arc<AtomicBool>,
    alert_count: Arc<AtomicUsize>,
}

struct CanaryState {
    guard: Mutex<Option<CanaryGuard>>,
}

impl CanaryState {
    fn new() -> Self {
        Self { guard: Mutex::new(None) }
    }
}

#[derive(serde::Serialize, Clone)]
struct CanaryAlertEvent {
    kind: String,
    folder: String,
    file: String,
    action: String,
    at_secs: u64,
    severity: u8,
}

#[tauri::command]
fn start_canary_guard(state: State<'_, CanaryState>, app: AppHandle) -> Result<String, String> {
    {
        let g = state.guard.lock().map_err(|e| e.to_string())?;
        if g.is_some() {
            return Ok("already-active".into());
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let alert_count = Arc::new(AtomicUsize::new(0));

    let dirs = user_folder_candidates();
    for dir in &dirs {
        if dir.is_dir() {
            plant_decoys(dir);
        }
    }

    let watch_dirs: Vec<PathBuf> = dirs.into_iter().filter(|d| d.is_dir()).collect();
    for dir in &watch_dirs {
        spawn_dir_watcher(dir.clone(), app.clone(), stop.clone(), alert_count.clone());
    }

    spawn_tripwire_poller(app.clone(), stop.clone(), alert_count.clone());

    {
        let mut g = state.guard.lock().map_err(|e| e.to_string())?;
        *g = Some(CanaryGuard { stop, alert_count });
    }
    Ok("started".into())
}

#[tauri::command]
fn stop_canary_guard(state: State<'_, CanaryState>) -> Result<String, String> {
    let mut g = state.guard.lock().map_err(|e| e.to_string())?;
    if let Some(guard) = g.take() {
        guard.stop.store(true, Ordering::SeqCst);
        Ok("stopped".into())
    } else {
        Ok("not-active".into())
    }
}

#[tauri::command]
fn canary_status(state: State<'_, CanaryState>) -> Result<serde_json::Value, String> {
    let g = state.guard.lock().map_err(|e| e.to_string())?;
    let active = g.is_some();
    let count = g.as_ref()
        .map(|g| g.alert_count.load(Ordering::Relaxed))
        .unwrap_or(0);
    Ok(serde_json::json!({ "active": active, "alert_count": count }))
}

fn plant_decoys(dir: &Path) {
    let names = decoy_names(6);
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
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn spawn_dir_watcher(
    dir: PathBuf,
    app: AppHandle,
    stop: Arc<AtomicBool>,
    _alert_count: Arc<AtomicUsize>,
) {
    std::thread::spawn(move || {
        use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, OPEN_EXISTING,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE,
            ReadDirectoryChangesW,
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FILE_NOTIFY_INFORMATION, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
            FILE_ACTION_RENAMED_OLD_NAME, FILE_ACTION_RENAMED_NEW_NAME,
        };
        use windows::Win32::System::IO::{OVERLAPPED, GetOverlappedResult};
        use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
        use windows::Win32::Foundation::BOOL;

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

        while !stop.load(Ordering::Relaxed) {
            let mut bytes_returned = 0u32;
            let mut overlapped = OVERLAPPED::default();
            overlapped.hEvent = h_event;

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
                let _ = GetOverlappedResult(h_dir, &mut overlapped, &mut bytes_returned, BOOL::from(false));
            }

            if bytes_returned == 0 {
                continue;
            }

            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let mut offset: usize = 0;
            let folder_str = dir.to_string_lossy().to_string();
            let buf_end = bytes_returned as usize;

            while offset + 12 <= buf_end {
                let info = unsafe {
                    &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION)
                };
                let name_len = info.FileNameLength as usize;
                // FileName starts at byte 12 (after NextEntryOffset + Action + FileNameLength)
                let name_byte_offset = offset + 12;
                if name_byte_offset + name_len > buf_end {
                    break;
                }
                let name_bytes = unsafe {
                    std::slice::from_raw_parts(
                        buffer.as_ptr().add(name_byte_offset),
                        name_len,
                    )
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
                    at_secs: now_secs,
                    folder: folder_str.clone(),
                    name,
                    kind,
                };

                let alerts = engine.observe(ev);
                for alert in alerts {
                    let event = match &alert {
                        CanaryAlert::CanaryTamper { folder, file, action, at_secs } => {
                            CanaryAlertEvent {
                                kind: "canary-tamper".into(),
                                folder: folder.clone(),
                                file: file.clone(),
                                action: (*action).into(),
                                at_secs: *at_secs,
                                severity: alert.severity(),
                            }
                        }
                        CanaryAlert::BurstEncryption { folder, distinct_files, window_secs, at_secs } => {
                            CanaryAlertEvent {
                                kind: "burst-encryption".into(),
                                folder: folder.clone(),
                                file: format!("{distinct_files} files in {window_secs}s"),
                                action: "burst".into(),
                                at_secs: *at_secs,
                                severity: alert.severity(),
                            }
                        }
                        CanaryAlert::ExtensionRewrite { folder, extension, renamed_count, at_secs } => {
                            CanaryAlertEvent {
                                kind: "extension-rewrite".into(),
                                folder: folder.clone(),
                                file: format!(".{extension}"),
                                action: format!("{renamed_count} files renamed"),
                                at_secs: *at_secs,
                                severity: alert.severity(),
                            }
                        }
                    };
                    let _ = app.emit("canary-alert", &event);
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
fn spawn_dir_watcher(
    _dir: PathBuf,
    _app: AppHandle,
    stop: Arc<AtomicBool>,
    _alert_count: Arc<AtomicUsize>,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

fn spawn_tripwire_poller(app: AppHandle, stop: Arc<AtomicBool>, _alert_count: Arc<AtomicUsize>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            #[cfg(windows)]
            {
                let processes = enumerate_running_processes();
                for p in &processes {
                    let name = std::path::Path::new(&p.exe_path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if let Some(reason) = shadow_wipe_reason(&name, "") {
                        let _ = app.emit("canary-alert", CanaryAlertEvent {
                            kind: "shadow-wipe".into(),
                            folder: String::new(),
                            file: name,
                            action: reason.into(),
                            at_secs: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0),
                            severity: 3,
                        });
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

#[cfg(test)]
#[cfg(windows)]
mod overlay_fixture_tests {
    use super::*;
    use std::process::Command;

    fn fake_overlay_bin() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testing/fake-overlay/target/release/fake-overlay.exe")
    }

    #[test]
    fn overlay_fixture_dismisses_fake_overlay_and_spares_notepad() {
        let bin = fake_overlay_bin();
        assert!(bin.exists(), "fake-overlay.exe not built yet — run: cargo build --release -p fake-overlay");

        // 1. Spawn fake-overlay (borderless topmost window, unsigned binary)
        let mut overlay_proc = Command::new(&bin).spawn().expect("spawn fake-overlay");
        std::thread::sleep(std::time::Duration::from_secs(2));

        // 2. Spawn notepad (legitimate topmost=false, borderless=false)
        let mut notepad_proc = Command::new("notepad.exe").spawn().expect("spawn notepad");
        std::thread::sleep(std::time::Duration::from_secs(1));

        // 3. Collect all window candidates
        let candidates = collect_window_candidates().expect("collect_window_candidates failed");
        assert!(candidates.len() >= 2, "expected at least 2 window candidates, got {}", candidates.len());

        // 4. Check that pick_overlays flags the fake-overlay but not notepad
        let scored: Vec<(WindowDesc, SignatureStatus)> = candidates
            .iter()
            .map(|(_, desc, sig)| (desc.clone(), sig.clone()))
            .collect();
        let picks = overlay::pick_overlays(&scored);

        let overlay_name = bin.file_stem().unwrap().to_string_lossy().to_string();
        let mut found_overlay = false;
        let mut found_notepad = false;
        for &idx in &picks {
            let (_, desc, _) = &candidates[idx];
            if desc.process_name().to_ascii_lowercase().contains(&overlay_name.to_ascii_lowercase()) {
                found_overlay = true;
            }
        }
        // Notepad should NOT be in the picks
        for &idx in &picks {
            let (_, desc, _) = &candidates[idx];
            assert!(
                !desc.process_name().to_ascii_lowercase().contains("notepad"),
                "notepad was incorrectly flagged for dismissal"
            );
        }
        assert!(found_overlay, "fake-overlay was not detected as a suspicious overlay");

        // 5. Close the fake-overlay
        let (hwnd_raw, _, _) = candidates.iter().find(|(_, desc, _)| {
            desc.process_name().to_ascii_lowercase().contains(&overlay_name.to_ascii_lowercase())
        }).expect("fake-overlay hwnd not found");
        let closed = close_overlay(*hwnd_raw);
        assert!(closed, "fake-overlay window was not closed");
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert!(!is_window_alive(*hwnd_raw), "fake-overlay process still alive after close_overlay");

        // 6. Kill notepad (cleanup)
        let _ = notepad_proc.kill();
    }

    fn is_window_alive(hwnd_raw: isize) -> bool {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        unsafe { IsWindow(hwnd).as_bool() }
    }
}
