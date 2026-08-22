#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use cure_core::{baseline, quarantine, risk, scanners};
use cure_core::model::{PersistenceEntry, PersistenceSource, RiskLevel, ScoredEntry};

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

#[derive(Debug, Clone, Serialize)]
struct ItemScannedEvent {
    stage: String,
    name: String,
    source: String,
    location: String,
    risk: String,
    score: i32,
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
    let scored: Vec<ScoredEntry> = entries.iter().map(risk::score_entry).collect();

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

    emit_stage(&app, "done", "Saving baseline");
    baseline::save_baseline(&data_dir.join("baseline.json"), &entries)
        .map_err(|e| format!("cannot write baseline: {e}"))?;

    let total = high_risk_cleaned.len() + suspicious_for_review.len() + safe;
    Ok(ScanSummary {
        total,
        high_risk_cleaned,
        suspicious_for_review,
        safe,
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            run_auto_scan,
            quarantine_entry,
            undo_entry,
            open_quarantine_folder,
            view_log,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running cure-gui");
}
