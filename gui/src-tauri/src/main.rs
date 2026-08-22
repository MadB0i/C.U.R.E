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

fn blocking_scan(app: AppHandle) -> Result<ScanSummary, String> {
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

    emit_stage(
        &app,
        "scoring",
        format!(
            "Risk-scoring {} persistence entr{}",
            entries.len(),
            if entries.len() == 1 { "y" } else { "ies" }
        ),
    );
    let scored: Vec<ScoredEntry> = entries.iter().map(risk::score_entry).collect();

    emit_stage(&app, "cleaning", "Auto-cleaning high-risk file-backed persistence");
    let mut high_risk_cleaned = Vec::new();
    let mut suspicious_for_review = Vec::new();
    let mut safe = 0usize;
    for s in scored {
        match s.risk {
            RiskLevel::HighRisk => match s.entry.source {
                PersistenceSource::RegistryRun => suspicious_for_review.push(s),
                _ => match quarantine::quarantine_file(&data_dir, &s.entry) {
                    Ok(_) => high_risk_cleaned.push(s),
                    Err(err) => {
                        let mut flagged = s;
                        flagged.reasons.push(format!("auto-clean failed: {err}"));
                        suspicious_for_review.push(flagged);
                    }
                },
            },
            RiskLevel::Suspicious => suspicious_for_review.push(s),
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

#[tauri::command]
async fn run_auto_scan(app: AppHandle) -> Result<ScanSummary, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || blocking_scan(handle))
        .await
        .map_err(|e| format!("scan task failed: {e}"))?
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            run_auto_scan,
            quarantine_entry,
            undo_entry
        ])
        .run(tauri::generate_context!())
        .expect("error while running cure-gui");
}
