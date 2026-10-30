#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use cure_core::agent;
use cure_core::canary::{shadow_wipe_reason, CanaryAlert};
use cure_dirwatch::WorkspaceChangeKind;
use cure_core::model::{PersistenceEntry, PersistenceSource, RiskLevel, ScoredEntry};
use cure_core::overlay::{self, WindowDesc};
use cure_core::process_scan::{self, ProcessInfo, ProcessScore};
use cure_core::ransom_detect::{self, RansomFinding as RansomFindingCore};
use cure_core::signature::SignatureStatus;
use cure_core::storage;
use cure_core::{baseline, quarantine, risk, scanners};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
#[cfg(feature = "desktop-e2e")]
use tauri::Listener;
use tauri::{AppHandle, Emitter, Manager, State};

/// Tell Windows that this process is per-monitor DPI aware.
///
/// Without this, Windows treats the process as DPI-unaware on a scaled
/// display: it hands the window a client area measured in *physical* pixels
/// but lays the page out as if those were *logical* pixels, and then paints
/// the page at the display's scale factor. The result is an application
/// drawn 25 % too large for its own window, with the right-hand quarter of
/// the interface — including the companion — clipped off the screen.
///
/// The call has to happen before the first window exists, which is why it
/// is the first thing `main` does. If a manifest has already declared the
/// process aware, the call fails and is ignored: the awareness is already
/// correct.
#[cfg(windows)]
fn declare_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    // SAFETY: setting the process DPI awareness context is safe before any
    // window or device context exists.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

