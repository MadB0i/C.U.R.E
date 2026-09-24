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
use cure_core::canary::{CanaryAlert, shadow_wipe_reason};

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
    reasons: Vec<String>,
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

// Live process enumeration lives in cure_core::process_scan
// (shared with the CLI report) — pure scoring stays there too.

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
    cure_core::ransom_detect::read_dir_entries(folder)
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
    // Access states ride alongside the entries so coverage UI can render
    // CHECK FAILED / PARTIAL / skipped counts instead of a bare zero.
    // (area, state label, human detail).
    let mut source_states: Vec<cure_core::report::CoverageRow> = Vec::new();
    #[cfg(windows)]
    {
        let reg = scanners::registry::scan_report();
        source_states.push(registry_state_row(&reg));
        entries.extend(reg.entries);
    }
    #[cfg(not(windows))]
    source_states.push(cure_core::report::CoverageRow::unavailable(
        "Registry autoruns",
        "Windows-only source",
    ));

    emit_stage(&app, "startup", "Walking the per-user Startup folder");
    entries.extend(scanners::startup::scan(&startup_root()));

    emit_stage(&app, "tasks", "Parsing scheduled task definitions");
    let task_report = scanners::scheduled_tasks::scan_report(&tasks_root());
    source_states.push(tasks_state_row(&task_report));
    entries.extend(task_report.entries);

    emit_stage(&app, "services", "Enumerating auto-start services");
    let service_report = scanners::services::scan_report();
    source_states.push(services_state_row(&service_report));
    let service_records = service_report.records;
    entries.extend(service_records.iter().map(|r| r.entry.clone()));

    #[cfg(windows)]
    {
        emit_stage(&app, "wmi", "Querying WMI event subscriptions");
        let wmi_report = scanners::wmi::scan_report();
        source_states.push(wmi_state_row(&wmi_report));
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
                format!("Scoring auto-start services {}–{} of {}", done + 1, done + chunk.len(), total),
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
                        let missing = status
                            == cure_core::scanners::services::ImageStatus::Missing;
                        let signature = match &exe {
                            Some(path) => cure_core::signature::check_signature(path),
                            None => cure_core::signature::SignatureStatus::Unknown,
                        };
                        let hash = if risk::service_needs_hash(
                            &record.image_path,
                            exe.as_deref(),
                        ) {
                            exe.as_deref()
                                .and_then(cure_core::hash_intel::check_hash)
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
            RiskLevel::HighRisk | RiskLevel::Suspicious => {
                suspicious_for_review.push(s.clone())
            }
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
                RansomFinding {
                    finding_type: "ransom-note".to_string(),
                    path: note.path.to_string_lossy().to_string(),
                    detail: if snippet.is_empty() {
                        format!("Matched pattern: {}", note.matched_stem)
                    } else {
                        format!("Matched pattern: {} — \"{}\"", note.matched_stem, &snippet[..snippet.len().min(120)])
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

/// Coverage row for the scan summary. Non-OK states surface the scanner's
/// own reason so a failed check never renders as a clean zero.
/// Thin wrappers over `cure_core::report::source_state_row` (shared with
/// the CLI report so both surfaces stay consistent).
fn state_row(
    area: &str,
    status: &cure_core::elevation::SourceStatus,
    ok_detail: String,
) -> cure_core::report::CoverageRow {
    cure_core::report::source_state_row(area, status, ok_detail)
}

fn registry_state_row(reg: &scanners::registry::RegistryScanReport) -> cure_core::report::CoverageRow {
    state_row(
        "Registry autoruns",
        &reg.status(),
        format!("{} values", reg.values_read),
    )
}

fn tasks_state_row(task_report: &scanners::scheduled_tasks::TaskScanReport) -> cure_core::report::CoverageRow {
    state_row(
        "Scheduled tasks",
        &task_report.status(),
        format!("{} files", task_report.files_seen),
    )
}

fn services_state_row(service_report: &scanners::services::ServiceScanReport) -> cure_core::report::CoverageRow {
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

fn find_current_entry(id: &str) -> Option<PersistenceEntry> {    if let Some(entry) = scanners::collect_all(&startup_root(), &tasks_root())
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
    // Scoped restore (see cli cmd_undo): only under the scanned roots.
    let roots = vec![startup_root(), tasks_root()];
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
}

/// Everything currently in quarantine (newest last). Powers the Quarantine
/// view; undo actions go through `undo_entry` one record at a time.
#[tauri::command]
fn list_quarantine() -> Result<Vec<QuarantineRecordDto>, String> {    Ok(quarantine::list_records(&resolve_data_dir())
        .into_iter()
        .map(|r| QuarantineRecordDto {
            id: r.id,
            name: r.name,
            source: r.source,
            original_path: r.original_path.to_string_lossy().into_owned(),
            quarantine_path: r.quarantine_path.to_string_lossy().into_owned(),
            archived_at: r.archived_at.to_rfc3339(),
        })
        .collect())
}

/// Forensic details for one finding (shortcut resolution, task metadata,
/// publisher/signature) — strictly read-only enrichment for display.
/// The id must come from a fresh scan; unknown ids are rejected, so the
/// webview cannot turn this into an arbitrary-path inspection oracle.
#[tauri::command]
fn entry_details(id: String) -> Result<cure_core::entry_details::EntryDetails, String> {
    let Some(entry) = find_current_entry(&id) else {
        return Err(
            "entry is no longer present in the latest scan — rescan to refresh".to_string(),
        );
    };
    Ok(cure_core::entry_details::for_entry(&entry))
}

/// Reveal a finding's file in Explorer (selects it, opens nothing).
/// Strict id-based lookup like `entry_details`; registry/service/WMI and
/// other non-file locations are refused. Direct argv to explorer — no
/// shell parsing, so spaces and metacharacters in paths are inert.
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

/// Explicit security-report export (JSON/TXT, optional redaction).
/// Runs fresh scans itself so coverage states are honest; remediates
/// nothing. Returns the written file path for the footer message.
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
    coverage.push(CoverageRow::unavailable("Registry autoruns", "Windows-only source"));
    entries.extend(scanners::startup::scan(&startup_root()));
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
    coverage.push(CoverageRow::unavailable("WMI subscriptions", "Windows-only source"));

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
        scored.push(risk::score_service(record, missing, signature, hash.as_deref()));
    }

    let processes = process_scan::enumerate_processes();
    let mut scored_procs = Vec::new();
    for info in &processes {
        let sig = cure_core::signature::check_signature(std::path::Path::new(&info.exe_path));
        let hash = cure_core::hash_intel::check_hash(std::path::Path::new(&info.exe_path));
        scored_procs.push((info.clone(), process_scan::score_process(info, &sig, hash.as_deref())));
    }

    let folders: Vec<(PathBuf, Vec<cure_core::ransom_detect::DirEntry>)> =
        user_folder_candidates()
            .into_iter()
            .map(|f| (f.clone(), read_dir_entries(&f)))
            .collect();
    let ransom = cure_core::ransom_detect::scan_folders(&folders);

    let count = |source: PersistenceSource| {
        entries.iter().filter(|e| e.source == source).count()
    };
    // State rows (registry/tasks/services/WMI) are already in `coverage`
    // from the collection block above; append the static rows here.
    coverage.push(CoverageRow::checked("Startup folder", format!("{} entries", count(PersistenceSource::StartupFolder))));
    coverage.push(CoverageRow::checked("IFEO debuggers", format!("{} entries", count(PersistenceSource::IfeoDebugger))));
    coverage.push(CoverageRow::checked("AppInit DLLs", format!("{} entries", count(PersistenceSource::AppInitDlls))));
    coverage.push(CoverageRow::checked("COM hijacks (HKCU)", format!("{} entries", count(PersistenceSource::ComHijack))));
    #[cfg(windows)]
    {
        coverage.push(CoverageRow::checked("Registry autoruns", format!("{} entries", count(PersistenceSource::RegistryRun))));
        coverage.push(CoverageRow::checked("Running processes", format!("{} enumerated", processes.len())));
    }
    #[cfg(not(windows))]
    {
        coverage.push(CoverageRow::unavailable("Registry autoruns", "Windows-only source".to_string()));
        coverage.push(CoverageRow::unavailable("Running processes", "Windows-only source".to_string()));
    }
    if folders.is_empty() {
        coverage.push(CoverageRow::not_checked("Ransom indicators", "no user profile folders found".to_string()));
    } else {
        coverage.push(CoverageRow::checked("Ransom indicators", format!("{} folders, {} findings", folders.len(), ransom.len())));
    }
    coverage.push(CoverageRow::not_checked("Canary guard", "session feature — see the Canary Guard view".to_string()));
    {
        use cure_core::hash_intel::ThreatIntelProvider;
        coverage.push(CoverageRow::checked("Threat intel", cure_core::hash_intel::fixture_provider().provider_label().to_string()));
    }

    let report = cure_core::report::assemble(
        ScanInput { persistence: scored, processes: scored_procs, ransom, canary_note: "session feature".to_string() },
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
}

// ---------------------------------------------------------------------------
// Post-login incident investigation (observation only — see core::incident)
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
struct IncidentProgressEvent {
    stage: String,
    polls: usize,
    processes: usize,
    windows: usize,
}

/// Run a post-login incident observation: collect current persistence
/// findings, observe process/window activity for the validated duration,
/// correlate, and return the verdict. Observation installs nothing and
/// modifies nothing; durations are restricted to 15/30/60/120 seconds.
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
    let mut findings: Vec<incident::StartupRef> = scanners::collect_all(&startup_root(), &tasks_root())
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
        let verdict = incident::decide_verdict(&correlations, &live.windows, &live.process_observation);
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

/// Export an observation result as JSON/TXT incident report (explicit;
/// remediates nothing). The result is passed back in — the backend keeps
/// no observation state between commands.
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
        CoverageRow::checked("Startup folder", format!("{} entries", count(PersistenceSource::StartupFolder))),
        CoverageRow::checked("Scheduled tasks", format!("{} entries", count(PersistenceSource::ScheduledTask))),
        CoverageRow::checked("Services (auto-start)", format!("{} services", service_records.len())),
        CoverageRow::checked("Incident observation", format!("{} processes, {} windows in {} s", result.processes.len(), result.windows.len(), result.duration_secs)),
    ];
    let empty: Vec<(cure_core::process_scan::ProcessInfo, cure_core::process_scan::ProcessScore)> = Vec::new();
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
    let filename = format!("cure-incident-{}.{}", cure_core::report::utc_stamp(), format);
    let out_path = data_dir.join(&filename);
    std::fs::write(&out_path, body).map_err(|e| format!("cannot write report: {e}"))?;
    Ok(out_path.to_string_lossy().into_owned())
}

/// Confirm-before-kill hardening: a PID can be recycled between the scan
/// and the user's confirmation click. Refuse to kill when the PID no longer
/// belongs to the process the user actually approved.
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
    open_with_default_handler(&path)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Open a file with its registered handler via ShellExecuteW("open").
///
/// Deliberately NOT `cmd /C start ...`: cmd.exe re-parses its command line,
/// so a data dir containing shell metacharacters (`&`, `^`, …) would become
/// command injection. The path here is passed as structured data, never
/// through a shell — spaces and metacharacters are inert.
#[cfg(windows)]
fn open_with_default_handler(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{w, PCWSTR};

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
            list_quarantine,
            entry_details,
            reveal_location,
            export_report,
            start_incident_observation,
            export_incident_report,
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
            maybe_start_exit_driver(app.handle().clone());
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
                .map(|(_, desc, sig)| (desc.clone(), *sig))
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
                // Still closed by pick_overlays (`!= ValidSigned`): a window
                // whose signature cannot be revocation-checked is not trusted.
                SignatureStatus::ValidRevocationUnknown => "revocation-unverified",
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
    document.getElementById("start-rescue-btn").click();
    if (!await waitFor(() => document.getElementById("results-view") !== null)) {
      throw new Error("app DOM never became ready");
    }
    if (!await waitFor(() => !document.getElementById("results-view").classList.contains("hidden"))) {
      throw new Error("security results never appeared");
    }
    // Footer success paths: a scan just completed, so the quarantine folder
    // and baseline log both exist on disk for the real backend to open.
    const quarantineMsg = await clickFoot("btn-quarantine-folder");
    const logMsg = await clickFoot("btn-view-log");
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
    // V3: destructive actions share an explicit confirm dialog.
    if (!await waitFor(() => !document.getElementById("confirm-overlay").classList.contains("hidden"))) {
      throw new Error("cleanup confirm dialog never appeared");
    }
    document.getElementById("confirm-ok").click();
    if (!await waitFor(() => document.getElementById("cleanup-status").textContent.startsWith("Freed"), 60000)) {
      throw new Error("cleanup status never showed Freed");
    }
    // Snapshot now: leaving the cleanup view (quarantine check below) resets
    // the transient result line by design.
    const finalCleanupStatus = document.getElementById("cleanup-status").textContent;
    await wait(300);
    // Quarantine view vs the REAL backend: the sandbox startup root is seeded
    // with a HighRisk .bat, which run_auto_scan must have auto-quarantined.
    // Listing it and undoing it through the UI proves list_quarantine,
    // quarantine (auto path), and undo_entry end to end.
    document.getElementById("nav-quarantine").click();
    if (!await waitFor(() =>
      !document.getElementById("view-quarantine").classList.contains("hidden"))) {
      throw new Error("quarantine view never appeared");
    }
    if (!await waitFor(() =>
      document.querySelectorAll("#q-list .q-row").length >= 1, 30000)) {
      throw new Error("quarantine list never showed the auto-quarantined seed");
    }
    document.querySelector("#q-list .q-undo").click();
    if (!await waitFor(() =>
      document.querySelectorAll("#q-list .q-row").length === 0, 30000)) {
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
      ["nav-canary", "view-canary"],
    ];
    for (const [nav, view] of smokeViews) {
      document.getElementById(nav).click();
      if (!await waitFor(() =>
        !document.getElementById(view).classList.contains("hidden"))) {
        throw new Error(view + " never appeared");
      }
      await wait(200);
    }
    const posture = document.getElementById("ov-posture").textContent;
    const auditCards = document.querySelectorAll("#audit-list .audit-card").length;
    if (!posture) throw new Error("overview posture empty");
    if (auditCards < 1) throw new Error("audit view empty despite auto-quarantined seed");
    // Canary guard against the REAL backend: enable, expect ACTIVE. (Decoys
    // land in the sandboxed profile; the TRIGGERED path via real filesystem
    // events stays a manual/VM test — see TESTING.md §5.)
    document.getElementById("can-toggle").click();
    if (!await waitFor(() =>
      document.getElementById("can-state").textContent === "ACTIVE", 15000)) {
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
    if (!await waitFor(() =>
      !document.getElementById("results-view").classList.contains("hidden"))) {
      throw new Error("results view never re-appeared");
    }
    await wait(300);
    emit({
      ok: true,
      status: finalCleanupStatus,
      pill: document.getElementById("cleanup-status-text").textContent,
      downloadsTicked: boxes.length > 0,
      tossSeen: window.__cureTossSeen === true,
      failures: Array.from(document.querySelectorAll("#cleanup-failures li")).map((li) => li.textContent),
      footer: { preScanLogMsg, quarantineMsg, logMsg },
      quarantineVerified: true,
      viewsVerified: { posture, auditCards },
      canaryActive: true,
      incidentVerified: incidentSummary,
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

// Exit-button driver (test-only, inert unless CURE_E2E_EXIT is set).
// Clicks the real Exit footer button after load; the outer harness asserts
// the process actually terminates. Kept separate from the cleanup driver so
// each run has exactly one terminal action.
const E2E_EXIT_RUNNER_JS: &str = r##"(async () => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, t = 30000) => {
    for (let i = 0; i < t / 100; i++) { if (f()) return true; await wait(100); }
    return false;
  };
  if (!await waitFor(() => document.getElementById("btn-exit") !== null)) {
    return;
  }
  await wait(500);
  document.getElementById("btn-exit").click();
})();"##;

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
    let count = g.as_ref()
        .map(|g| g.alert_count.load(Ordering::Relaxed))
        .unwrap_or(0);
    Ok(serde_json::json!({ "active": active, "alert_count": count }))
}

// Directory watching lives in cure_dirwatch (shared with cure-watch, same
// engine, same Win32 acquisition loop); the GUI only adapts alerts into
// Tauri events here. The GUI keeps its own user_folder_candidates() because
// its ransom-scan path needs pre-filtered existing dirs — a different
// contract from the shared helper.

fn spawn_dir_watcher(
    dir: PathBuf,
    app: AppHandle,
    stop: Arc<AtomicBool>,
    _alert_count: Arc<AtomicUsize>,
) {
    std::thread::spawn(move || {
        cure_dirwatch::run_dir_guard(&dir, &stop, |alert| {
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
            .map(|(_, desc, sig)| (desc.clone(), *sig))
            .collect();
        let picks = overlay::pick_overlays(&scored);

        let overlay_name = bin.file_stem().unwrap().to_string_lossy().to_string();
        let mut found_overlay = false;
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
        let local =
            std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA must be set on Windows");
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