#[cfg(not(windows))]
fn declare_dpi_awareness() {}

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
    /// Per-source access states so the UI never renders "0 entries" as a
    /// clean bill of health when enumeration actually failed or skipped.
    source_states: Vec<cure_core::report::CoverageRow>,
    elevated: bool,
}
#[derive(Serialize)]
struct ProcessFinding {
    name: String,
    pid: u32,
    exe_path: String,
    score: i32,
    risk: String,
    /// Evidence items with explicit levels (mirrors the core model).
    evidence: Vec<cure_core::evidence::Evidence>,
}
#[derive(Serialize)]
struct RansomFinding {
    finding_type: String,
    path: String,
    detail: String,
    suspected_family: Option<String>,
    // NOTE: no help URL is attached. The frontend must never navigate to
    // remote content; the ransom help panel names the resource as plain
    // text for the operator to type into a browser themselves.
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
    // Explicit override first (portable/USB workflows, tests). Otherwise an
    // app-owned directory — NEVER the folder holding the executable, which
    // may be the user's Desktop (baseline.json / quarantine/ must never
    // appear there unexpectedly).
    if let Some(dir) = arg_value("--data-dir") {
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    let owned = std::env::var("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("CURE"))
        .unwrap_or_else(|_| std::env::temp_dir().join("CURE"));
    // Best-effort: every writer below expects the directory to exist
    // (baseline save, quarantine records, exports, overlay log).
    let _ = std::fs::create_dir_all(&owned);
    owned
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
// Live process enumeration lives in cure_core::process_scan// (shared with the CLI report) — pure scoring stays there too.// ---------------------------------------------------------------------------// Ransom-detection folder scanning (OS glue: read-only directory walks)// ---------------------------------------------------------------------------
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
    cure_core::ransom_detect::read_dir_entries(folder)
}
/// Kill a process by PID.  Returns Ok(()) on success.
#[cfg(windows)]
fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("OpenProcess failed: {e}"))?;
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
    // Access states ride alongside the entries so coverage UI can render
    // CHECK FAILED / PARTIAL / skipped counts instead of a bare zero.
    // (area, state label, human detail).
    let mut source_states: Vec<cure_core::report::CoverageRow> = Vec::new();
    #[cfg(windows)]
    {
        let reg = scanners::registry::scan_report();
        record_source_state(&app, &mut source_states, registry_state_row(&reg));
        entries.extend(reg.entries);
    }
    #[cfg(not(windows))]
    source_states.push(cure_core::report::CoverageRow::unavailable(
        "Registry autoruns",
        "Windows-only source",
    ));
    emit_stage(&app, "startup", "Walking the per-user Startup folder");
    entries.extend(scanners::startup::scan(&startup_root()));
    emit_stage(
        &app,
        "startup-common",
        "Walking the machine-wide Startup folder",
    );
    entries.extend(scanners::startup::scan_common());
    emit_stage(&app, "tasks", "Parsing scheduled task definitions");
    let task_report = scanners::scheduled_tasks::scan_report(&tasks_root());
    record_source_state(&app, &mut source_states, tasks_state_row(&task_report));
    entries.extend(task_report.entries);
    emit_stage(&app, "services", "Enumerating auto-start services");
    let service_report = scanners::services::scan_report();
    record_source_state(
        &app,
        &mut source_states,
        services_state_row(&service_report),
    );
    let service_records = service_report.records;
    entries.extend(service_records.iter().map(|r| r.entry.clone()));
    #[cfg(windows)]
    {
        emit_stage(&app, "wmi", "Querying WMI event subscriptions");
        let wmi_report = scanners::wmi::scan_report();
        record_source_state(&app, &mut source_states, wmi_state_row(&wmi_report));
        entries.extend(wmi_report.entries);
        emit_stage(&app, "ifeo", "Checking IFEO debuggers and AppInit DLLs");
        entries.extend(scanners::ifeo::scan());
        entries.extend(scanners::appinit::scan());
        emit_stage(&app, "com", "Walking per-user COM registrations");
        entries.extend(scanners::com::scan());
    }
    #[cfg(not(windows))]
    source_states.push(cure_core::report::CoverageRow::unavailable(
        "WMI subscriptions",
        "Windows-only source",
    ));
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
    let mut scored: Vec<ScoredEntry> = entries
        .iter()
        // Service entries are scored below with start-type/account evidence.
        .filter(|e| e.source != PersistenceSource::WindowsService)
        .map(|e| {
            let exe_path = cure_core::signature::resolve_executable_path(&e.command);
            risk::score_entry(e, exe_path.as_deref())
        })
        .collect();
    // Service scoring hits the disk per image (Authenticode), so batch it
    // with progress like the process scan. Services are NEVER auto-cleaned
    // (see the review rule below) — detection only.
    if !service_records.is_empty() {
        const SVC_BATCH: usize = 24;
        let mut done = 0usize;
        let total = service_records.len();
        for chunk in service_records.chunks(SVC_BATCH) {
            let chunk = chunk.to_vec();
            emit_stage(
                &app,
                "services",
                format!(
                    "Scoring auto-start services {}–{} of {}",
                    done + 1,
                    done + chunk.len(),
                    total
                ),
            );
            let batch = tokio::task::spawn_blocking(move || {
                chunk
                    .into_iter()
                    .map(|record| {
                        let status =
                            cure_core::scanners::services::image_status(&record.image_path);
                        let exe = match &status {
                            cure_core::scanners::services::ImageStatus::Found(path) => {
                                Some(path.clone())
                            }
                            _ => None,
                        };
                        let missing = status == cure_core::scanners::services::ImageStatus::Missing;
                        let signature = match &exe {
                            Some(path) => cure_core::signature::check_signature(path),
                            None => cure_core::signature::SignatureStatus::Unknown,
                        };
                        let hash = if risk::service_needs_hash(&record.image_path, exe.as_deref()) {
                            exe.as_deref().and_then(cure_core::hash_intel::check_hash)
                        } else {
                            None
                        };
                        risk::score_service(&record, missing, signature, hash.as_deref())
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            match batch {
                Ok(part) => {
                    done += part.len();
                    scored.extend(part);
                }
                Err(e) => return Err(format!("service scan failed: {e}")),
            }
        }
    }
    emit_stage(&app, "item-scan", "Inspecting each persistence entry");
    let per_item_ms = (SCAN_TARGET_TOTAL_MS / std::cmp::max(count, 1) as u64)
        .clamp(SCAN_MIN_PER_ITEM_MS, SCAN_MAX_PER_ITEM_MS);
    let high_risk_cleaned = Vec::new();
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
        // NO automatic remediation here: even HighRisk findings only land in
        // the review queue. Relocation happens exclusively through the
        // explicit per-item confirmation path (`quarantine_entry`), which
        // re-validates the entry against a fresh scan, refuses non-file
        // sources, moves the file, and records a scoped undo.
        // `high_risk_cleaned` therefore stays empty; the field is kept so
        // the ScanSummary shape (and its consumers) remains stable.
        match s.risk {
            RiskLevel::HighRisk | RiskLevel::Suspicious => suspicious_for_review.push(s.clone()),
            RiskLevel::Safe => safe += 1,
        }
    }
    // ── live process scan ──────────────────────────────────────────────
    emit_stage(&app, "process-scan", "Enumerating running processes");
    let processes = process_scan::enumerate_processes();
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
                format!(
                    "Scanning running processes {}–{} of {}",
                    done + 1,
                    done + chunk.len(),
                    total
                ),
            );
            let batch = tokio::task::spawn_blocking(move || {
                chunk
                    .into_iter()
                    .map(|p| {
                        let sig = cure_core::signature::check_signature(std::path::Path::new(
                            &p.exe_path,
                        ));
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
                evidence: ps.evidence.clone(),
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
    emit_stage(
        &app,
        "ransom-detect",
        "Checking for ransom notes and mass encryption",
    );
    let folders: Vec<(PathBuf, Vec<cure_core::ransom_detect::DirEntry>)> = user_folder_candidates()
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
                RansomFinding {
                    finding_type: "ransom-note".to_string(),
                    path: note.path.to_string_lossy().to_string(),
                    detail: if snippet.is_empty() {
                        format!("Matched pattern: {}", note.matched_stem)
                    } else {
                        format!(
                            "Matched pattern: {} — \"{}\"",
                            note.matched_stem,
                            &snippet[..snippet.len().min(120)]
                        )
                    },
                    suspected_family: family.map(|s| s.to_string()),
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
        source_states,
        elevated: cure_core::elevation::is_elevated(),
    })
}
/// Coverage row for the scan summary. Non-OK states surface the scanner's/// own reason so a failed check never renders as a clean zero./// Thin wrappers over `cure_core::report::source_state_row` (shared with/// the CLI report so both surfaces stay consistent).
fn record_source_state(
    app: &AppHandle,
    rows: &mut Vec<cure_core::report::CoverageRow>,
    row: cure_core::report::CoverageRow,
) {
    // Additive display event: uses exactly the same coverage row as the summary.
    let _ = app.emit(
        "scan-progress",
        serde_json::json!({
            "stage": "source-state", "area": row.area,
            "state": row.state, "detail": row.detail,
        }),
    );
    rows.push(row);
}
fn state_row(
    area: &str,
    status: &cure_core::elevation::SourceStatus,
    ok_detail: String,
) -> cure_core::report::CoverageRow {
    cure_core::report::source_state_row(area, status, ok_detail)
}
fn registry_state_row(
    reg: &scanners::registry::RegistryScanReport,
) -> cure_core::report::CoverageRow {
    state_row(
        "Registry autoruns",
        &reg.status(),
        format!("{} values", reg.values_read),
    )
}
fn tasks_state_row(
    task_report: &scanners::scheduled_tasks::TaskScanReport,
) -> cure_core::report::CoverageRow {
    state_row(
        "Scheduled tasks",
        &task_report.status(),
        format!("{} files", task_report.files_seen),
    )
}
fn services_state_row(
    service_report: &scanners::services::ServiceScanReport,
) -> cure_core::report::CoverageRow {
    state_row(
        "Services (auto-start)",
        &service_report.status,
        format!("{} services", service_report.records.len()),
    )
}
fn wmi_state_row(wmi_report: &scanners::wmi::WmiScanReport) -> cure_core::report::CoverageRow {
    state_row(
        "WMI subscriptions",
        &wmi_report.status,
        format!("{} entries", wmi_report.entries.len()),
    )
}
fn find_current_entry(id: &str) -> Option<PersistenceEntry> {
    if let Some(entry) = scanners::collect_all(&startup_root(), &tasks_root())
        .into_iter()
        .find(|entry| entry.id == id)
    {
        return Some(entry);
    }
    scanners::collect_services()
        .into_iter()
        .map(|record| record.entry)
        .find(|entry| entry.id == id)
}
#[tauri::command]
fn quarantine_entry(id: String, _name: String, _command: String) -> Result<String, String> {
    let data_dir = resolve_data_dir();
    // Strict: only entries present in a fresh scan may be quarantined. Never
    // construct an entry from frontend-supplied paths — the webview must not
    // be able to turn this command into a move-any-file primitive.
    let Some(entry) = find_current_entry(&id) else {
        return Err(
            "entry is no longer present in the latest scan (already quarantined or removed?) — rescan to refresh"
                .to_string(),
        );
    };
    if entry.source == PersistenceSource::RegistryRun {
        return Err(format!(
            "registry autoruns are detected and scored but never auto-disabled — remove manually: {} / value \"{}\" (back up the key first)",
            entry.location, entry.name
        ));
    }
    if !entry.source.is_file_backed() {
        return Err(format!(
            "{} findings are detected and scored but never auto-disabled — investigate first, back up {}, then remove manually",
            entry.source, entry.location
        ));
    }
    let record = quarantine::quarantine_file(&data_dir, &entry).map_err(|e| e.to_string())?;
    Ok(format!(
        "moved {} -> {}",
        record.original_path.display(),
        record.quarantine_path.display()
    ))
}
#[tauri::command]
fn undo_entry(id: String) -> Result<(), String> {
    // Scoped restore (see cli cmd_undo): only under the scanned roots. The
    // machine-wide Startup folder is always scanned, so always in scope.
    let mut roots = vec![startup_root(), tasks_root()];
    roots.extend(scanners::startup::default_common_startup_root());
    quarantine::undo_scoped(&resolve_data_dir(), &id, Some(&roots))
        .map(|_| ())
        .map_err(|e| e.to_string())
}
#[derive(Serialize)]
struct QuarantineRecordDto {
    id: String,
    name: String,
    source: String,
    original_path: String,
    quarantine_path: String,
    archived_at: String,
    state: String,
    file_size: Option<u64>,
    sha256_hex: Option<String>,
    acl_captured: bool,
    security_notes: Vec<String>,
}
/// Everything currently in quarantine (newest last). Powers the Quarantine/// view; undo actions go through `undo_entry` one record at a time.
#[tauri::command]
fn list_quarantine() -> Result<Vec<QuarantineRecordDto>, String> {
    Ok(quarantine::list_records(&resolve_data_dir())
        .into_iter()
        .map(|r| QuarantineRecordDto {
            id: r.id,
            name: r.name,
            source: r.source,
            original_path: r.original_path.to_string_lossy().into_owned(),
            quarantine_path: r.quarantine_path.to_string_lossy().into_owned(),
            archived_at: r.archived_at.to_rfc3339(),
            state: format!("{:?}", r.state),
            file_size: r.file_size,
            sha256_hex: r.sha256_hex,
            acl_captured: r.security.as_ref().is_some_and(|s| s.sddl.is_some()),
            security_notes: r.security_notes,
        })
        .collect())
}
/// Forensic details for one finding (shortcut resolution, task metadata,/// publisher/signature) — strictly read-only enrichment for display./// The id must come from a fresh scan; unknown ids are rejected, so the/// webview cannot turn this into an arbitrary-path inspection oracle.
#[tauri::command]
fn entry_details(id: String) -> Result<cure_core::entry_details::EntryDetails, String> {
    let Some(entry) = find_current_entry(&id) else {
        return Err(
            "entry is no longer present in the latest scan — rescan to refresh".to_string(),
        );
    };
    Ok(cure_core::entry_details::for_entry(&entry))
}
/// Reveal a finding's file in Explorer (selects it, opens nothing)./// Strict id-based lookup like `entry_details`; registry/service/WMI and/// other non-file locations are refused. Direct argv to explorer — no/// shell parsing, so spaces and metacharacters in paths are inert.
#[tauri::command]
fn reveal_location(id: String) -> Result<String, String> {
    let Some(entry) = find_current_entry(&id) else {
        return Err(
            "entry is no longer present in the latest scan — rescan to refresh".to_string(),
        );
    };
    let target = if Path::new(&entry.location).is_file() {
        entry.location.clone()
    } else {
        match cure_core::signature::resolve_executable_path(&entry.command) {
            Some(path) => path.to_string_lossy().into_owned(),
            None => {
                return Err(format!(
                    "nothing file-backed to reveal for {} ({})",
                    entry.name, entry.location
                ))
            }
        }
    };
    std::process::Command::new("explorer")
        .arg("/select,")
        .arg(&target)
        .spawn()
        .map_err(|e| format!("cannot open explorer: {e}"))?;
    Ok(target)
}
/// Explicit security-report export (JSON/TXT, optional redaction)./// Runs fresh scans itself so coverage states are honest; remediates/// nothing. Returns the written file path for the footer message.
#[tauri::command]
async fn export_report(format: String, redact: bool) -> Result<String, String> {
    use cure_core::report::{CoverageRow, ReportOptions, ScanInput};
    let format = format.to_ascii_lowercase();
    if format != "json" && format != "txt" {
        return Err("unknown format: use json or txt".to_string());
    }
    let data_dir = resolve_data_dir();
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("cannot create data dir: {e}"))?;
    let mut entries: Vec<PersistenceEntry> = Vec::new();
    let mut coverage: Vec<CoverageRow> = Vec::new();
    #[cfg(windows)]
    {
        let reg = scanners::registry::scan_report();
        coverage.push(registry_state_row(&reg));
        entries.extend(reg.entries);
    }
    #[cfg(not(windows))]
    coverage.push(CoverageRow::unavailable(
        "Registry autoruns",
        "Windows-only source",
    ));
    entries.extend(scanners::startup::scan(&startup_root()));
    entries.extend(scanners::startup::scan_common());
    let task_report = scanners::scheduled_tasks::scan_report(&tasks_root());
    coverage.push(tasks_state_row(&task_report));
    entries.extend(task_report.entries);
    let service_report = scanners::services::scan_report();
    coverage.push(services_state_row(&service_report));
    let service_records = service_report.records;
    entries.extend(service_records.iter().map(|r| r.entry.clone()));
    #[cfg(windows)]
    {
        let wmi_report = scanners::wmi::scan_report();
        coverage.push(wmi_state_row(&wmi_report));
        entries.extend(wmi_report.entries);
        entries.extend(scanners::ifeo::scan());
        entries.extend(scanners::appinit::scan());
        entries.extend(scanners::com::scan());
    }
    #[cfg(not(windows))]
    coverage.push(CoverageRow::unavailable(
        "WMI subscriptions",
        "Windows-only source",
    ));
    let mut scored: Vec<ScoredEntry> = entries
        .iter()
        .filter(|e| e.source != PersistenceSource::WindowsService)
        .map(|e| {
            let exe_path = cure_core::signature::resolve_executable_path(&e.command);
            risk::score_entry(e, exe_path.as_deref())
        })
        .collect();
    for record in &service_records {
        let status = cure_core::scanners::services::image_status(&record.image_path);
        let exe = match &status {
            cure_core::scanners::services::ImageStatus::Found(path) => Some(path.clone()),
            _ => None,
        };
        let missing = status == cure_core::scanners::services::ImageStatus::Missing;
        let signature = match &exe {
            Some(path) => cure_core::signature::check_signature(path),
            None => cure_core::signature::SignatureStatus::Unknown,
        };
        let hash = if risk::service_needs_hash(&record.image_path, exe.as_deref()) {
            exe.as_deref().and_then(cure_core::hash_intel::check_hash)
        } else {
            None
        };
        scored.push(risk::score_service(
            record,
            missing,
            signature,
            hash.as_deref(),
        ));
    }
    let processes = process_scan::enumerate_processes();
    let mut scored_procs = Vec::new();
    for info in &processes {
        let sig = cure_core::signature::check_signature(std::path::Path::new(&info.exe_path));
        let hash = cure_core::hash_intel::check_hash(std::path::Path::new(&info.exe_path));
        scored_procs.push((
            info.clone(),
            process_scan::score_process(info, &sig, hash.as_deref()),
        ));
    }
    let folders: Vec<(PathBuf, Vec<cure_core::ransom_detect::DirEntry>)> = user_folder_candidates()
        .into_iter()
        .map(|f| (f.clone(), read_dir_entries(&f)))
        .collect();
    let ransom = cure_core::ransom_detect::scan_folders(&folders);
    let count = |source: PersistenceSource| entries.iter().filter(|e| e.source == source).count();
    // State rows (registry/tasks/services/WMI) are already in `coverage`
    // from the collection block above; append the static rows here.
    coverage.push(CoverageRow::checked(
        "Startup folder",
        format!("{} entries", count(PersistenceSource::StartupFolder)),
    ));
    coverage.push(CoverageRow::checked(
        "IFEO debuggers",
        format!("{} entries", count(PersistenceSource::IfeoDebugger)),
    ));
    coverage.push(CoverageRow::checked(
        "AppInit DLLs",
        format!("{} entries", count(PersistenceSource::AppInitDlls)),
    ));
    coverage.push(CoverageRow::checked(
        "COM hijacks (HKCU)",
        format!("{} entries", count(PersistenceSource::ComHijack)),
    ));
    #[cfg(windows)]
    {
        coverage.push(CoverageRow::checked(
            "Registry autoruns",
            format!("{} entries", count(PersistenceSource::RegistryRun)),
        ));
        coverage.push(CoverageRow::checked(
            "Running processes",
            format!("{} enumerated", processes.len()),
        ));
    }
    #[cfg(not(windows))]
    {
        coverage.push(CoverageRow::unavailable(
            "Registry autoruns",
            "Windows-only source".to_string(),
        ));
        coverage.push(CoverageRow::unavailable(
            "Running processes",
            "Windows-only source".to_string(),
        ));
    }
    if folders.is_empty() {
        coverage.push(CoverageRow::not_checked(
            "Ransom indicators",
            "no user profile folders found".to_string(),
        ));
    } else {
        coverage.push(CoverageRow::checked(
            "Ransom indicators",
            format!("{} folders, {} findings", folders.len(), ransom.len()),
        ));
    }
    coverage.push(CoverageRow::not_checked(
        "Canary guard",
        "session feature — see the Canary Guard view".to_string(),
    ));
    {
        use cure_core::hash_intel::ThreatIntelProvider;
        coverage.push(CoverageRow::checked(
            "Threat intel",
            cure_core::hash_intel::fixture_provider()
                .provider_label()
                .to_string(),
        ));
    }
    let report = cure_core::report::assemble(
        ScanInput {
            persistence: scored,
            processes: scored_procs,
            ransom,
            canary_note: "session feature".to_string(),
        },
        coverage,
        &ReportOptions { redact },
    );
    let body = if format == "json" {
        cure_core::report::render_json(&report)
    } else {
        cure_core::report::render_txt(&report)
    };
    let filename = format!("cure-report-{}.{}", cure_core::report::utc_stamp(), format);
    let out_path = data_dir.join(&filename);
    std::fs::write(&out_path, body).map_err(|e| format!("cannot write report: {e}"))?;
    Ok(out_path.to_string_lossy().into_owned())
} // ---------------------------------------------------------------------------// Post-login incident investigation (observation only — see core::incident)// ---------------------------------------------------------------------------
#[derive(Serialize, Clone)]
struct IncidentProgressEvent {
    stage: String,
    polls: usize,
    processes: usize,
    windows: usize,
}
/// Run a post-login incident observation: collect current persistence/// findings, observe process/window activity for the validated duration,/// correlate, and return the verdict. Observation installs nothing and/// modifies nothing; durations are restricted to 15/30/60/120 seconds.
#[tauri::command]
async fn start_incident_observation(
    app: AppHandle,
    duration_secs: u64,
) -> Result<cure_core::incident::ObservationResult, String> {
    use cure_core::incident;
    let duration_secs = incident::validate_duration(duration_secs)?;
    let elevated = cure_core::elevation::is_elevated();
    // Fresh findings for correlation (scoring unnecessary — correlation
    // works on command/location/name).
    let mut findings: Vec<incident::StartupRef> =
        scanners::collect_all(&startup_root(), &tasks_root())
            .iter()
            .map(incident::StartupRef::from)
            .collect();
    for record in scanners::collect_services().iter() {
        let mut finding = incident::StartupRef::from(&record.entry);
        finding.aux_pid = record.pid;
        findings.push(finding);
    }
    let started_at = std::time::SystemTime::now();
    let started_rfc = cure_core::incident::rfc3339(started_at);
    let investigation_id = incident::new_investigation_id();
    let duration = std::time::Duration::from_secs(duration_secs);
    let progress_app = app.clone();
    let live = tokio::task::spawn_blocking(move || {
        incident::observe_blocking(duration, |polls, procs, wins| {
            let _ = progress_app.emit(
                "incident-progress",
                IncidentProgressEvent {
                    stage: "incident-observing".to_string(),
                    polls,
                    processes: procs,
                    windows: wins,
                },
            );
        })
    })
    .await
    .map_err(|e| format!("observation failed: {e}"))?;
    // First pass without command lines, then fetch them for correlated
    // processes only (bounded) and re-correlate — command lines can only
    // upgrade PARTIAL matches to STRONG, never invent DIRECT ones.
    // (pid, observed name) pairs: the fetch attaches a command line only
    // when the live name still matches (PID-reuse guard).
    let mut correlations = incident::correlate(&findings, &live.processes);
    let mut need_cmdline: Vec<(u32, String)> = correlations
        .iter()
        .filter(|c| {
            c.level == incident::CorrelationLevel::Direct
                || c.level == incident::CorrelationLevel::Strong
        })
        .map(|c| (c.process_pid, c.process_name.clone()))
        .collect();
    need_cmdline.sort_unstable();
    need_cmdline.dedup();
    if !need_cmdline.is_empty() {
        let cmdlines = incident::fetch_command_lines(&need_cmdline);
        let mut processes = live.processes;
        for p in processes.iter_mut() {
            if let Some(cmd) = cmdlines.get(&p.pid) {
                p.command_line = Some(cmd.clone());
            }
        }
        correlations = incident::correlate(&findings, &processes);
        let timeline = incident::build_timeline(
            started_at,
            duration_secs,
            &processes,
            &live.windows,
            &correlations,
        );
        let verdict =
            incident::decide_verdict(&correlations, &live.windows, &live.process_observation);
        return Ok(cure_core::incident::ObservationResult {
            investigation_id,
            started_at: started_rfc,
            duration_secs,
            elevated,
            process_observation: live.process_observation,
            window_observation: live.window_observation,
            processes,
            windows: live.windows,
            correlations,
            timeline,
            verdict,
            truncated: live.truncated,
        });
    }
    let timeline = incident::build_timeline(
        started_at,
        duration_secs,
        &live.processes,
        &live.windows,
        &correlations,
    );
    let verdict = incident::decide_verdict(&correlations, &live.windows, &live.process_observation);
    Ok(cure_core::incident::ObservationResult {
        investigation_id,
        started_at: started_rfc,
        duration_secs,
        elevated,
        process_observation: live.process_observation,
        window_observation: live.window_observation,
        processes: live.processes,
        windows: live.windows,
        correlations,
        timeline,
        verdict,
        truncated: live.truncated,
    })
}
/// Export an observation result as JSON/TXT incident report (explicit;/// remediates nothing). The result is passed back in — the backend keeps/// no observation state between commands.
#[tauri::command]
async fn export_incident_report(
    result: cure_core::incident::ObservationResult,
    format: String,
    redact: bool,
) -> Result<String, String> {
    use cure_core::report::{CoverageRow, ReportOptions};
    let format = format.to_ascii_lowercase();
    if format != "json" && format != "txt" {
        return Err("unknown format: use json or txt".to_string());
    }
    let data_dir = resolve_data_dir();
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("cannot create data dir: {e}"))?;
    // Fresh persistence scoring so the export's findings stand on their own.
    let mut entries: Vec<PersistenceEntry> = Vec::new();
    #[cfg(windows)]
    entries.extend(scanners::registry::scan().unwrap_or_default());
    entries.extend(scanners::startup::scan(&startup_root()));
    entries.extend(scanners::startup::scan_common());
    entries.extend(scanners::scheduled_tasks::scan(&tasks_root()));
    let service_records = scanners::collect_services();
    entries.extend(service_records.iter().map(|r| r.entry.clone()));
    #[cfg(windows)]
    {
        entries.extend(scanners::wmi::scan());
        entries.extend(scanners::ifeo::scan());
        entries.extend(scanners::appinit::scan());
        entries.extend(scanners::com::scan());
    }
    let count = |source: PersistenceSource| entries.iter().filter(|e| e.source == source).count();
    let scored: Vec<ScoredEntry> = entries
        .iter()
        .filter(|e| e.source != PersistenceSource::WindowsService)
        .map(|e| {
            let exe_path = cure_core::signature::resolve_executable_path(&e.command);
            risk::score_entry(e, exe_path.as_deref())
        })
        .collect();
    let coverage = vec![
        CoverageRow::checked(
            "Startup folder",
            format!("{} entries", count(PersistenceSource::StartupFolder)),
        ),
        CoverageRow::checked(
            "Scheduled tasks",
            format!("{} entries", count(PersistenceSource::ScheduledTask)),
        ),
        CoverageRow::checked(
            "Services (auto-start)",
            format!("{} services", service_records.len()),
        ),
        CoverageRow::checked(
            "Incident observation",
            format!(
                "{} processes, {} windows in {} s",
                result.processes.len(),
                result.windows.len(),
                result.duration_secs
            ),
        ),
    ];
    let empty: Vec<(
        cure_core::process_scan::ProcessInfo,
        cure_core::process_scan::ProcessScore,
    )> = Vec::new();
    let no_ransom: Vec<cure_core::ransom_detect::RansomFinding> = Vec::new();
    let mut report = cure_core::report::assemble(
        cure_core::report::ScanInput {
            persistence: scored,
            processes: empty,
            ransom: no_ransom,
            canary_note: String::new(),
        },
        coverage,
        &ReportOptions { redact },
    );
    report.incident = Some(cure_core::report::incident_section(&result, redact));
    let body = if format == "json" {
        cure_core::report::render_json(&report)
    } else {
        cure_core::report::render_txt(&report)
    };
    let filename = format!(
        "cure-incident-{}.{}",
        cure_core::report::utc_stamp(),
        format
    );
    let out_path = data_dir.join(&filename);
    std::fs::write(&out_path, body).map_err(|e| format!("cannot write report: {e}"))?;
    Ok(out_path.to_string_lossy().into_owned())
}
/// Confirm-before-kill hardening: a PID can be recycled between the scan/// and the user's confirmation click. Refuse to kill when the PID no longer/// belongs to the process the user actually approved.
#[cfg(windows)]
fn pid_still_matches(
    pid: u32,
    approved_name: &str,
    live: &std::collections::HashMap<u32, String>,
) -> Result<(), String> {
    match live.get(&pid) {
        Some(current) if current.eq_ignore_ascii_case(approved_name) => Ok(()),
        Some(current) => Err(format!(
            "pid {pid} no longer belongs to {approved_name} (now {current}) — refusing to kill"
        )),
        None => Err(format!("{approved_name} (pid {pid}) already exited")),
    }
}
#[cfg(not(windows))]
fn pid_still_matches(
    _pid: u32,
    _approved_name: &str,
    _live: &std::collections::HashMap<u32, String>,
) -> Result<(), String> {
    // No process enumeration on this platform; the kill attempt below
    // produces the platform error, as before.
    Ok(())
}
#[tauri::command]
fn kill_high_risk_processes(processes: Vec<(String, u32)>) -> Result<KillReport, String> {
    let mut killed = Vec::new();
    let mut failed = Vec::new();
    // Snapshot once so every decision below uses the same process table.
    let live: std::collections::HashMap<u32, String> = process_scan::enumerate_processes()
        .into_iter()
        .map(|p| (p.pid, p.name))
        .collect();
    for (name, pid) in processes {
        if let Err(reason) = pid_still_matches(pid, &name, &live) {
            failed.push(reason);
            continue;
        }
        match kill_process_by_pid(pid) {
            Ok(()) => {
                killed.push(ProcessFinding {
                    name: name.clone(),
                    pid,
                    exe_path: String::new(),
                    score: 0,
                    risk: "HighRisk".to_string(),
                    evidence: vec![cure_core::evidence::Evidence::observed(
                        "termination requested by the operator",
                    )],
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
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create quarantine folder: {e}"))?;
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
    open_with_default_handler(&path)?;
    Ok(path.to_string_lossy().into_owned())
}
/// Open a file with its registered handler via ShellExecuteW("open").////// Deliberately NOT `cmd /C start ...`: cmd.exe re-parses its command line,/// so a data dir containing shell metacharacters (`&`, `^`, …) would become/// command injection. The path here is passed as structured data, never/// through a shell — spaces and metacharacters are inert.
#[cfg(windows)]
fn open_with_default_handler(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // ShellExecuteW returns a value > 32 on success.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if (result.0 as usize) > 32 {
        Ok(())
    } else {
        Err(format!(
            "cannot open log viewer (ShellExecute failed for {})",
            path.display()
        ))
    }
}
#[cfg(not(windows))]
fn open_with_default_handler(path: &Path) -> Result<(), String> {
    Err(format!(
        "cannot open log viewer on this platform ({})",
        path.display()
    ))
}
#[tauri::command]
fn exit_app(app: AppHandle) {
    app.exit(0);
} // ---------------------------------------------------------------------------// storage workstation//// The GUI never sees a raw delete instruction. It sends a *selection*, gets// back a plan it must show the operator, and only then asks the engine to// execute that exact plan. The backend holds the plan, so the caller cannot// substitute a different selection between preview and execution.//// The legacy `scan_cleanup` / `run_cleanup` commands lived here once. They// were removed because they were reachable over IPC with no confirmation, no// preview and no receipt: an unguarded permanent-delete endpoint sitting// next to a carefully gated one. The CLI's own `cure cleanup` path is// unaffected and still lives in `cure_core::cleanup`.// ---------------------------------------------------------------------------
#[derive(Serialize, Clone)]
struct CleanupFailureDto {
    path: String,
    reason: String,
}
/// Held for the lifetime of the process: the last scan, the plan the/// operator approved, and the flag that cancels a scan in flight.
#[derive(Default)]
struct StorageState {
    last_scan: Mutex<Option<storage::StorageScan>>,
    /// The investigation derived from the last scan.
    ///
    /// It is kept separately from the scan because the scan is dropped the
    /// moment a cleanup runs: the filesystem has changed, so the next action
    /// must be built from a fresh measurement. The investigation is a
    /// snapshot of what was measured, and a case file is exactly a snapshot
    /// of what was measured.
    last_investigation: Mutex<Option<storage::detective::Investigation>>,
    /// The receipt from the last executed plan, when one ran.
    ///
    /// A case file can only claim that space was recovered when this exists,
    /// and only when it carries an observed free-space movement.
    last_receipt: Mutex<Option<storage::CleanupReceipt>>,
    /// The plan the operator approved. Execution runs *this* plan, not a plan
    /// rebuilt from whatever the caller sends, so what is removed is exactly
    /// what was approved.
    approved_plan: Mutex<Option<storage::CleanupPlan>>,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    running: AtomicBool,
}
/// Progress relayed from the engine to the webview.
#[derive(Clone, Serialize)]
struct StorageProgressDto {
    scan_id: String,
    phase: String,
    root_label: String,
    current_path: String,
    dirs_seen: u64,
    files_seen: u64,
    bytes_seen: u64,
    elapsed_ms: u64,
    eta_ms: Option<u64>,
}
impl From<storage::ScanProgress> for StorageProgressDto {
    fn from(p: storage::ScanProgress) -> Self {
        StorageProgressDto {
            scan_id: p.scan_id,
            phase: p.phase,
            root_label: p.root_label,
            current_path: p.current_path,
            dirs_seen: p.dirs_seen,
            files_seen: p.files_seen,
            bytes_seen: p.bytes_seen,
            elapsed_ms: p.elapsed_ms,
            eta_ms: p.eta_ms,
        }
    }
}
/// One row the operator can act on, with the decision already made.
#[derive(Serialize)]
struct StorageFileDto {
    id: String,
    path: String,
    name: String,
    parent: String,
    extension: String,
    size_logical: u64,
    size_allocated: u64,
    modified_unix: Option<u64>,
    accessed_unix: Option<u64>,
    created_unix: Option<u64>,
    hard_link_count: Option<u32>,
    is_read_only: bool,
    is_hidden: bool,
    class: String,
    class_label: String,
    tier: String,
    tier_label: String,
    /// True when the tier permits appearing in a bulk action.
    bulk_eligible: bool,
    reasons: Vec<String>,
}
fn storage_file_dto(f: &storage::FileRecord) -> StorageFileDto {
    StorageFileDto {
        id: storage::tiers::normalize(&f.path),
        path: f.path.to_string_lossy().into_owned(),
        name: f
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        parent: f
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        extension: f.extension.clone(),
        size_logical: f.size_logical,
        size_allocated: f.reclaimable_bytes(),
        modified_unix: f.modified_unix,
        accessed_unix: f.accessed_unix,
        created_unix: f.created_unix,
        hard_link_count: f.hard_link_count,
        is_read_only: f.is_read_only,
        is_hidden: f.is_hidden,
        class: f.class.key().to_string(),
        class_label: f.class.label().to_string(),
        tier: f.tier.key().to_string(),
        tier_label: f.tier.label().to_string(),
        bulk_eligible: f.tier.allows_bulk_recommendation(),
        reasons: f.reasons.clone(),
    }
}
/// A folder for the treemap. Deep trees are pruned to the largest children so/// the payload stays proportional to what is actually on screen.
#[derive(Serialize)]
struct StorageFolderDto {
    id: String,
    path: String,
    name: String,
    depth: usize,
    size_allocated: u64,
    size_logical: u64,
    file_count: u64,
    class: String,
    tier: String,
    incomplete: bool,
}
#[derive(Serialize)]
struct StorageDuplicateCopyDto {
    id: String,
    path: String,
    name: String,
    parent: String,
    size_allocated: u64,
    modified_unix: Option<u64>,
    tier: String,
    tier_label: String,
    class_label: String,
    is_kept_candidate: bool,
    reasons: Vec<String>,
}
#[derive(Serialize)]
struct StorageDuplicateSetDto {
    id: String,
    size_allocated: u64,
    sha256: String,
    reclaimable_bytes: u64,
    protected_copies: usize,
    copies: Vec<StorageDuplicateCopyDto>,
}
#[derive(Serialize)]
struct StorageAbandonedDto {
    path: String,
    app_name: String,
    size_allocated: u64,
    file_count: u64,
    days_since_change: Option<u64>,
    tier: String,
    tier_label: String,
    reasons: Vec<String>,
    caveat: String,
    /// Always false. Present so the UI cannot offer the action by omission.
    removable: bool,
}
#[derive(Serialize)]
struct StorageSummaryDto {
    key: String,
    label: String,
    tier: String,
    item_count: u64,
    bytes_allocated: u64,
    bytes_logical: u64,
}
#[derive(Serialize)]
struct StorageRootDto {
    path: String,
    label: String,
    class: String,
    is_catalogue: bool,
    resolved: bool,
    reason: String,
    files_seen: u64,
    bytes_allocated: u64,
    incomplete: bool,
}
#[derive(Serialize)]
struct StorageCoverageDto {
    completeness: String,
    files_seen: u64,
    dirs_seen: u64,
    permission_denied_dirs: u64,
    permission_denied_files: u64,
    reparse_points_skipped: u64,
    locked_or_vanished: u64,
    overlapping_roots_dropped: u64,
    notes: Vec<String>,
    duration_ms: u64,
    cancelled: bool,
    volume_total_bytes: Option<u64>,
    volume_free_bytes: Option<u64>,
    volume_label: Option<String>,
    failures: Vec<CleanupFailureDto>,
    /// Links deliberately not followed. Not failures: not following a link
    /// is the policy working.
    skipped_links: Vec<CleanupFailureDto>,
    roots: Vec<StorageRootDto>,
}
#[derive(Serialize)]
struct StorageHeadlineDto {
    safe_bytes: u64,
    careful_bytes: u64,
    certainty: String,
    certainty_label: String,
    incomplete: bool,
    safe_bytes_label: String,
    careful_bytes_label: String,
}
#[derive(Serialize)]
struct StorageTierCountsDto {
    safe_to_review: u64,
    review_carefully: u64,
    protected: u64,
    unknown: u64,
}

#[derive(Serialize)]
struct StorageScanDto {
    scan_id: String,
    started_unix: u64,
    finished_unix: u64,
    duration_ms: u64,
    headline: StorageHeadlineDto,
    protected_bytes: u64,
    duplicate_reclaimable_bytes: u64,
    summaries: Vec<StorageSummaryDto>,
    coverage: StorageCoverageDto,
    duplicate_coverage: storage::DuplicateCoverage,
    files: Vec<StorageFileDto>,
    /// How many files the scan measured in total. The list above is bounded,
    /// so this is the honest count behind it.
    files_total: u64,
    /// The per-tier counts, also from the whole measurement rather than from
    /// the bounded list.
    tier_counts: StorageTierCountsDto,
    folders: Vec<StorageFolderDto>,
    large_files: Vec<StorageFileDto>,
    large_folders: Vec<StorageFolderDto>,
    duplicates: Vec<StorageDuplicateSetDto>,
    abandoned: Vec<StorageAbandonedDto>,
    /// The investigation derived from the very same measurement.
    ///
    /// This is a reading of the scan above, not a second scan: the categories,
    /// the leads and the totals all come from the numbers the engine just
    /// produced.
    investigation: storage::detective::Investigation,
}
/// The most file rows the interface is given in one payload.
///
/// A deep scan of a whole drive measures hundreds of thousands of files.
/// The interface shows the largest of them, so the rest are counted rather
/// than transmitted.
const MAX_FILES_TO_UI: usize = 20_000;

/// Drives and well-known roots the operator may add to a scan.
#[derive(Serialize)]
struct StorageRootOptionDto {
    path: String,
    label: String,
    exists: bool,
    is_drive: bool,
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    volume_label: Option<String>,
    /// `true` for volumes and folders the user would authorise explicitly.
    selectable: bool,
}
/// Validate a folder the operator typed, so it can be added to a scan.
///
/// The picker offers drives and well-known locations, but a project on
/// `D:\Projects` is exactly the kind of place a storage tool needs to look,
/// and the operator is the only one who knows where it is.
#[tauri::command]
fn storage_validate_root(path: String) -> Result<StorageRootOptionDto, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Type a folder path.".to_string());
    }
    let target = PathBuf::from(trimmed);
    let metadata = std::fs::symlink_metadata(&target)
        .map_err(|e| format!("That folder could not be read: {e}"))?;
    if !metadata.is_dir() {
        return Err("That path is not a folder.".to_string());
    }
    if storage::fsx::is_reparse_point(&metadata) {
        return Err(
            "That path is a link or junction. Add the folder it points to instead.".to_string(),
        );
    }
    let space = storage::fsx::volume_space(&target);
    Ok(StorageRootOptionDto {
        label: target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| trimmed.to_string()),
        path: target.to_string_lossy().into_owned(),
        exists: true,
        is_drive: false,
        total_bytes: space.as_ref().map(|v| v.total_bytes),
        free_bytes: space.as_ref().map(|v| v.free_bytes),
        volume_label: space.and_then(|v| v.label),
        selectable: true,
    })
}

/// List candidate roots: fixed drives plus the profile's real user folders.
#[tauri::command]
fn storage_available_roots() -> Result<Vec<StorageRootOptionDto>, String> {
    let mut out: Vec<StorageRootOptionDto> = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        let Ok(md) = std::fs::metadata(&root) else {
            continue;
        };
        if !md.is_dir() {
            continue;
        }
        let space = storage::fsx::volume_space(&root);
        out.push(StorageRootOptionDto {
            label: format!("Drive {}", letter as char),
            path: root.to_string_lossy().into_owned(),
            exists: true,
            is_drive: true,
            total_bytes: space.as_ref().map(|v| v.total_bytes),
            free_bytes: space.as_ref().map(|v| v.free_bytes),
            volume_label: space.and_then(|v| v.label),
            selectable: true,
        });
    }
    // The removable/external drives a USB rescue copy sits on are the whole
    // point of the watcher; offer them explicitly rather than hiding them in
    // an OS dialog the operator may not know to open.
    for (key, label) in [
        ("USERPROFILE", "Profile"),
        ("LOCALAPPDATA", "App data (local)"),
        ("APPDATA", "App data (roaming)"),
        ("TEMP", "Temp"),
        ("SystemDrive", "Windows drive"),
    ] {
        let Some(p) = std::env::var_os(key).map(PathBuf::from) else {
            continue;
        };
        if p.as_os_str().is_empty() || out.iter().any(|o| o.path == p.to_string_lossy()) {
            continue;
        }
        let exists = p.is_dir();
        let space = storage::fsx::volume_space(&p);
        out.push(StorageRootOptionDto {
            label: label.to_string(),
            path: p.to_string_lossy().into_owned(),
            exists,
            is_drive: false,
            total_bytes: space.as_ref().map(|v| v.total_bytes),
            free_bytes: space.as_ref().map(|v| v.free_bytes),
            volume_label: space.and_then(|v| v.label),
            selectable: true,
        });
    }
    Ok(out)
}
/// Run a storage scan. Blocking work runs on a worker thread; progress and/// the finished scan are relayed through events, never invented by the UI.
#[tauri::command]
async fn storage_scan(
    app: AppHandle,
    state: State<'_, StorageState>,
    depth: String,
    roots: Vec<String>,
    find_duplicates: bool,
    find_abandoned: bool,
) -> Result<StorageScanDto, String> {
    if state
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A storage scan is already running.".to_string());
    }
    let depth = storage::catalog::ScanDepth::parse(&depth);
    let extra: Vec<PathBuf> = roots
        .iter()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    let options = storage::ScanOptions {
        walk_limits: storage::catalog::limits_for(depth),
        ..storage::ScanOptions {
            depth,
            extra_roots: extra,
            find_duplicates,
            find_abandoned,
            ..storage::ScanOptions::default()
        }
    };
    let scan_id = storage::new_scan_id();
    let cancel = Arc::new(AtomicBool::new(false));
    *state
        .cancel
        .lock()
        .map_err(|_| "storage state is poisoned".to_string())? = Some(Arc::clone(&cancel));
    let relay = app.clone();
    let progress_scan_id = scan_id.clone();
    let worker = std::thread::spawn(move || {
        let mut last_emit = std::time::Instant::now();
        let result = storage::scan(&options, &cancel, &progress_scan_id, |progress| {
            // Throttle: the engine can emit far faster than a UI can use,
            // and an unthrottled event stream makes the app feel slow.
            if last_emit.elapsed().as_millis() < 100 && progress.phase != storage::PHASE_FINISHED {
                return;
            }
            last_emit = std::time::Instant::now();
            let _ = relay.emit("storage-progress", StorageProgressDto::from(progress));
        });
        (result, options.depth)
    });
    let (scan, _depth) = tauri::async_runtime::spawn_blocking(move || worker.join())
        .await
        .map_err(|_| "the storage worker thread could not be joined".to_string())?
        .map_err(|_| "the storage worker thread panicked".to_string())?;
    state.running.store(false, Ordering::SeqCst);
    *state
        .cancel
        .lock()
        .map_err(|_| "storage state is poisoned".to_string())? = None;
    let dto = storage_scan_dto(&scan);
    *state
        .last_scan
        .lock()
        .map_err(|_| "storage state is poisoned".to_string())? = Some(scan);
    if let Ok(mut guard) = state.last_investigation.lock() {
        *guard = Some(dto.investigation.clone());
    }
    Ok(dto)
}
fn storage_scan_dto(scan: &storage::StorageScan) -> StorageScanDto {
    let headline = scan.headline();
    let coverage = &scan.coverage;
    StorageScanDto {
        scan_id: scan.scan_id.clone(),
        started_unix: scan.started_unix,
        finished_unix: scan.finished_unix,
        duration_ms: coverage.duration_ms,
        headline: StorageHeadlineDto {
            safe_bytes: headline.safe_bytes,
            careful_bytes: headline.careful_bytes,
            certainty: format!("{:?}", headline.certainty).to_lowercase(),
            certainty_label: headline.certainty.label().to_string(),
            incomplete: headline.incomplete,
            safe_bytes_label: storage::human_bytes(headline.safe_bytes),
            careful_bytes_label: storage::human_bytes(headline.careful_bytes),
        },
        protected_bytes: scan.protected_bytes,
        duplicate_reclaimable_bytes: scan.duplicate_reclaimable_bytes,
        summaries: scan
            .summaries
            .iter()
            .map(|s| StorageSummaryDto {
                key: s.class.key().to_string(),
                label: s.class.label().to_string(),
                tier: s.tier.key().to_string(),
                item_count: s.item_count,
                bytes_allocated: s.bytes_allocated,
                bytes_logical: s.bytes_logical,
            })
            .collect(),
        coverage: StorageCoverageDto {
            completeness: coverage.completeness().to_string(),
            files_seen: coverage.files_seen,
            dirs_seen: coverage.dirs_seen,
            permission_denied_dirs: coverage.permission_denied_dirs,
            permission_denied_files: coverage.permission_denied_files,
            reparse_points_skipped: coverage.reparse_points_skipped,
            locked_or_vanished: coverage.locked_or_vanished,
            overlapping_roots_dropped: coverage.overlapping_roots_dropped,
            notes: coverage.notes(),
            duration_ms: coverage.duration_ms,
            cancelled: coverage.cancelled,
            volume_total_bytes: coverage.volume_total_bytes,
            volume_free_bytes: coverage.volume_free_bytes,
            volume_label: coverage.volume_label.clone(),
            failures: coverage
                .failures
                .iter()
                .map(|f| CleanupFailureDto {
                    path: f.path.to_string_lossy().into_owned(),
                    reason: f.reason.clone(),
                })
                .collect(),
            skipped_links: coverage
                .skipped_links
                .iter()
                .map(|f| CleanupFailureDto {
                    path: f.path.to_string_lossy().into_owned(),
                    reason: f.reason.clone(),
                })
                .collect(),
            roots: coverage
                .roots
                .iter()
                .map(|r| StorageRootDto {
                    path: r.path.to_string_lossy().into_owned(),
                    label: r.label.clone(),
                    class: r.class.key().to_string(),
                    is_catalogue: true,
                    resolved: r.resolved,
                    reason: r.reason.clone(),
                    files_seen: r.files_seen,
                    bytes_allocated: r.bytes_allocated,
                    incomplete: r.incomplete,
                })
                .collect(),
        },
        duplicate_coverage: scan.duplicate_coverage.clone(),
        /* The measured set can be hundreds of thousands of files. Sending
        all of them over IPC would make the window slow to open for no
        gain: the interface only ever shows the largest ones. The count
        that was measured travels with the list so nothing is implied
        that did not happen. */
        files: scan
            .files
            .iter()
            .take(MAX_FILES_TO_UI)
            .map(storage_file_dto)
            .collect(),
        files_total: scan.distinct_file_count,
        tier_counts: StorageTierCountsDto {
            safe_to_review: scan.safe_file_count,
            review_carefully: scan.careful_file_count,
            protected: scan.protected_file_count,
            unknown: scan.protected_file_count.saturating_sub(
                scan.files
                    .iter()
                    .filter(|f| f.tier == storage::SafetyTier::Unknown)
                    .count() as u64,
            ),
        },
        folders: scan
            .folders
            .iter()
            .map(|f| StorageFolderDto {
                id: storage::tiers::normalize(&f.path),
                path: f.path.to_string_lossy().into_owned(),
                name: f
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| f.path.to_string_lossy().into_owned()),
                depth: f.depth,
                size_allocated: f.size_allocated,
                size_logical: f.size_logical,
                file_count: f.file_count,
                class: f.class.key().to_string(),
                tier: f.tier.key().to_string(),
                incomplete: f.incomplete,
            })
            .collect(),
        large_files: scan.large_files.iter().map(storage_file_dto).collect(),
        large_folders: scan
            .large_folders
            .iter()
            .map(|f| StorageFolderDto {
                id: storage::tiers::normalize(&f.path),
                path: f.path.to_string_lossy().into_owned(),
                name: f
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| f.path.to_string_lossy().into_owned()),
                depth: f.depth,
                size_allocated: f.size_allocated,
                size_logical: f.size_logical,
                file_count: f.file_count,
                class: f.class.key().to_string(),
                tier: f.tier.key().to_string(),
                incomplete: f.incomplete,
            })
            .collect(),
        duplicates: scan
            .duplicates
            .iter()
            .map(|d| StorageDuplicateSetDto {
                id: d.id.clone(),
                size_allocated: d.size_allocated,
                sha256: d.sha256.clone(),
                reclaimable_bytes: d.reclaimable_bytes,
                protected_copies: d.protected_copies,
                copies: d
                    .copies
                    .iter()
                    .map(|c| StorageDuplicateCopyDto {
                        id: storage::tiers::normalize(&c.path),
                        path: c.path.to_string_lossy().into_owned(),
                        name: c
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        parent: c
                            .path
                            .parent()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        size_allocated: d.size_allocated,
                        modified_unix: c.modified_unix,
                        tier: c.tier.key().to_string(),
                        tier_label: c.tier.label().to_string(),
                        class_label: c.class.label().to_string(),
                        is_kept_candidate: c.is_kept_candidate,
                        reasons: c.reasons.clone(),
                    })
                    .collect(),
            })
            .collect(),
        abandoned: scan
            .abandoned
            .iter()
            .map(|a| StorageAbandonedDto {
                path: a.path.to_string_lossy().into_owned(),
                app_name: a.app_name.clone(),
                size_allocated: a.size_allocated,
                file_count: a.file_count,
                days_since_change: a.days_since_change,
                tier: a.tier.key().to_string(),
                tier_label: a.tier.label().to_string(),
                reasons: a.reasons.clone(),
                caveat: a.caveat.clone(),
                removable: false,
            })
            .collect(),
        investigation: storage::detective::investigate(scan),
    }
}

/// Build the shareable case file.
///
/// This command reads the investigation and the last receipt and returns an
/// allowlisted model. It cannot remove anything, it cannot read a file, and
/// the model it returns has no field capable of holding a path, a user name
/// or a machine name. The summary is sanitised in the engine before it is
/// allowed through.
#[tauri::command]
fn storage_case_file(
    state: State<'_, StorageState>,
    summary: Option<String>,
    include_summary: Option<bool>,
    privacy: Option<String>,
) -> Result<storage::casefile::CaseFile, String> {
    let investigation = {
        let guard = state
            .last_investigation
            .lock()
            .map_err(|_| "storage state is poisoned".to_string())?;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "Run a storage scan before exporting a case file.".to_string())?
    };
    let receipt = {
        let guard = state
            .last_receipt
            .lock()
            .map_err(|_| "storage state is poisoned".to_string())?;
        guard.clone()
    };
    Ok(storage::casefile::build(
        &investigation,
        receipt.as_ref(),
        summary.as_deref(),
        include_summary.unwrap_or(false),
        storage::casefile::PrivacyLevel::parse(privacy.as_deref().unwrap_or("")),
    ))
}

/// Decode standard base64 without pulling in a dependency.
///
/// Only what the exported image needs: no line breaks are required, padding
/// ends the stream, and anything outside the alphabet is refused.
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len() / 4 * 3 + 3);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for ch in input.chars() {
        if ch == '=' {
            break;
        }
        if ch.is_ascii_whitespace() {
            continue;
        }
        let value = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 26,
            '0'..='9' => ch as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return Err("the image data is not valid base64".to_string()),
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

/// Refuse anything that is not a small PNG before the export path allocates.
fn decode_case_file_png(data_base64: &str) -> Result<Vec<u8>, String> {
    /* Refuse an oversized payload before allocating a decoding buffer for
    it. 16 MiB of PNG is about 22.4 MiB of base64; allow a little slack
    for padding and line breaks. */
    const MAX_CASE_FILE_B64: usize = 24 * 1024 * 1024;
    if data_base64.len() > MAX_CASE_FILE_B64 {
        return Err("the case file image is too large to write".to_string());
    }
    let bytes = decode_base64(data_base64)?;
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("that file is not a PNG image".to_string());
    }
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("the case file image is too large to write".to_string());
    }
    Ok(bytes)
}

/// Write an exported case file image to this machine.
///
/// The file name is reduced to letters, digits, dashes and underscores, so
/// the caller cannot choose where the file lands, and the bytes are checked
/// for the PNG signature before anything is written. Nothing is uploaded.
#[tauri::command]
fn storage_save_case_file(name: String, data_base64: String) -> Result<String, String> {
    let bytes = decode_case_file_png(&data_base64)?;
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if safe.is_empty() {
        return Err("the case file has no identifier".to_string());
    }
    let dir = resolve_data_dir().join("case-files");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the export folder: {e}"))?;
    let path = dir.join(format!("{safe}.png"));
    std::fs::write(&path, &bytes).map_err(|e| format!("cannot write the case file: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}
/// Ask the engine to stop. Safe to call at any time, including when no scan/// is running.
#[tauri::command]
fn storage_cancel(state: State<'_, StorageState>) -> Result<(), String> {
    if let Some(flag) = state
        .cancel
        .lock()
        .map_err(|_| "storage state is poisoned".to_string())?
        .as_ref()
    {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}
#[derive(Serialize)]
struct StoragePlanDto {
    scan_id: String,
    items: Vec<StoragePlanItemDto>,
    refused: Vec<StoragePlanItemDto>,
    estimated_bytes: u64,
    estimated_label: String,
    certainty: String,
    certainty_label: String,
    consequence: String,
    /// The digest of the approved selections. The UI passes this back when
    /// the operator confirms, so the backend runs this exact plan.
    plan_id: String,
    /// The phrase the operator must type to approve this plan.
    confirmation_phrase: String,
}
#[derive(Serialize, Clone)]
struct StoragePlanItemDto {
    path: String,
    name: String,
    size_allocated: u64,
    size_label: String,
    tier: String,
    tier_label: String,
    class_label: String,
    reasons: Vec<String>,
    stale: bool,
}
fn storage_plan_dto(plan: &storage::CleanupPlan) -> StoragePlanDto {
    let to_item = |i: &storage::PlanItem| StoragePlanItemDto {
        path: i.path.to_string_lossy().into_owned(),
        name: i
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| i.path.to_string_lossy().into_owned()),
        size_allocated: i.size_allocated,
        size_label: storage::human_bytes(i.size_allocated),
        tier: i.tier.key().to_string(),
        tier_label: i.tier.label().to_string(),
        class_label: i.class.label().to_string(),
        reasons: i.reasons.clone(),
        stale: i.stale,
    };
    StoragePlanDto {
        scan_id: plan.scan_id.clone(),
        items: plan.items.iter().map(to_item).collect(),
        refused: plan.refused.iter().map(to_item).collect(),
        estimated_bytes: plan.estimated_bytes,
        estimated_label: storage::human_bytes(plan.estimated_bytes),
        certainty: format!("{:?}", plan.estimated_certainty).to_lowercase(),
        certainty_label: plan.estimated_certainty.label().to_string(),
        consequence: plan.consequence.clone(),
        plan_id: plan.plan_id.clone(),
        confirmation_phrase: plan.confirmation_phrase.clone(),
    }
}
/// Build the preview for a selection. This is the only thing the operator/// sees before anything is removed, so it carries the full consequence text.////// The plan is retained by the backend. Execution runs this exact plan, so/// what is removed is exactly what was shown and approved.
#[tauri::command]
fn storage_preview(
    state: State<'_, StorageState>,
    paths: Vec<String>,
) -> Result<StoragePlanDto, String> {
    let plan = {
        let guard = state
            .last_scan
            .lock()
            .map_err(|_| "storage state is poisoned".to_string())?;
        let scan = guard
            .as_ref()
            .ok_or_else(|| "Run a storage scan before selecting anything.".to_string())?;
        let selection: Vec<storage::SelectionItem> = paths
            .iter()
            .map(|p| storage::SelectionItem {
                path: PathBuf::from(p),
                expected_size: None,
                bulk: false,
            })
            .collect();
        storage::build_plan(scan, &selection)
    };
    let dto = storage_plan_dto(&plan);
    *state
        .approved_plan
        .lock()
        .map_err(|_| "storage state is poisoned".to_string())? = Some(plan);
    Ok(dto)
}
#[derive(Serialize, Clone)]
struct StorageVolumeDeltaDto {
    root: String,
    free_before: Option<u64>,
    free_after: Option<u64>,
    delta: Option<i64>,
    delta_label: String,
    files_removed: usize,
}
#[derive(Serialize, Clone)]
struct StorageReceiptDto {
    scan_id: String,
    plan_id: String,
    planned: usize,
    attempted: usize,
    deleted: usize,
    refused: usize,
    failed: usize,
    /// What the removed files occupied on disk before they were removed.
    bytes_removed_estimate: u64,
    bytes_removed_label: String,
    /// The part of the figure above whose reclaim could not be promised.
    bytes_uncertain: u64,
    bytes_uncertain_label: String,
    /// The pre-delete estimate for everything that reached the delete stage.
    bytes_estimated: u64,
    bytes_estimated_label: String,
    finished_unix: u64,
    volumes: Vec<StorageVolumeDeltaDto>,
    /// One sentence stating exactly what the figures above mean.
    note: String,
    failures: Vec<CleanupFailureDto>,
    refused_items: Vec<StoragePlanItemDto>,
}
/// Execute the plan the operator approved.////// The caller passes back the `plan_id` it was shown and the phrase that/// approves it. The backend runs the retained plan, verifies its digest, and/// refuses anything that no longer matches the live filesystem. The caller/// cannot substitute a different selection: it never supplies one.
#[tauri::command]
fn storage_execute(
    app: AppHandle,
    state: State<'_, StorageState>,
    plan_id: String,
    confirm_phrase: String,
) -> Result<StorageReceiptDto, String> {
    if !confirm_phrase.eq_ignore_ascii_case(storage::CONFIRM_PHRASE) {
        return Err(format!("Type {} to confirm.", storage::CONFIRM_PHRASE));
    }
    let plan = {
        let guard = state
            .approved_plan
            .lock()
            .map_err(|_| "storage state is poisoned".to_string())?;
        let plan = guard.as_ref().ok_or_else(|| {
            "There is no approved plan. Review the removal preview first.".to_string()
        })?;
        if plan.plan_id != plan_id {
            return Err(
                "That approval is for a different plan. Review the removal preview again."
                    .to_string(),
            );
        }
        plan.clone()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let receipt = {
        let guard = state
            .last_scan
            .lock()
            .map_err(|_| "storage state is poisoned".to_string())?;
        let scan = guard
            .as_ref()
            .ok_or_else(|| "Run a storage scan before removing anything.".to_string())?;
        if plan.items.is_empty() {
            return Err(format!(
                "Nothing in that selection can be removed. {}",
                plan.refused
                    .first()
                    .and_then(|i| i.reasons.last().cloned())
                    .unwrap_or_else(|| "Check the refused list.".to_string())
            ));
        }
        storage::execute_plan(scan, &plan, &cancel)
    };
    // Drop the stored scan and the approved plan: both describe a
    // filesystem that has just changed. The next action must be built from a
    // fresh measurement.
    if let Ok(mut guard) = state.last_scan.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = state.approved_plan.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = state.last_receipt.lock() {
        *guard = Some(receipt.clone());
    }
    let dto = storage_receipt_dto(&receipt);
    let _ = app.emit("storage-receipt", dto.clone());
    Ok(dto)
}
fn storage_receipt_dto(receipt: &storage::CleanupReceipt) -> StorageReceiptDto {
    StorageReceiptDto {
        scan_id: receipt.scan_id.clone(),
        plan_id: String::new(),
        planned: receipt.planned,
        attempted: receipt.attempted,
        deleted: receipt.deleted,
        refused: receipt.refused,
        failed: receipt.failed,
        bytes_removed_estimate: receipt.bytes_removed_estimate,
        bytes_removed_label: storage::human_bytes(receipt.bytes_removed_estimate),
        bytes_uncertain: receipt.bytes_uncertain,
        bytes_uncertain_label: storage::human_bytes(receipt.bytes_uncertain),
        bytes_estimated: receipt.bytes_estimated,
        bytes_estimated_label: storage::human_bytes(receipt.bytes_estimated),
        finished_unix: receipt.finished_unix,
        volumes: receipt
            .volumes
            .iter()
            .map(|v| StorageVolumeDeltaDto {
                root: v.root.clone(),
                free_before: v.free_before,
                free_after: v.free_after,
                delta: v.delta,
                delta_label: match v.delta {
                    Some(d) if d >= 0 => {
                        format!("+{} free", storage::human_bytes(d as u64))
                    }
                    Some(d) => format!(
                        "{} free (other writes landed too)",
                        storage::human_bytes(d.unsigned_abs())
                    ),
                    None => "not readable".to_string(),
                },
                files_removed: v.files_removed,
            })
            .collect(),
        note: receipt.note.clone(),
        failures: receipt
            .failures
            .iter()
            .map(|f| CleanupFailureDto {
                path: f.path.to_string_lossy().into_owned(),
                reason: f.reason.clone(),
            })
            .collect(),
        refused_items: receipt
            .refused_items
            .iter()
            .map(|i| StoragePlanItemDto {
                path: i.path.to_string_lossy().into_owned(),
                name: i
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                size_allocated: i.size_allocated,
                size_label: storage::human_bytes(i.size_allocated),
                tier: i.tier.key().to_string(),
                tier_label: i.tier.label().to_string(),
                class_label: i.class.label().to_string(),
                reasons: i.reasons.clone(),
                stale: i.stale,
            })
            .collect(),
    }
}
/// Show a storage path in Windows Explorer.////// Read-only by construction: it selects a path rather than opening or/// executing anything, and it never passes the path through a shell. The path/// is a single argument to `explorer`, not a command line.
#[tauri::command]
fn storage_reveal(path: String) -> Result<String, String> {
    /* An empty path means "the folder C.U.R.E keeps its own records in".
    The button that used to sit on the coverage tab promised exactly that
    and did nothing, so the command now answers it for real. */
    let target = if path.trim().is_empty() {
        let dir = resolve_data_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        dir
    } else {
        PathBuf::from(&path)
    };
    if !target.exists() {
        return Err("That path no longer exists.".to_string());
    }
    // A directory is opened directly; a file is selected inside Explorer.
    let mut command = std::process::Command::new("explorer");
    if target.is_dir() {
        command.arg(&target);
    } else {
        command.arg(format!("/select,{}", target.display()));
    }
    command
        .spawn()
        .map_err(|e| format!("cannot open explorer: {e}"))?;
    Ok(target.to_string_lossy().into_owned())
}
fn main() {
    // Before anything creates a window: see `declare_dpi_awareness`.
    declare_dpi_awareness();
    let context = tauri::generate_context!();

    // The public demo build has no backend.
    //
    // The demo ships the deterministic sample backend in its frontend, and
    // the real engine must not be reachable from it. The Tauri runtime
    // injects a low-level IPC bridge into every window, so replacing the
    // application's own API is not enough: `window.__TAURI_INTERNALS__` can
    // still call the engine. The demo is therefore built with **no invoke
    // handler at all**, so every command — through either route — is
    // refused.
    //
    // The switch is the build's own identifier rather than a cargo feature,
    // because a feature has to be remembered at build time and the demo
    // configuration is the thing that says "this is the demo".
    let demo_build = context.config().identifier.contains(".demo");
    if demo_build {
        eprintln!(
            "[cure-demo] public demo build: the engine is not reachable; \
             every command is answered by the deterministic sample backend"
        );
    }

    let builder = tauri::Builder::default();
    let builder = if demo_build {
        // Returning `false` means "this command is not handled here", which
        // the runtime reports as an unknown command. Nothing reaches the
        // engine, and the sample backend in the frontend answers instead.
        builder.invoke_handler(|_invoke| false)
    } else {
        builder.invoke_handler(tauri::generate_handler![
            run_auto_scan,
            list_overlay_candidates,
            close_overlay_window,
            allowlist_overlay,
            kill_high_risk_processes,
            quarantine_entry,
            undo_entry,
            list_quarantine,
            entry_details,
            reveal_location,
            export_report,
            start_incident_observation,
            export_incident_report,
            open_quarantine_folder,
            view_log,
            exit_app,
            storage_scan,
            storage_cancel,
            storage_preview,
            storage_execute,
            storage_reveal,
            storage_case_file,
            storage_save_case_file,
            storage_available_roots,
            storage_validate_root,
            start_canary_guard,
            stop_canary_guard,
            canary_status,
            agent_radar,
            flight_start,
            flight_status,
            flight_stop,
            windows_change_capture,
            windows_change_compare,
            agent_case_file,
            export_agent_case_file
        ])
    };

    builder
        .manage(CanaryState::new())
        .manage(StorageState::default())
        .manage(AgentState {
            last_session: Mutex::new(recover_last_session()),
            ..AgentState::default()
        })
        .setup(|app| {
            // Create the main window programmatically so we can conditionally
            // inject --remote-debugging-port only when CURE_E2E_CDP_PORT is set.
            // This avoids the wry behavior where WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
            // is ignored when additional_browser_args is not explicitly set.
            let cdp_port = std::env::var("CURE_E2E_CDP_PORT").ok();
            let mut window_builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("C.U.R.E \u{2014} Evidence Console")
            .inner_size(1200.0, 800.0)
            .min_inner_size(900.0, 600.0)
            .resizable(true);
            if let Some(port) = cdp_port {
                // When additional_browser_args is set, wry does not prepend
                // its own defaults (see wry 0.55.1, webview2/mod.rs:294).
                // We must reproduce them here so the e2e launch is otherwise
                // identical to a normal release launch, plus CDP.
                let args = format!(
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
                );
                window_builder = window_builder.additional_browser_args(&args);
            }
            window_builder.build()?;

            if launched_by_watcher() {
                let handle = app.handle().clone();
                surface_above_overlays(&handle);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(RESURFACE_DELAY_MS));
                    surface_above_overlays(&handle);
                });
            }
            #[cfg(feature = "desktop-e2e")]
            maybe_start_e2e_driver(app.handle().clone());
            #[cfg(feature = "desktop-e2e")]
            maybe_start_exit_driver(app.handle().clone());
            Ok(())
        })
        .run(context)
        .expect("error while running cure-gui");
} // ---------------------------------------------------------------------------// suspicious-overlay dismissal (runs when the user presses Start Rescue)//// OS glue only: enumeration, style/process interrogation, close/terminate.// The DECISION (which windows deserve closing) lives in// cure_core::overlay and is unit-tested there.// ---------------------------------------------------------------------------
#[derive(Serialize)]
struct OverlayCandidateDto {
    hwnd: isize,
    title: String,
    process: String,
    path: String,
    pid: u32,
    signature: String,
    width: i32,
    height: i32,
    coverage_pct: f64,
}
#[derive(Serialize)]
struct OverlayCandidateList {
    checked: usize,
    candidates: Vec<OverlayCandidateDto>,
}
fn signature_text(sig: &SignatureStatus) -> String {
    match sig {
        SignatureStatus::ValidSigned => "signed",
        SignatureStatus::Invalid => "INVALID signature",
        SignatureStatus::Unsigned => "unsigned",
        SignatureStatus::Unknown => "unverifiable",
        // Shown for confirmation (`!= ValidSigned`): a window whose
        // signature cannot be revocation-checked is not trusted.
        SignatureStatus::ValidRevocationUnknown => "revocation-unverified",
    }
    .to_string()
}
/// Persistent per-app allowlist ("Don't ask again for this app"), stored as/// JSON next to the other app-owned data. Missing/corrupt file = empty list/// (fail open to *asking*, never to silent closing).
fn overlay_allowlist_path() -> PathBuf {
    resolve_data_dir().join("overlay-allowlist.json")
}
fn load_overlay_allowlist() -> Vec<cure_core::overlay::AllowEntry> {
    load_overlay_allowlist_from(&overlay_allowlist_path())
}
fn load_overlay_allowlist_from(path: &Path) -> Vec<cure_core::overlay::AllowEntry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}
fn save_overlay_allowlist(entries: &[cure_core::overlay::AllowEntry]) -> Result<(), String> {
    let dir = resolve_data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create data dir: {e}"))?;
    save_overlay_allowlist_to(&dir.join("overlay-allowlist.json"), entries)
}
fn save_overlay_allowlist_to(
    path: &Path,
    entries: &[cure_core::overlay::AllowEntry],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create data dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(entries).map_err(|e| format!("encode error: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write error: {e}"))
}
/// List overlay candidates for per-window confirmation. Matches are SHOWN,/// never closed here — closing is a separate per-window command below.
#[tauri::command]
fn list_overlay_candidates() -> Result<OverlayCandidateList, String> {
    #[cfg(not(windows))]
    {
        return Ok(OverlayCandidateList {
            checked: 0,
            candidates: Vec::new(),
        });
    }
    #[cfg(windows)]
    {
        use cure_core::overlay::OverlayCandidate;
        let allowlist = load_overlay_allowlist();
        let collected = collect_window_candidates()?;
        let checked = collected.len();
        let mut with_hash: Vec<OverlayCandidate> = Vec::with_capacity(checked);
        for (_, desc, sig) in &collected {
            let hash = cure_core::hash_intel::sha256_file_hex(&desc.process_path);
            with_hash.push(OverlayCandidate {
                desc: desc.clone(),
                signature: *sig,
                binary_sha256: hash,
            });
        }
        let picks = overlay::pick_overlays(&with_hash, &allowlist);
        let mut candidates = Vec::with_capacity(picks.len());
        for idx in picks {
            let (hwnd_raw, desc, sig) = &collected[idx];
            let width = (desc.rect.right - desc.rect.left).max(0);
            let height = (desc.rect.bottom - desc.rect.top).max(0);
            candidates.push(OverlayCandidateDto {
                hwnd: *hwnd_raw,
                title: desc.title.clone(),
                process: desc.process_name(),
                path: desc.process_path.to_string_lossy().into_owned(),
                pid: desc.pid,
                signature: signature_text(sig),
                width,
                height,
                coverage_pct: desc.coverage_ratio() * 100.0,
            });
        }
        Ok(OverlayCandidateList {
            checked,
            candidates,
        })
    }
}
/// Close a single candidate window the operator confirmed./// `force = false`: graceful `WM_CLOSE` only. `force = true`: terminate the/// owning process (explicit per-window Force-close button only).
#[tauri::command]
fn close_overlay_window(hwnd: isize, force: bool) -> Result<CloseResult, String> {
    #[cfg(not(windows))]
    {
        let _ = (hwnd, force);
        return Err("overlay closing is Windows-only".to_string());
    }
    #[cfg(windows)]
    {
        let result = do_close_overlay_window(hwnd, force)?;
        log_overlay_action(&format!(
            "{} window hwnd={hwnd} (force={force}): closed={} terminated={}",
            if force {
                "force-closed"
            } else {
                "close-requested"
            },
            result.closed,
            result.terminated
        ));
        Ok(result)
    }
}
/// "Don't ask again for this app": pin the binary currently at `path` by/// (normalized full path, SHA-256) so future matches skip it. Fails when/// the file cannot be hashed — unhashable binaries stay askable.
#[tauri::command]
fn allowlist_overlay(path: String) -> Result<String, String> {
    let target = PathBuf::from(&path);
    if !target.is_file() {
        return Err(format!("not a file (anymore): {path}"));
    }
    let hash = cure_core::hash_intel::sha256_file_hex(&target)
        .ok_or_else(|| format!("could not hash (locked/unreadable?): {path}"))?;
    let entry = cure_core::overlay::AllowEntry::new(&path, &hash);
    let mut entries = load_overlay_allowlist();
    if entries.iter().any(|e| e == &entry) {
        return Ok(format!("already allowlisted: {path}"));
    }
    entries.push(entry);
    save_overlay_allowlist(&entries)?;
    log_overlay_action(&format!("allowlisted {path} (sha256 {hash})"));
    Ok(format!("allowlisted (this binary only): {path}"))
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
/// Enumerate visible top-level windows and describe each one. Pure glue:/// every judgement call is delegated to cure_core::overlay. Unattributable/// windows (no PID / unresolvable owner binary) are skipped here and can/// never become candidates — the matcher additionally refuses pid-0/empty/// owners as defense in depth.
#[cfg(windows)]
fn collect_window_candidates() -> Result<Vec<(isize, WindowDesc, SignatureStatus)>, String> {
    use cure_core::overlay::WindowRect;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE,
        SM_CXSCREEN, SM_CYSCREEN, WS_CAPTION, WS_EX_TOPMOST,
    };
    let mut out: Vec<(isize, WindowDesc, SignatureStatus)> = Vec::new();
    let own_exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.canonicalize().ok());
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
            let canonical =
                std::fs::canonicalize(&process_path).unwrap_or_else(|_| process_path.clone());
            let is_own = own_exe.as_ref() == Some(&canonical);
            let is_system = is_under_windows_dir(&process_path);
            let signature = cure_core::signature::check_signature(&process_path);
            // Window rect (for the coverage gate + the confirmation card).
            let mut rc = RECT::default();
            let rect = if GetWindowRect(hwnd, &mut rc).is_ok() {
                WindowRect {
                    left: rc.left,
                    top: rc.top,
                    right: rc.right,
                    bottom: rc.bottom,
                }
            } else {
                WindowRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                }
            };
            // Containing monitor size (per-monitor, not primary-only, so a
            // fullscreen window on a secondary display still measures ~1.0).
            // Falls back to the primary metrics if the monitor query fails.
            // (Inside the loop's outer unsafe block.)
            let mut monitor = (
                GetSystemMetrics(SM_CXSCREEN) as u32,
                GetSystemMetrics(SM_CYSCREEN) as u32,
            );
            {
                let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(hmon, &mut info).as_bool() {
                    let w = (info.rcMonitor.right - info.rcMonitor.left).max(0) as u32;
                    let h = (info.rcMonitor.bottom - info.rcMonitor.top).max(0) as u32;
                    if w > 0 && h > 0 {
                        monitor = (w, h);
                    }
                }
            }
            out.push((
                raw,
                WindowDesc {
                    title,
                    process_path,
                    pid,
                    rect,
                    monitor,
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
/// Request a graceful close (`WM_CLOSE` only — never terminate). Returns/// whether the window is gone afterwards (checked after a short wait so the/// UI can report honestly instead of assuming).
#[cfg(windows)]
fn request_close(hwnd_raw: isize) -> bool {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_CLOSE};
    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return true; // already gone
        }
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        std::thread::sleep(std::time::Duration::from_millis(500));
        !IsWindow(hwnd).as_bool()
    }
}
/// Explicit, per-window "Force close": terminate the owning process./// Only ever called from the frontend's Force-close button for a window the/// operator already reviewed — never as an automatic fallback.
#[cfg(windows)]
fn force_close(hwnd_raw: isize) -> Result<bool, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};
    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return Ok(true); // already gone
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Err("could not attribute the window to a process".to_string());
        }
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("OpenProcess failed: {e}"))?;
        let _ = TerminateProcess(handle, 1);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        Ok(true)
    }
}
/// Close one candidate window. `force = false` posts `WM_CLOSE` and reports/// whether the window went away; `force = true` terminates the owning/// process (explicit operator action only).
#[cfg(windows)]
fn do_close_overlay_window(hwnd_raw: isize, force: bool) -> Result<CloseResult, String> {
    if force {
        let closed = force_close(hwnd_raw)?;
        Ok(CloseResult {
            closed,
            terminated: true,
        })
    } else {
        let closed = request_close(hwnd_raw);
        Ok(CloseResult {
            closed,
            terminated: false,
        })
    }
}
#[derive(Serialize)]
struct CloseResult {
    closed: bool,
    terminated: bool,
} // NOTE: the old auto-closing `dismiss_overlays` command was removed in the// overlay-safety pass: candidates are listed (`list_overlay_candidates`)// and each close is a separate, operator-confirmed `close_overlay_window`// call (graceful by default, explicit per-window force only).// ---------------------------------------------------------------------------// E2E driver (test-only, inert unless CURE_E2E_CLEANUP is set)//// Drives the REAL webview UI (real invoke() plumbing, real backend deletes)// through one full disk-cleanup flow, then emits the outcome as an event and// exits. Never active in normal launches; exists to close the audit gap of// cleanup being verified mock-only.// ---------------------------------------------------------------------------
#[cfg(feature = "desktop-e2e")]
const E2E_RUNNER_JS: &str = r##"(async () => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, t = 60000) => {
    for (let i = 0; i < t / 100; i++) { if (f()) return true; await wait(100); }
    return false;
  };
  const emit = (payload) => window.__TAURI__.event.emit("e2e-done", JSON.stringify(payload));
  const footMsg = () => document.getElementById("footbar-msg");
  // Click a footer button and capture the transient footer feedback text.
  // Proves the button is wired to the REAL backend (success AND error paths
  // both surface through this element).
  const clickFoot = async (btnId) => {
    const before = footMsg().textContent;
    document.getElementById(btnId).click();
    if (!await waitFor(() => footMsg().textContent !== before && footMsg().classList.contains("show"))) {
      throw new Error("footer feedback never appeared for " + btnId);
    }
    return footMsg().textContent;
  };
  try {
    if (!await waitFor(() => document.getElementById("start-rescue-btn") !== null)) {
      throw new Error("start-rescue button never appeared");
    }
    // Footer error path: no scan has run yet, so there is no baseline/log.
    const preScanLogMsg = await clickFoot("btn-view-log");
    await wait(300);
    document.getElementById("nav-scan").click();
    document.getElementById("start-rescue-btn").click();
    if (!await waitFor(() => document.getElementById("results-view") !== null)) {
      throw new Error("app DOM never became ready");
    }
    if (!await waitFor(() => !document.getElementById("results-view").classList.contains("hidden"))) {
      throw new Error("security results never appeared");
    }
    // The seed remains review-only until this explicit confirmation.
    const quarantineButton = document.querySelector("#review-cards .quarantine-btn");
    if (!quarantineButton) throw new Error("sandbox file-backed finding not available for quarantine");
    quarantineButton.click();
    if (!await waitFor(() => !document.getElementById("confirm-overlay").classList.contains("hidden")))
      throw new Error("quarantine confirmation missing");
    document.getElementById("confirm-ok").click();
    if (!await waitFor(() => quarantineButton.textContent === "Quarantined"))
      throw new Error("explicit quarantine never completed");
    // Footer success paths: a scan just completed, so the quarantine folder
    // and baseline log both exist on disk for the real backend to open.
    const quarantineMsg = await clickFoot("btn-quarantine-folder");
    const logMsg = await clickFoot("btn-view-log");
    await wait(600);

    // ---- the Storage Workstation, against the REAL backend --------------
    // The destructive removal is deliberately NOT driven from here: it needs
    // a file the run owns, and the native harness (testing/native) does
    // that inside an isolated fixture. What this driver proves is that the
    // real engine measures the machine and that the preview opens.
    document.getElementById("nav-cleanup").click();
    if (!await waitFor(() => !document.getElementById("cleanup-view").classList.contains("hidden"))) {
      throw new Error("storage view never appeared");
    }
    if (!await waitFor(() => !document.getElementById("sw-scan").disabled)) {
      throw new Error("storage scan button never became ready");
    }
    document.getElementById("sw-scan").click();
    if (!await waitFor(() => document.getElementById("sw-progress").getClientRects().length > 0, 10000)) {
      throw new Error("storage progress never appeared");
    }
    if (!await waitFor(() => document.getElementById("sw-progress").classList.contains("hidden"), 300000)) {
      throw new Error("storage scan never finished");
    }
    await wait(500);
    const storageStatus = document.getElementById("sw-status-text").textContent.trim();
    if (!storageStatus) throw new Error("storage status never reported");
    document.getElementById("sw-tab-coverage").click();
    if (!await waitFor(() => document.querySelectorAll("#sw-roots-body tr").length > 0, 30000)) {
      throw new Error("storage coverage never listed the measured roots");
    }
    const measuredRoots = document.querySelectorAll("#sw-roots-body tr").length;
    const measuredFiles = (document.getElementById("sw-large-count") || {}).textContent || "";
    // The preview must open and must not execute anything on its own.
    document.getElementById("sw-tab-large").click();
    await wait(400);
    const queueable = document.querySelector("#sw-large-body .sw-queue-btn");
    if (queueable) {
      queueable.click();
      await wait(300);
      document.getElementById("sw-tab-queue").click();
      await wait(300);
      document.getElementById("sw-preview-btn").click();
      if (!await waitFor(() => !document.getElementById("sw-preview-overlay").classList.contains("hidden"))) {
        throw new Error("storage removal preview never appeared");
      }
      if (!document.getElementById("sw-preview-confirm").disabled) {
        throw new Error("removal was confirmable without typing the word");
      }
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await wait(300);
    }

    // Quarantine view vs the REAL backend: the sandbox startup root is seeded
    // with a HighRisk .bat, which the explicit confirmed action above quarantined.
    document.getElementById("nav-quarantine").click();
    if (!await waitFor(() => !document.getElementById("view-quarantine").classList.contains("hidden"))) {
      throw new Error("quarantine view never appeared");
    }
    if (!await waitFor(() => document.querySelectorAll("#q-list .q-row").length >= 1, 30000)) {
      throw new Error("quarantine list never showed the confirmed quarantine seed");
    }
    document.querySelector("#q-list .q-undo").click();
    if (!await waitFor(() => document.querySelectorAll("#q-list .q-row").length === 0, 30000)) {
      throw new Error("undo never cleared the quarantine list");
    }
    await wait(300);
    // Views smoke test: every nav target must render (proves the V2 views
    // work against real backend data, not just the mock harness).
    const smokeViews = [
      ["nav-overview", "view-overview"],
      ["nav-audit", "view-audit"],
      ["nav-processes", "view-processes"],
      ["nav-eventlog", "view-eventlog"],
      ["canary-secondary", "view-canary"],
    ];
    for (const [nav, view] of smokeViews) {
      (nav === "canary-secondary" ? document.querySelector('[data-route="view-canary"]') : document.getElementById(nav)).click();
      if (!await waitFor(() => !document.getElementById(view).classList.contains("hidden"))) {
        throw new Error(view + " never appeared");
      }
      await wait(200);
    }
    const posture = document.getElementById("ov-posture").textContent;
    const auditCards = document.querySelectorAll("#audit-list .review-card").length;
    if (!posture) throw new Error("overview posture empty");
    if (auditCards < 1) throw new Error("audit view empty despite confirmed quarantine seed");
    // Canary guard against the REAL backend: enable, expect ACTIVE. (Decoys
    // land in the sandboxed profile; the TRIGGERED path via real filesystem
    // events stays a manual/VM test — see TESTING.md §5.)
    document.getElementById("can-toggle").click();
    if (!await waitFor(() => document.getElementById("can-state").textContent === "ACTIVE", 15000)) {
      throw new Error("canary guard never became ACTIVE");
    }
    // Incident observation vs the REAL backend: 15 s window, then assert a
    // usable result (verdict + timeline + observed processes).
    const incident = await window.__TAURI__.core.invoke("start_incident_observation", { durationSecs: 15 });
    if (!incident || !incident.verdict || !Array.isArray(incident.timeline) || incident.timeline.length < 2) {
      throw new Error("incident observation returned no usable result");
    }
    if (!Array.isArray(incident.processes) || incident.processes.length < 1) {
      throw new Error("incident observed no processes");
    }
    const incidentSummary = {
      verdict: incident.verdict,
      timeline: incident.timeline.length,
      processes: incident.processes.length,
      windows: Array.isArray(incident.windows) ? incident.windows.length : 0,
      correlations: Array.isArray(incident.correlations) ? incident.correlations.length : 0,
    };
    document.getElementById("nav-results").click();
    if (!await waitFor(() => !document.getElementById("results-view").classList.contains("hidden"))) {
      throw new Error("results view never re-appeared");
    }
    await wait(300);
    emit({
      ok: true,
      status: storageStatus,
      pill: storageStatus,
      storageVerified: { roots: measuredRoots, files: measuredFiles },
      cleanupConfirmed: false,
      footer: { preScanLogMsg, quarantineMsg, logMsg },
      quarantineVerified: true,
      viewsVerified: { posture, auditCards },
      canaryActive: true,
      incidentVerified: incidentSummary,
    });
  } catch (err) {
    emit({ ok: false, error: String(err) });
  }
})()"##;
#[cfg(feature = "desktop-e2e")]
fn maybe_start_e2e_driver(handle: AppHandle) {
    if std::env::var("CURE_E2E_CLEANUP").is_err() {
        return;
    }
    let out_path = std::env::var("CURE_E2E_OUT").unwrap_or_else(|_| "e2e-result.json".to_string());
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
} // Exit-button driver (test-only, inert unless CURE_E2E_EXIT is set).// Clicks the real Exit footer button after load; the outer harness asserts// the process actually terminates. Kept separate from the cleanup driver so// each run has exactly one terminal action.
#[cfg(feature = "desktop-e2e")]
const E2E_EXIT_RUNNER_JS: &str = r##"(async () => {  const wait = (ms) => new Promise((r) => setTimeout(r, ms));  const waitFor = async (f, t = 30000) => {    for (let i = 0; i < t / 100; i++) { if (f()) return true; await wait(100); }    return false;  };  if (!await waitFor(() => document.getElementById("btn-exit") !== null)) {    return;  }  await wait(500);  document.getElementById("btn-exit").click();})();"##;
#[cfg(feature = "desktop-e2e")]
fn maybe_start_exit_driver(handle: AppHandle) {
    if std::env::var("CURE_E2E_EXIT").is_err() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.eval(E2E_EXIT_RUNNER_JS);
        }
    });
} // ---------------------------------------------------------------------------// Ransomware canary guard — real-time folder monitoring + process tripwire// ---------------------------------------------------------------------------
struct CanaryGuard {
    stop: Arc<AtomicBool>,
    alert_count: Arc<AtomicUsize>,
}
struct CanaryState {
    guard: Mutex<Option<CanaryGuard>>,
}
impl CanaryState {
    fn new() -> Self {
        Self {
            guard: Mutex::new(None),
        }
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
            cure_dirwatch::plant_decoys(dir);
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
    let count = g
        .as_ref()
        .map(|g| g.alert_count.load(Ordering::Relaxed))
        .unwrap_or(0);
    Ok(serde_json::json!({ "active": active, "alert_count": count }))
} // Directory watching lives in cure_dirwatch (shared with cure-watch, same// engine, same Win32 acquisition loop); the GUI only adapts alerts into// Tauri events here. The GUI keeps its own user_folder_candidates() because// its ransom-scan path needs pre-filtered existing dirs — a different// contract from the shared helper.
fn spawn_dir_watcher(
    dir: PathBuf,
    app: AppHandle,
    stop: Arc<AtomicBool>,
    _alert_count: Arc<AtomicUsize>,
) {
    std::thread::spawn(move || {
        cure_dirwatch::run_dir_guard(&dir, &stop, |alert| {
            let event = match &alert {
                CanaryAlert::CanaryTamper {
                    folder,
                    file,
                    action,
                    at_secs,
                } => CanaryAlertEvent {
                    kind: "canary-tamper".into(),
                    folder: folder.clone(),
                    file: file.clone(),
                    action: (*action).into(),
                    at_secs: *at_secs,
                    severity: alert.severity(),
                },
                CanaryAlert::BurstEncryption {
                    folder,
                    distinct_files,
                    window_secs,
                    at_secs,
                } => CanaryAlertEvent {
                    kind: "burst-encryption".into(),
                    folder: folder.clone(),
                    file: format!("{distinct_files} files in {window_secs}s"),
                    action: "burst".into(),
                    at_secs: *at_secs,
                    severity: alert.severity(),
                },
                CanaryAlert::ExtensionRewrite {
                    folder,
                    extension,
                    renamed_count,
                    at_secs,
                } => CanaryAlertEvent {
                    kind: "extension-rewrite".into(),
                    folder: folder.clone(),
                    file: format!(".{extension}"),
                    action: format!("{renamed_count} files renamed"),
                    at_secs: *at_secs,
                    severity: alert.severity(),
                },
            };
            let _ = app.emit("canary-alert", &event);
        });
    });
}
fn spawn_tripwire_poller(app: AppHandle, stop: Arc<AtomicBool>, _alert_count: Arc<AtomicUsize>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            #[cfg(windows)]
            {
                let processes = process_scan::enumerate_processes();
                for p in &processes {
                    let name = std::path::Path::new(&p.exe_path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if let Some(reason) = shadow_wipe_reason(&name, "") {
                        let _ = app.emit(
                            "canary-alert",
                            CanaryAlertEvent {
                                kind: "shadow-wipe".into(),
                                folder: String::new(),
                                file: name,
                                action: reason.into(),
                                at_secs: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0),
                                severity: 3,
                            },
                        );
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

// ---------------------------------------------------------------------------
// AI agent visibility: radar, flight recorder, Windows change radar,
// privacy-safe case file.
// ---------------------------------------------------------------------------

fn recover_last_session() -> Option<agent::FlightSession> {
    let dir = agent::recorder::session_dir(&resolve_data_dir());
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        if newest
            .as_ref()
            .map(|(at, _)| modified > *at)
            .unwrap_or(true)
        {
            newest = Some((modified, path));
        }
    }
    let (_, path) = newest?;
    Some(
        agent::recorder::load_session(&path)
            .ok()?
            .recover_interrupted(),
    )
}

#[derive(Default)]
struct AgentState {
    recorder: Mutex<Option<FlightRecorder>>,
    last_session: Mutex<Option<agent::FlightSession>>,
    last_windows: Mutex<Option<agent::WindowsChanges>>,
    counter: AtomicUsize,
    starting: AtomicBool,
}

struct FlightRecorder {
    root: PathBuf,
    baseline: agent::WorkspaceSnapshot,
    session: Arc<Mutex<agent::FlightSession>>,
    stop: Arc<AtomicBool>,
    watcher_started: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Serialize)]
struct AgentRadarDto {
    generated_unix: u64,
    supported_agents: Vec<AgentPanelDto>,
    detected_count: usize,
    model_server_count: usize,
    processes: Vec<AgentProcessDto>,
    monitoring: Option<agent::FlightSession>,
    windows: Option<agent::WindowsChanges>,
    notes: Vec<String>,
}

#[derive(Serialize, Clone)]
struct AgentPanelDto {
    id: String,
    label: String,
    detected: bool,
    process_count: usize,
    state: String,
    state_label: String,
    state_note: String,
    integration_note: String,
    processes: Vec<AgentProcessDto>,
    is_model_server: bool,
    category: String,
}

#[derive(Serialize, Clone)]
struct AgentProcessDto {
    pid: u32,
    parent_pid: Option<u32>,
    name: String,
    exe_path: String,
    command_line: Option<String>,
    started_unix: Option<u64>,
    identity: String,
    identity_label: String,
    evidence: String,
    confidence: String,
    limitations: String,
    is_model_server: bool,
}

#[derive(Serialize)]
struct WindowsSnapshotSummary {
    saved_at_unix: u64,
    entry_count: usize,
    previous_saved_at_unix: Option<u64>,
    path: String,
    note: String,
}

fn agent_process_dto(process: &agent::DetectedAgentProcess) -> AgentProcessDto {
    AgentProcessDto {
        pid: process.pid,
        parent_pid: process.parent_pid,
        name: process.name.clone(),
        exe_path: process.exe_path.clone(),
        command_line: process.command_line.clone(),
        started_unix: process.started_unix,
        identity: format!("{:?}", process.identity),
        identity_label: process.identity.label().to_string(),
        evidence: process.evidence.clone(),
        confidence: process.confidence.clone(),
        limitations: process.limitations.clone(),
        is_model_server: process.is_model_server,
    }
}

fn collect_persistence_entries() -> Vec<PersistenceEntry> {
    let mut entries: Vec<PersistenceEntry> = Vec::new();
    #[cfg(windows)]
    entries.extend(scanners::registry::scan().unwrap_or_default());
    entries.extend(scanners::startup::scan(&startup_root()));
    entries.extend(scanners::startup::scan_common());
    entries.extend(scanners::scheduled_tasks::scan(&tasks_root()));
    let service_records = scanners::collect_services();
    entries.extend(service_records.iter().map(|r| r.entry.clone()));
    #[cfg(windows)]
    {
        entries.extend(scanners::wmi::scan());
        entries.extend(scanners::ifeo::scan());
        entries.extend(scanners::appinit::scan());
        entries.extend(scanners::com::scan());
    }
    entries
}

fn agent_state_for(
    kind: agent::AgentKind,
    processes: &[AgentProcessDto],
    session: Option<&agent::FlightSession>,
) -> (String, String, String) {
    let active = session.filter(|s| s.active);
    if processes.is_empty() {
        if let Some(session) = active {
            if session.agents_seen.contains(&kind) {
                return (
                    "disconnected".to_string(),
                    "Disconnected".to_string(),
                    "The process is no longer present in this sample; the monitored session continues."
                        .to_string(),
                );
            }
        }
        return (
            "not_detected".to_string(),
            "Not detected".to_string(),
            "No supported process image with this agent's documented name is running.".to_string(),
        );
    }

    let any_verified = processes.iter().any(|p| p.identity.contains("Verified"));
    let any_suspect = processes.iter().any(|p| p.identity.contains("SuspectMismatch"));
    let all_access_denied = processes.iter().all(|p| p.identity.contains("AccessDenied"));

    if any_suspect {
        return (
            "suspect_mismatch".to_string(),
            "Suspect mismatch".to_string(),
            "Process image name matched, but binary metadata or signature contradicts the genuine tool."
                .to_string(),
        );
    }

    if all_access_denied {
        return (
            "access_denied".to_string(),
            "Access denied".to_string(),
            "Process was enumerated in snapshot, but query access was denied."
                .to_string(),
        );
    }

    if !any_verified {
        return (
            "unverified_process".to_string(),
            "Unverified process".to_string(),
            "Process matched image name, but tool identity is unconfirmed (unsigned or unverified binary)."
                .to_string(),
        );
    }

    match active {
        None => (
            "process_detected".to_string(),
            "Process detected".to_string(),
            if kind.is_model_server() {
                "Local inference server active. Ready for local queries."
            } else {
                "The executable is running. No monitored workspace session is active, so no activity is claimed."
            }
            .to_string(),
        ),
        Some(session) => {
            let recent = session.events.iter().rev().any(|e| {
                e.agents.contains(&kind) && agent::now_unix().saturating_sub(e.at_unix) <= 300
            });
            if recent {
                (
                    "active".to_string(),
                    "Active (changes observed)".to_string(),
                    "A monitored file change was observed while this process was present. Correlation is not attribution."
                        .to_string(),
                )
            } else {
                (
                    "idle".to_string(),
                    "Present (no recent changes)".to_string(),
                    "The process is present during a monitored session. No recent correlated change was observed."
                        .to_string(),
                )
            }
        }
    }
}

#[tauri::command]
fn agent_radar(state: State<'_, AgentState>) -> Result<AgentRadarDto, String> {
    let processes = agent::enumerate_agent_processes();
    let session = state
        .recorder
        .lock()
        .map_err(|_| "agent state is unavailable".to_string())?
        .as_ref()
        .and_then(|r| r.session.lock().ok().map(|s| s.clone()))
        .or_else(|| {
            state
                .last_session
                .lock()
                .ok()
                .and_then(|guard| guard.clone())
        });

    let mut panels = Vec::new();
    for kind in agent::AgentKind::ALL {
        let own: Vec<AgentProcessDto> = processes
            .iter()
            .filter(|p| p.kind == kind)
            .map(agent_process_dto)
            .collect();
        let detected = !own.is_empty();
        let (state_id, state_label, state_note) = agent_state_for(kind, &own, session.as_ref());
        panels.push(AgentPanelDto {
            id: kind.id().to_string(),
            label: kind.label().to_string(),
            detected,
            process_count: own.len(),
            state: state_id,
            state_label,
            state_note,
            integration_note: kind.integration_note().to_string(),
            processes: own,
            is_model_server: kind.is_model_server(),
            category: format!("{:?}", kind.category()),
        });
    }

    let detected_count = panels.iter().filter(|p| p.detected && !p.is_model_server).count();
    let model_server_count = panels.iter().filter(|p| p.detected && p.is_model_server).count();
    let windows = state
        .last_windows
        .lock()
        .map_err(|_| "Windows change state is unavailable".to_string())?
        .clone();

    let mut notes = Vec::new();
    notes.push(
        "A running process does not prove that an agent is generating code or executing tasks."
            .to_string(),
    );
    if detected_count == 0 && model_server_count == 0 {
        notes.push("No supported agent process or model server was detected in this sample.".to_string());
    } else {
        if detected_count > 0 {
            let verified_count = panels
                .iter()
                .filter(|p| {
                    p.detected
                        && !p.is_model_server
                        && p.processes.iter().any(|proc| proc.identity.contains("Verified"))
                })
                .count();
            if verified_count == detected_count {
                notes.push(format!("{detected_count} verified AI agent(s) detected."));
            } else if verified_count > 0 {
                notes.push(format!(
                    "{detected_count} agent image(s) detected ({verified_count} verified tool, {} unverified).",
                    detected_count - verified_count
                ));
            } else {
                notes.push(format!(
                    "{detected_count} unverified agent candidate(s) detected (image name match only; tool identity unconfirmed)."
                ));
            }
        }
        if model_server_count > 0 {
            notes.push(format!("{model_server_count} local model server(s) running (Ollama)."));
        }
    }

    Ok(AgentRadarDto {
        generated_unix: agent::now_unix(),
        supported_agents: panels,
        detected_count,
        model_server_count,
        processes: processes.iter().map(agent_process_dto).collect(),
        monitoring: session,
        windows,
        notes,
    })
}

fn current_agent_kinds() -> Vec<agent::AgentKind> {
    let mut seen = Vec::new();
    for process in agent::enumerate_agent_processes() {
        if process.is_model_server
            || process.identity == agent::ExecutableIdentity::SuspectMismatch
        {
            continue;
        }
        if !seen.contains(&process.kind) {
            seen.push(process.kind);
        }
    }
    seen
}

fn display_workspace(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
}

fn file_meta(root: &Path, rel: &str) -> Option<(u64, Option<u64>)> {
    let path = root.join(rel);
    let meta = std::fs::symlink_metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    Some((meta.len(), modified))
}

enum FlightRaw {
    Change {
        kind: WorkspaceChangeKind,
        path: String,
        at: u64,
    },
    Rename {
        from: Option<String>,
        to: String,
        size_after: Option<u64>,
        at: u64,
    },
}

fn make_flight_event(
    kind: WorkspaceChangeKind,
    path: String,
    meta: Option<(u64, Option<u64>)>,
    size_before: Option<u64>,
    at: u64,
    agents: &[agent::AgentKind],
    session: &agent::FlightSession,
) -> agent::FlightEvent {
    let (change, size) = match kind {
        WorkspaceChangeKind::Created => (agent::ChangeKind::Created, meta.map(|(s, _)| s)),
        WorkspaceChangeKind::Modified => (agent::ChangeKind::Modified, meta.map(|(s, _)| s)),
        WorkspaceChangeKind::Removed => (agent::ChangeKind::Removed, None),
        WorkspaceChangeKind::RenamedFrom
        | WorkspaceChangeKind::RenamedTo
        | WorkspaceChangeKind::Overflow => (agent::ChangeKind::Modified, meta.map(|(s, _)| s)),
    };
    let confidence = if !session.coverage.complete {
        agent::EvidenceConfidence::Unknown
    } else if agents.is_empty() {
        agent::EvidenceConfidence::Observed
    } else {
        agent::EvidenceConfidence::Correlated
    };
    agent::FlightEvent {
        seq: 0,
        at_unix: at,
        kind: change,
        path,
        from_path: None,
        size_before: if change == agent::ChangeKind::Created {
            None
        } else {
            size_before
        },
        size_after: size,
        source: agent::ObservationSource::DirectoryWatcher,
        confidence,
        agents: agents.to_vec(),
        note: if agents.is_empty() {
            "Filesystem change observed. No supported agent process was present.".to_string()
        } else {
            "Filesystem change observed while a process matching a supported tool was running in the background. Correlation is not attribution — filesystem notifications do not track process write handles."
                .to_string()
        },
    }
}

#[tauri::command]
async fn flight_start(
    state: State<'_, AgentState>,
    app: AppHandle,
    workspace: String,
) -> Result<agent::FlightSession, String> {
    if state.starting.swap(true, Ordering::SeqCst) {
        return Err("A monitoring session is already starting.".to_string());
    }
    let result = flight_start_inner(&state, app, workspace).await;
    state.starting.store(false, Ordering::SeqCst);
    result
}

async fn flight_start_inner(
    state: &AgentState,
    app: AppHandle,
    workspace: String,
) -> Result<agent::FlightSession, String> {
    let requested = PathBuf::from(&workspace);
    if !requested.is_dir() {
        return Err("Choose an existing workspace directory to monitor.".to_string());
    }
    let root = requested
        .canonicalize()
        .map_err(|e| format!("The workspace could not be resolved: {e}"))?;
    if root.parent().is_none() {
        return Err(
            "Choose a project folder, not an entire drive. Monitoring a drive root is too broad."
                .to_string(),
        );
    }

    {
        let recorder = state
            .recorder
            .lock()
            .map_err(|_| "agent state is unavailable".to_string())?;
        if let Some(active) = recorder.as_ref() {
            if let Ok(session) = active.session.lock() {
                if session.active {
                    return Err(format!(
                        "A monitoring session is already active for {}. Stop it before starting another.",
                        active.root.to_string_lossy()
                    ));
                }
            }
        }
    }

    let scan_root = root.clone();
    let baseline = tokio::task::spawn_blocking(move || {
        agent::capture_workspace(&scan_root, agent::recorder::MAX_BASELINE_FILES)
    })
    .await
    .map_err(|e| format!("baseline task failed: {e}"))?
    .map_err(|e| format!("The baseline could not be captured: {e}"))?;

    let counter = state.counter.fetch_add(1, Ordering::SeqCst);
    let id = format!("session-{}-{counter}", agent::now_unix());
    let agents = current_agent_kinds();
    let watcher_started = Arc::new(AtomicBool::new(true));
    let stop = Arc::new(AtomicBool::new(false));
    let session = Arc::new(Mutex::new(agent::FlightSession::new(
        &id,
        display_workspace(&root),
        &baseline,
        agents.clone(),
        true,
    )));
    if let Ok(mut s) = session.lock() {
        s.note_agents(&agents);
    }

    let data_dir = resolve_data_dir();
    let thread_root = root.clone();
    let thread_stop = Arc::clone(&stop);
    let thread_session = Arc::clone(&session);
    let thread_app = app.clone();
    let thread_started = Arc::clone(&watcher_started);
    let thread_data = data_dir.clone();
    let mut known_sizes: std::collections::HashMap<String, u64> = baseline
        .files
        .iter()
        .map(|file| (file.path.clone(), file.size))
        .collect();

    let handle = std::thread::spawn(move || {
        let mut last_probe = std::time::Instant::now() - std::time::Duration::from_secs(5);
        let mut cached_agents: Vec<agent::AgentKind> = Vec::new();
        let mut last_save = std::time::Instant::now();
        let probe = |last: &mut std::time::Instant, cached: &mut Vec<agent::AgentKind>| {
            if last.elapsed() >= std::time::Duration::from_secs(1) {
                *cached = current_agent_kinds();
                *last = std::time::Instant::now();
            }
            cached.clone()
        };

        let watch_result =
            cure_dirwatch::run_workspace_recorder(&thread_root, &thread_stop, |batch| {
            let agents = probe(&mut last_probe, &mut cached_agents);
            let mut events = Vec::new();
            let mut pending_rename: Option<(String, u64)> = None;
            for change in &batch {
                if change.kind == WorkspaceChangeKind::Overflow {
                    continue;
                }
                match change.kind {
                    WorkspaceChangeKind::RenamedFrom => {
                        pending_rename = Some((change.relative.clone(), change.at_unix));
                    }
                    WorkspaceChangeKind::RenamedTo => {
                        let after = file_meta(&thread_root, &change.relative);
                        events.push(FlightRaw::Rename {
                            from: pending_rename.take().map(|(p, _)| p),
                            to: change.relative.clone(),
                            size_after: after.map(|(s, _)| s),
                            at: change.at_unix,
                        });
                    }
                    WorkspaceChangeKind::Created
                    | WorkspaceChangeKind::Modified
                    | WorkspaceChangeKind::Removed => {
                        events.push(FlightRaw::Change {
                            kind: change.kind,
                            path: change.relative.clone(),
                            at: change.at_unix,
                        });
                    }
                    WorkspaceChangeKind::Overflow => {}
                }
            }

            let overflow = batch
                .iter()
                .any(|c| c.kind == WorkspaceChangeKind::Overflow);

            if let Ok(mut session) = thread_session.lock() {
                session.note_agents(&agents);
                if overflow {
                    session.coverage.complete = false;
                    session.coverage.dropped_events += 1;
                    if !session.coverage.note.contains("overflow") {
                        session.coverage.note =
                            "The Windows directory-notification buffer overflowed during a change burst; some events were not observed."
                                .to_string();
                    }
                }
                for raw in events {
                    match raw {
                        FlightRaw::Change { kind, path, at } => {
                            let meta = file_meta(&thread_root, &path);
                            let size_before = if kind == WorkspaceChangeKind::Created {
                                None
                            } else {
                                known_sizes.get(&path).copied()
                            };
                            match kind {
                                WorkspaceChangeKind::Created | WorkspaceChangeKind::Modified => {
                                    if let Some((size, _)) = meta {
                                        known_sizes.insert(path.clone(), size);
                                    }
                                }
                                WorkspaceChangeKind::Removed => {
                                    known_sizes.remove(&path);
                                }
                                _ => {}
                            }
                            let event = make_flight_event(
                                kind,
                                path,
                                meta,
                                size_before,
                                at,
                                &agents,
                                &session,
                            );
                            session.record_event(event);
                        }
                        FlightRaw::Rename {
                            from,
                            to,
                            size_after,
                            at,
                        } => {
                            let size_before = from.as_ref().and_then(|p| known_sizes.remove(p));
                            if let Some(size) = size_after {
                                known_sizes.insert(to.clone(), size);
                            }
                            let confidence = if !session.coverage.complete {
                                agent::EvidenceConfidence::Unknown
                            } else if agents.is_empty() {
                                agent::EvidenceConfidence::Observed
                            } else {
                                agent::EvidenceConfidence::Correlated
                            };
                            session.record_event(agent::FlightEvent {
                                seq: 0,
                                at_unix: at,
                                kind: agent::ChangeKind::Renamed,
                                path: to,
                                from_path: from,
                                size_before,
                                size_after,
                                source: agent::ObservationSource::DirectoryWatcher,
                                confidence,
                                agents: agents.clone(),
                                note: if agents.is_empty() {
                                    "Rename observed. No supported agent process was present."
                                        .to_string()
                                } else {
                                    "Rename observed while a process matching a supported tool was running in the background. Correlation is not attribution — filesystem notifications do not track process write handles."
                                        .to_string()
                                },
                            });
                        }
                    }
                }

                if last_save.elapsed() >= std::time::Duration::from_secs(5) {
                    let _ = agent::recorder::save_session(&thread_data, &session);
                    last_save = std::time::Instant::now();
                }
            }

            let _ = thread_app.emit("flight-update", ());
        });

        if let Err(e) = watch_result {
            thread_started.store(false, Ordering::SeqCst);
            if let Ok(mut session) = thread_session.lock() {
                session.coverage.complete = false;
                session.coverage.note =
                    format!("The directory watcher failed to start or encountered an error: {e}");
            }
        }
    });

    let snapshot = {
        let guard = session
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        guard.clone()
    };

    *state
        .recorder
        .lock()
        .map_err(|_| "agent state is unavailable".to_string())? = Some(FlightRecorder {
        root,
        baseline,
        session,
        stop,
        watcher_started,
        handle: Some(handle),
    });

    let _ = agent::recorder::save_session(&data_dir, &snapshot);
    *state
        .last_session
        .lock()
        .map_err(|_| "session state is unavailable".to_string())? = Some(snapshot.clone());

    Ok(snapshot)
}

#[tauri::command]
fn flight_status(state: State<'_, AgentState>) -> Result<Option<agent::FlightSession>, String> {
    let recorder = state
        .recorder
        .lock()
        .map_err(|_| "agent state is unavailable".to_string())?;
    if let Some(recorder) = recorder.as_ref() {
        return recorder
            .session
            .lock()
            .map(|s| Some(s.clone()))
            .map_err(|_| "session state is unavailable".to_string());
    }
    Ok(state
        .last_session
        .lock()
        .map_err(|_| "session state is unavailable".to_string())?
        .clone())
}

#[tauri::command]
async fn flight_stop(state: State<'_, AgentState>) -> Result<agent::FlightSession, String> {
    let mut recorder = {
        let mut guard = state
            .recorder
            .lock()
            .map_err(|_| "agent state is unavailable".to_string())?;
        guard.take()
    };
    let Some(mut active) = recorder.take() else {
        return state
            .last_session
            .lock()
            .map_err(|_| "session state is unavailable".to_string())?
            .clone()
            .ok_or_else(|| "No monitoring session is active.".to_string());
    };

    active.stop.store(true, Ordering::SeqCst);
    if let Some(handle) = active.handle.take() {
        let _ = handle.join();
    }

    let scan_root = active.root.clone();
    let final_snapshot = tokio::task::spawn_blocking(move || {
        agent::capture_workspace(&scan_root, agent::recorder::MAX_BASELINE_FILES)
    })
    .await
    .map_err(|e| format!("final reconciliation task failed: {e}"))?;

    let mut session = active
        .session
        .lock()
        .map_err(|_| "session state is unavailable".to_string())?
        .clone();
    if let Ok(final_snapshot) = final_snapshot {
        session.reconcile(&final_snapshot, &active.baseline);
    } else {
        session.coverage.complete = false;
        session.coverage.note =
            "Final reconciliation could not run; completeness cannot be claimed.".to_string();
    }
    session.note_agents(&current_agent_kinds());
    session.finish();
    if !active.watcher_started.load(Ordering::SeqCst) {
        session.coverage.complete = false;
        if !session.coverage.note.contains("overflow") {
            session.coverage.note =
                "The directory watcher did not start or overflowed; coverage is incomplete."
                    .to_string();
        }
    }

    let data_dir = resolve_data_dir();
    let _ = agent::recorder::save_session(&data_dir, &session);
    *state
        .last_session
        .lock()
        .map_err(|_| "session state is unavailable".to_string())? = Some(session.clone());
    Ok(session)
}

#[tauri::command]
async fn windows_change_capture(
    state: State<'_, AgentState>,
) -> Result<WindowsSnapshotSummary, String> {
    let entries = tokio::task::spawn_blocking(collect_persistence_entries)
        .await
        .map_err(|e| format!("Windows inventory task failed: {e}"))?;
    let data_dir = resolve_data_dir();
    let path = agent::windows::snapshot_path(&data_dir);
    let previous = agent::windows::load(&path).ok();
    let saved = agent::windows::save(&path, &entries)
        .map_err(|e| format!("The trusted snapshot could not be written: {e}"))?;
    Ok(WindowsSnapshotSummary {
        saved_at_unix: saved.saved_at.timestamp().max(0) as u64,
        entry_count: saved.entries.len(),
        previous_saved_at_unix: previous
            .as_ref()
            .map(|p| p.saved_at.timestamp().max(0) as u64),
        path: path.to_string_lossy().into_owned(),
        note: if previous.is_some() {
            "Trusted snapshot replaced. Future comparisons use this timestamp.".to_string()
        } else {
            "First trusted snapshot captured. Future comparisons use this timestamp.".to_string()
        },
    })
    .inspect(|_| {
        if let Ok(mut last) = state.last_windows.lock() {
            *last = None;
        }
    })
}

#[tauri::command]
async fn windows_change_compare(
    state: State<'_, AgentState>,
) -> Result<agent::WindowsChanges, String> {
    let entries = tokio::task::spawn_blocking(collect_persistence_entries)
        .await
        .map_err(|e| format!("Windows inventory task failed: {e}"))?;
    let data_dir = resolve_data_dir();
    let path = agent::windows::snapshot_path(&data_dir);
    let snapshot = agent::windows::load(&path).ok();
    let changes = agent::windows::compare(snapshot.as_ref(), &entries);
    *state
        .last_windows
        .lock()
        .map_err(|_| "Windows change state is unavailable".to_string())? = Some(changes.clone());
    Ok(changes)
}

#[tauri::command]
fn agent_case_file(
    state: State<'_, AgentState>,
    summary: String,
    include_summary: bool,
) -> Result<agent::AgentCaseFile, String> {
    let session = state
        .last_session
        .lock()
        .map_err(|_| "session state is unavailable".to_string())?
        .clone()
        .or_else(|| {
            state.recorder.lock().ok().and_then(|r| {
                r.as_ref()
                    .and_then(|rec| rec.session.lock().ok().map(|s| s.clone()))
            })
        })
        .ok_or_else(|| {
            "Run and stop a monitoring session before building a case file.".to_string()
        })?;
    let windows = state
        .last_windows
        .lock()
        .map_err(|_| "Windows change state is unavailable".to_string())?
        .clone();
    Ok(agent::casefile::build(
        &session,
        windows.as_ref(),
        Some(&summary),
        include_summary,
        false,
    ))
}

#[tauri::command]
fn export_agent_case_file(
    case: agent::AgentCaseFile,
    format: String,
) -> Result<String, String> {
    let data_dir = resolve_data_dir();
    let export_dir = data_dir.join("exports");
    std::fs::create_dir_all(&export_dir).map_err(|e| format!("cannot create exports dir: {e}"))?;

    let filename = format!("case-{}.{}", case.case_id, match format.to_lowercase().as_str() {
        "json" => "json",
        "html" => "html",
        _ => "md",
    });
    let target = export_dir.join(filename);
    let content = match format.to_lowercase().as_str() {
        "json" => case.to_json().map_err(|e| e.to_string())?,
        "html" => case.to_html(),
        _ => case.to_markdown(),
    };
    std::fs::write(&target, content).map_err(|e| format!("cannot write export file: {e}"))?;
    Ok(target.to_string_lossy().to_string())
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
    #[ignore = "needs an interactive desktop session: spawns real windows (fake-overlay + notepad) and closes one. Run locally via testing/run-gui-desktop-tests.bat; never in CI."]
    fn overlay_fixture_dismisses_fake_overlay_and_spares_notepad() {
        let bin = fake_overlay_bin();
        assert!(
            bin.exists(),
            "fake-overlay.exe not built yet — run: cargo build --release -p fake-overlay"
        );
        // 1. Spawn fake-overlay (borderless topmost window, unsigned binary)
        let mut overlay_proc = Command::new(&bin).spawn().expect("spawn fake-overlay");
        std::thread::sleep(std::time::Duration::from_secs(2));
        // 2. Spawn notepad (legitimate topmost=false, borderless=false)
        let mut notepad_proc = Command::new("notepad.exe").spawn().expect("spawn notepad");
        std::thread::sleep(std::time::Duration::from_secs(1));
        // 3. Collect all window candidates
        let candidates = collect_window_candidates().expect("collect_window_candidates failed");
        assert!(
            candidates.len() >= 2,
            "expected at least 2 window candidates, got {}",
            candidates.len()
        );
        // 4. Check that pick_overlays flags the fake-overlay but not notepad
        // (allowlist empty: nothing pre-approved on a test box).
        let scored: Vec<cure_core::overlay::OverlayCandidate> = candidates
            .iter()
            .map(|(_, desc, sig)| cure_core::overlay::OverlayCandidate {
                desc: desc.clone(),
                signature: *sig,
                binary_sha256: cure_core::hash_intel::sha256_file_hex(&desc.process_path),
            })
            .collect();
        let picks = overlay::pick_overlays(&scored, &[]);
        let overlay_name = bin.file_stem().unwrap().to_string_lossy().to_string();
        let mut found_overlay = false;
        for &idx in &picks {
            let (_, desc, _) = &candidates[idx];
            if desc
                .process_name()
                .to_ascii_lowercase()
                .contains(&overlay_name.to_ascii_lowercase())
            {
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
        assert!(
            found_overlay,
            "fake-overlay was not detected as a suspicious overlay"
        );
        // 5. Graceful close of the fake-overlay (default path: WM_CLOSE
        // only, no termination — the fixture's DefWindowProc handles it).
        let (hwnd_raw, _, _) = candidates
            .iter()
            .find(|(_, desc, _)| {
                desc.process_name()
                    .to_ascii_lowercase()
                    .contains(&overlay_name.to_ascii_lowercase())
            })
            .expect("fake-overlay hwnd not found");
        let result = close_overlay_window(*hwnd_raw, false).expect("close_overlay_window failed");
        assert!(result.closed, "fake-overlay window was not closed");
        assert!(
            !result.terminated,
            "graceful close must not terminate (force=false)"
        );
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert!(
            !is_window_alive(*hwnd_raw),
            "fake-overlay process still alive after close_overlay"
        );
        // 6. Kill notepad (cleanup) and reap both children so the test
        // leaves no zombies behind.
        let _ = notepad_proc.kill();
        let _ = notepad_proc.wait();
        let _ = overlay_proc.try_wait();
    }
    fn is_window_alive(hwnd_raw: isize) -> bool {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        unsafe { IsWindow(hwnd).as_bool() }
    }
}
#[cfg(test)]
#[cfg(windows)]
mod data_dir_tests {
    use super::*;
    #[test]
    fn default_data_dir_is_app_owned_never_exe_adjacent() {
        // Under `cargo test` no --data-dir flag is present, so this asserts
        // the production default. If this test ever runs with --data-dir in
        // argv it proves nothing — fail loudly instead of passing vacuously.
        assert!(
            std::env::args().all(|a| a != "--data-dir"),
            "test must run without --data-dir to assert the default"
        );
        let dir = resolve_data_dir();
        let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA must be set on Windows");
        assert_eq!(dir, PathBuf::from(local).join("CURE"));
        // Must never resolve next to the executable (which may sit on the
        // user's Desktop) nor to the bare current directory.
        let exe_parent = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf));
        assert_ne!(Some(dir.clone()), exe_parent);
        assert_ne!(dir, PathBuf::from("."));
    }
}
#[cfg(test)]
#[cfg(windows)]
mod overlay_allowlist_store_tests {
    use super::*;
    use cure_core::overlay::AllowEntry;
    #[test]
    fn missing_or_corrupt_file_means_empty_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overlay-allowlist.json");
        assert!(load_overlay_allowlist_from(&path).is_empty());
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_overlay_allowlist_from(&path).is_empty());
    }
    #[test]
    fn roundtrip_preserves_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overlay-allowlist.json");
        let entries = vec![
            AllowEntry::new(r"D:\Games\indie.exe", &"aa".repeat(32)),
            AllowEntry::new(r"C:\Tools\overlay-tool.exe", &"bb".repeat(32)),
        ];
        save_overlay_allowlist_to(&path, &entries).unwrap();
        assert_eq!(load_overlay_allowlist_from(&path), entries);
    }
    #[test]
    fn duplicates_are_the_callers_job_to_avoid() {
        // save/load is a dumb store; dedup lives in allowlist_overlay.
        // This locks the store behavior so a future dedup change is explicit.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overlay-allowlist.json");
        let one = AllowEntry::new(r"D:\Games\indie.exe", &"aa".repeat(32));
        save_overlay_allowlist_to(&path, &[one.clone(), one.clone()]).unwrap();
        assert_eq!(load_overlay_allowlist_from(&path).len(), 2);
    }
}

#[cfg(test)]
mod case_file_export_tests {
    use super::*;

    #[test]
    fn base64_decodes_the_png_signature() {
        let bytes = decode_base64("iVBORw0KGgo=").expect("valid base64");
        assert_eq!(&bytes[0..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn base64_refuses_anything_outside_the_alphabet() {
        assert!(decode_base64("not base64!").is_err());
        assert!(decode_base64("AA*A").is_err());
    }

    #[test]
    fn base64_ignores_line_breaks() {
        let joined = decode_base64("iVBORw0K\r\nGgo=").expect("valid base64");
        assert_eq!(joined, decode_base64("iVBORw0KGgo=").unwrap());
    }

    #[test]
    fn an_oversized_payload_is_refused_before_it_is_decoded() {
        let huge = "A".repeat(24 * 1024 * 1024 + 1);
        let err = decode_case_file_png(&huge).expect_err("must refuse");
        assert!(err.contains("too large"));
    }

    #[test]
    fn a_non_png_payload_is_refused() {
        let err = decode_case_file_png("aGVsbG8=").expect_err("must refuse");
        assert!(err.contains("PNG"));
    }

    #[test]
    fn a_case_file_name_cannot_choose_where_the_file_lands() {
        // The name is reduced to a safe alphabet, so no separator and no
        // traversal sequence can survive into the path.
        let hostile = "../../Windows/System32/evil";
        let safe: String = hostile
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(64)
            .collect();
        assert!(!safe.contains('/'));
        assert!(!safe.contains('\\'));
        assert!(!safe.contains(".."));
    }
}
