use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clap::{Parser, Subcommand};

use cure_core::baseline;
use cure_core::cleanup as disk_cleanup;
use cure_core::model::{PersistenceEntry, PersistenceSource, RiskLevel, ScoredEntry};
use cure_core::quarantine;
use cure_core::risk;
use cure_core::scanners;

#[derive(Parser)]
#[command(
    name = "cure",
    version,
    about = "C.U.R.E - Clean USB Rescue Engine",
    long_about = "Portable persistence-malware scanner and safe remediation tool.\n\
                  Detects how Windows malware survives a reboot (Run keys, Startup folder,\n\
                  scheduled tasks), risk-scores each finding, and lets you quarantine\n\
                  (never delete) malicious entries. User data is never touched."
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "DIR",
        help = "Directory for baseline.json and quarantine/ (default: folder holding cure.exe)"
    )]
    data_dir: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "DIR",
        help = "Override the Startup folder root"
    )]
    startup_root: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "DIR",
        help = "Override the scheduled-tasks XML root"
    )]
    tasks_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Scan,
    Diff,
    Quarantine {
        id: String,
    },
    Undo {
        id: String,
    },
    /// Write a JSON/TXT security report (explicit export; nothing is remediated).
    Report {
        #[arg(long, default_value = "txt", help = "json or txt")]
        format: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, help = "redact home directory and username (default: on)")]
        redact: bool,
        #[arg(long, help = "include full unredacted paths (explicit opt-in)")]
        full_paths: bool,
    },
    /// Observe process/window activity, correlate with startup findings.
    Incident {
        #[arg(
            long,
            default_value_t = 30,
            help = "observation seconds: 15, 30, 60, or 120"
        )]
        duration: u64,
        #[arg(long, default_value = "txt", help = "json or txt")]
        format: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, help = "redact home directory and username (default: on)")]
        redact: bool,
        #[arg(long, help = "include full unredacted paths (explicit opt-in)")]
        full_paths: bool,
    },
    Cleanup {
        #[command(subcommand)]
        action: CleanupAction,
    },
}

#[derive(Subcommand)]
enum CleanupAction {
    /// Report reclaimable junk (temp, caches, recycle bin, Windows.old,
    /// old installers in Downloads). Deletes nothing.
    Scan,
    /// Delete scanned candidates after showing a breakdown you confirm.
    Run {
        #[arg(
            long,
            help = "also offer old .exe/.msi files in Downloads (extra explicit confirmation)"
        )]
        include_downloads: bool,
        #[arg(
            long,
            value_name = "DAYS",
            default_value_t = 30,
            help = "Downloads installers older than this many days"
        )]
        downloads_age_days: u32,
        #[arg(
            long,
            help = "run DISM component-store cleanup afterwards (elevated shell required)"
        )]
        dism: bool,
    },
}

struct ResolvedPaths {
    data_dir: PathBuf,
    startup_root: PathBuf,
    tasks_root: PathBuf,
}

fn resolve(cli: &Cli) -> ResolvedPaths {
    ResolvedPaths {
        data_dir: cli.data_dir.clone().unwrap_or_else(default_data_dir),
        startup_root: cli
            .startup_root
            .clone()
            .unwrap_or_else(scanners::startup::default_startup_root),
        tasks_root: cli
            .tasks_root
            .clone()
            .unwrap_or_else(scanners::scheduled_tasks::default_tasks_root),
    }
}

fn default_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(&cli) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let paths = resolve(cli);
    fs::create_dir_all(&paths.data_dir)?;
    match &cli.command {
        Command::Scan => cmd_scan(&paths),
        Command::Diff => cmd_diff(&paths),
        Command::Quarantine { id } => cmd_quarantine(&paths, id),
        Command::Undo { id } => cmd_undo(&paths, id),
        Command::Report {
            format,
            redact,
            full_paths,
        } => cmd_report(&paths, format, *redact && !*full_paths),
        Command::Incident {
            duration,
            format,
            redact,
            full_paths,
        } => cmd_incident(&paths, *duration, format, *redact && !*full_paths),
        Command::Cleanup { action } => match action {
            CleanupAction::Scan => cmd_cleanup_scan(),
            CleanupAction::Run {
                include_downloads,
                downloads_age_days,
                dism,
            } => cmd_cleanup_run(*include_downloads, *downloads_age_days, *dism),
        },
    }
}

/// Collection with access accounting. The failure-prone scanners run
/// through their `*_report` variants once, so skipped locations are
/// counted instead of silently dropped.
struct CollectOutput {
    entries: Vec<PersistenceEntry>,
    services: Vec<ServiceRecord>,
    tasks_skipped: usize,
    reg_skipped: usize,
    svc_skipped: usize,
    source_states: Vec<cure_core::report::CoverageRow>,
}

fn collect(paths: &ResolvedPaths) -> CollectOutput {
    use cure_core::report::source_state_row;
    let mut entries = scanners::startup::scan(&paths.startup_root);
    let mut source_states = Vec::new();
    let task_report = scanners::scheduled_tasks::scan_report(&paths.tasks_root);
    let tasks_skipped = task_report.skipped;
    source_states.push(source_state_row(
        "Scheduled tasks",
        &task_report.status(),
        format!("{} files", task_report.files_seen),
    ));
    entries.extend(task_report.entries);
    #[cfg(windows)]
    let (reg_entries, reg_skipped, reg_row) = {
        let rep = scanners::registry::scan_report();
        let row = source_state_row(
            "Registry autoruns",
            &rep.status(),
            format!("{} values", rep.values_read),
        );
        (rep.entries, rep.skipped_keys, row)
    };
    #[cfg(not(windows))]
    let (reg_entries, reg_skipped, reg_row) = (
        Vec::new(),
        0,
        cure_core::report::CoverageRow::unavailable("Registry autoruns", "Windows-only source"),
    );
    entries.extend(reg_entries);
    source_states.push(reg_row);
    let svc_report = scanners::services::scan_report();
    let svc_skipped = svc_report.skipped_config;
    source_states.push(source_state_row(
        "Services (auto-start)",
        &svc_report.status,
        format!("{} services", svc_report.records.len()),
    ));
    let services = svc_report.records;
    entries.extend(services.iter().map(|r| r.entry.clone()));
    #[cfg(windows)]
    {
        let wmi_report = scanners::wmi::scan_report();
        source_states.push(source_state_row(
            "WMI subscriptions",
            &wmi_report.status,
            format!("{} entries", wmi_report.entries.len()),
        ));
        entries.extend(wmi_report.entries);
        entries.extend(scanners::ifeo::scan());
        entries.extend(scanners::appinit::scan());
        entries.extend(scanners::com::scan());
    }
    #[cfg(not(windows))]
    source_states.push(cure_core::report::CoverageRow::unavailable(
        "WMI subscriptions",
        "Windows-only source",
    ));
    CollectOutput {
        entries,
        services,
        tasks_skipped,
        reg_skipped,
        svc_skipped,
        source_states,
    }
}

fn skipped_note(output: &CollectOutput) -> Option<String> {
    let mut parts = Vec::new();
    if output.tasks_skipped > 0 {
        parts.push(format!("{} task files", output.tasks_skipped));
    }
    if output.reg_skipped > 0 {
        parts.push(format!("{} registry keys", output.reg_skipped));
    }
    if output.svc_skipped > 0 {
        parts.push(format!("{} service configs", output.svc_skipped));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

use cure_core::scanners::services::{ImageStatus, ServiceRecord};

fn score_services(services: &[ServiceRecord]) -> Vec<ScoredEntry> {
    services
        .iter()
        .map(|record| {
            let status = scanners::services::image_status(&record.image_path);
            let exe = match &status {
                ImageStatus::Found(path) => Some(path.as_path()),
                _ => None,
            };
            let missing = status == ImageStatus::Missing;
            let signature = match exe {
                Some(path) => cure_core::signature::check_signature(path),
                None => cure_core::signature::SignatureStatus::Unknown,
            };
            let hash = if risk::service_needs_hash(&record.image_path, exe) {
                exe.and_then(cure_core::hash_intel::check_hash)
            } else {
                None
            };
            risk::score_service(record, missing, signature, hash.as_deref())
        })
        .collect()
}

fn score_all(entries: &[PersistenceEntry], services: &[ServiceRecord]) -> Vec<ScoredEntry> {
    let mut scored: Vec<ScoredEntry> = entries
        .iter()
        // Service entries are scored by score_services below (they need
        // start-type/account evidence); skipping them here avoids doubles.
        .filter(|e| e.source != PersistenceSource::WindowsService)
        .map(|e| {
            let exe_path = cure_core::signature::resolve_executable_path(&e.command);
            risk::score_entry(e, exe_path.as_deref())
        })
        .collect();
    scored.extend(score_services(services));
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.entry.name.cmp(&b.entry.name))
    });
    scored
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{cut}…")
}

fn print_report(scored: &[ScoredEntry], data_dir: &Path) {
    for s in scored {
        println!(
            "{}  {:>3}  {:<30} ({})  id={}",
            s.risk,
            s.score,
            truncate(&s.entry.name, 30),
            s.entry.source.tag(),
            s.entry.id
        );
        println!("             cmd: {}", s.entry.command);
        if s.entry.location != s.entry.command {
            println!("             loc: {}", s.entry.location);
        }
        if !s.attack.id.is_empty() {
            println!("             att&ck: {} ({})", s.attack.id, s.attack.name);
        }
        // Shortcut targets: resolve without executing (popup forensics).
        if s.entry.source == PersistenceSource::StartupFolder
            && s.entry.name.len() > 4
            && s.entry.name[s.entry.name.len() - 4..].eq_ignore_ascii_case(".lnk")
        {
            match cure_core::lnk::analyze(Path::new(&s.entry.location)) {
                Some(info) => {
                    let target = info.target.as_deref().unwrap_or("(unresolvable)");
                    let exists = info
                        .target
                        .as_deref()
                        .map(|t| Path::new(t).is_file())
                        .unwrap_or(false);
                    println!(
                        "             lnk→: {target} [{}]",
                        if exists { "exists" } else { "MISSING" }
                    );
                    if let Some(args) = info.arguments.as_deref().filter(|a| !a.is_empty()) {
                        println!("             args : {args}");
                    }
                }
                None => println!("             lnk→: (unparseable shortcut)"),
            }
        }
        for reason in &s.reasons {
            println!("             why: {reason}");
        }
        if quarantine::is_quarantined(data_dir, &s.entry.id) {
            println!(
                "             status: ALREADY IN QUARANTINE (cure undo {})",
                s.entry.id
            );
        }
    }
}

fn summarize(scored: &[ScoredEntry]) -> (usize, usize, usize) {
    let high = scored
        .iter()
        .filter(|s| s.risk == RiskLevel::HighRisk)
        .count();
    let susp = scored
        .iter()
        .filter(|s| s.risk == RiskLevel::Suspicious)
        .count();
    let safe = scored.len() - high - susp;
    (high, susp, safe)
}

fn cmd_scan(paths: &ResolvedPaths) -> Result<(), Box<dyn Error>> {
    use std::time::Instant;
    println!("C.U.R.E - Clean USB Rescue Engine");
    println!("startup root : {}", paths.startup_root.display());
    println!("tasks root   : {}", paths.tasks_root.display());
    if cfg!(windows) {
        println!("registry     : HKCU + HKLM Run / RunOnce + IFEO + AppInit + COM (HKCU) + WMI + services");
    } else {
        println!("registry     : unavailable on this OS (Windows-only source)");
    }
    println!();

    let t_collect = Instant::now();
    let collected = collect(paths);
    let collect_ms = t_collect.elapsed().as_millis();
    let t_score = Instant::now();
    let scored = score_all(&collected.entries, &collected.services);
    let score_ms = t_score.elapsed().as_millis();
    if scored.is_empty() {
        println!("no persistence entries found in the scanned locations.");
    } else {
        print_report(&scored, &paths.data_dir);
    }

    let (high, susp, safe) = summarize(&scored);
    let quarantined = quarantine::list_records(&paths.data_dir).len();
    println!();
    println!(
        "summary: {} entr{} | {high} high-risk, {susp} suspicious, {safe} safe | {quarantined} in quarantine",
        scored.len(),
        if scored.len() == 1 { "y" } else { "ies" }
    );
    if let Some(skipped) = skipped_note(&collected) {
        println!("skipped: {skipped} (inaccessible — elevate to compare)");
    }
    println!("timing: collect {collect_ms} ms, score {score_ms} ms");

    let baseline_path = paths.data_dir.join("baseline.json");
    baseline::save(&baseline_path, &collected.entries)?;
    println!("baseline saved: {}", baseline_path.display());
    println!("next: `cure diff`, then `cure quarantine <id>` / `cure undo <id>`");
    Ok(())
}

fn cmd_diff(paths: &ResolvedPaths) -> Result<(), Box<dyn Error>> {
    let baseline_path = paths.data_dir.join("baseline.json");
    let baseline = match baseline::load(&baseline_path) {
        Ok(baseline) => baseline,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!(
                "no baseline at {} - run `cure scan` first.",
                baseline_path.display()
            );
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

    let collected = collect(paths);
    let scored = score_all(&collected.entries, &collected.services);
    let new_entries = baseline::diff(&scored, &baseline);

    if new_entries.is_empty() {
        println!(
            "no NEW persistence entries since {}.",
            baseline.saved_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        return Ok(());
    }

    println!(
        "{} NEW persistence entr{} since {}:",
        new_entries.len(),
        if new_entries.len() == 1 { "y" } else { "ies" },
        baseline.saved_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!();
    print_report(&new_entries, &paths.data_dir);
    let (high, _, _) = summarize(&new_entries);
    if high > 0 {
        println!();
        println!("review carefully - new high-risk items appeared since the last scan.");
    }
    Ok(())
}

fn cmd_quarantine(paths: &ResolvedPaths, id: &str) -> Result<(), Box<dyn Error>> {
    let collected = collect(paths);
    let scored = score_all(&collected.entries, &collected.services);

    let Some(found) = scored.iter().find(|s| s.entry.id == id) else {
        if quarantine::is_quarantined(&paths.data_dir, id) {
            println!("id {id} is already in quarantine (restore with `cure undo {id}`).");
            return Ok(());
        }
        return Err(
            format!("unknown id {id}: run `cure scan` and pick an id from the report").into(),
        );
    };

    // Only file-backed findings are relocated; everything else is manual
    // guidance (C.U.R.E implements NO automatic service/registry/WMI/COM
    // remediation — detection first, reversible file moves only).
    if !found.entry.source.is_file_backed() {
        print_manual_guidance(found);
        return Ok(());
    }
    match found.entry.source {
        PersistenceSource::StartupFolder => {
            let record = quarantine::quarantine_entry(&paths.data_dir, &found.entry)?;
            println!("moved: {}", record.original_path.display());
            println!("to   : {}", record.quarantine_path.display());
            println!("restore anytime with: cure undo {}", record.id);
        }
        PersistenceSource::ScheduledTask => {
            let record = quarantine::quarantine_entry(&paths.data_dir, &found.entry)?;
            println!("moved task definition: {}", record.original_path.display());
            println!(
                "to                   : {}",
                record.quarantine_path.display()
            );
            println!("note: an already-running instance keeps running until reboot;");
            println!("      the task disappears from Task Scheduler after refresh.");
            println!("restore anytime with: cure undo {}", record.id);
        }
        _ => unreachable!("non-file-backed sources handled above"),
    }
    Ok(())
}

/// Manual, reversible remediation guidance for findings C.U.R.E will never
/// touch automatically. Every path starts with a backup step.
fn print_manual_guidance(found: &ScoredEntry) {
    println!(
        "{} findings are detected and scored but NEVER auto-disabled.",
        found.entry.source
    );
    println!("investigate first, back up, then remove manually:");
    match found.entry.source {
        PersistenceSource::RegistryRun => {
            println!("  key   : {}", found.entry.location);
            println!("  value : {}", found.entry.name);
            println!(
                "  1. backup: reg export \"{}\" backup.reg",
                found.entry.location
            );
            println!(
                "  2. remove: reg delete \"{}\" /v \"{}\" /f",
                found.entry.location, found.entry.name
            );
        }
        PersistenceSource::WindowsService => {
            println!(
                "  service : {} ({})",
                found.entry.name, found.entry.location
            );
            println!("  image   : {}", found.entry.command);
            println!("  1. inspect: sc.exe qc \"{}\"", found.entry.name);
            println!(
                "  2. backup : reg export \"{}\" backup-{}.reg",
                found.entry.location, found.entry.name
            );
            println!(
                "  3. disable (reversible): sc.exe config \"{}\" start= demand",
                found.entry.name
            );
            println!(
                "  4. delete only if malicious AND backed up: sc.exe delete \"{}\"",
                found.entry.name
            );
        }
        PersistenceSource::WmiSubscription => {
            println!("  object  : {}", found.entry.location);
            println!("  payload : {}", found.entry.command);
            println!("  1. inspect: Get-CimInstance -Namespace root/subscription -ClassName __EventFilter | Format-List Name,Query");
            println!("  2. backup : Get-CimInstance -Namespace root/subscription -Query \"SELECT * FROM __EventFilter WHERE Name='{}'\" > backup.txt", found.entry.name);
            println!("  3. remove (filter, consumer, binding): Remove-CimInstance (same query) — only after backup");
        }
        PersistenceSource::IfeoDebugger
        | PersistenceSource::AppInitDlls
        | PersistenceSource::ComHijack => {
            println!("  key     : {}", found.entry.location);
            println!("  value   : {}", found.entry.command);
            println!(
                "  1. backup: reg export \"{}\" backup.reg",
                found.entry.location
            );
            println!("  2. remove the value in regedit (or reg delete) only after backup");
        }
        PersistenceSource::StartupFolder | PersistenceSource::ScheduledTask => {
            println!(
                "  unexpected file-backed source reached manual guidance; use `cure quarantine {}`",
                found.entry.id
            );
        }
    }
}

fn cmd_undo(paths: &ResolvedPaths, id: &str) -> Result<(), Box<dyn Error>> {
    // Scoped restore: the file may only go back under the scanned
    // startup/task roots — a planted records.json must not redirect an
    // elevated undo into a system location.
    let roots = vec![paths.startup_root.clone(), paths.tasks_root.clone()];
    match quarantine::undo_scoped(&paths.data_dir, id, Some(&roots)) {
        Ok(record) => {
            println!("restored: {}", record.quarantine_path.display());
            println!("      to: {}", record.original_path.display());
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Err(format!("unknown id {id}: nothing was ever quarantined under that id").into())
        }
        Err(err) => Err(err.into()),
    }
}

// ---------------------------------------------------------------------------
// security report export (explicit; remediates nothing)
// ---------------------------------------------------------------------------

fn user_profile_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        let home = PathBuf::from(home);
        for sub in ["Desktop", "Documents", "Downloads"] {
            let candidate = home.join(sub);
            if candidate.is_dir() {
                folders.push(candidate);
            }
        }
    }
    folders
}

fn count_source(entries: &[PersistenceEntry], source: PersistenceSource) -> usize {
    entries.iter().filter(|e| e.source == source).count()
}

use cure_core::process_scan::{ProcessInfo, ProcessScore};
use cure_core::ransom_detect::RansomFinding;

/// Fresh evidence snapshot shared by `report` and `incident`.
/// Read-only throughout: scans score, snapshots describe, nothing executes.
struct EvidenceBundle {
    entries: Vec<PersistenceEntry>,
    services: Vec<ServiceRecord>,
    scored: Vec<ScoredEntry>,
    processes: Vec<(ProcessInfo, ProcessScore)>,
    ransom: Vec<RansomFinding>,
    profile_folders: Vec<PathBuf>,
    /// Access states for the failure-prone scanners (tasks/services/WMI/
    /// registry), so coverage rows stay truthful. Single-key scanners
    /// (startup/IFEO/AppInit/COM) report counts; see AUDIT.md for the
    /// per-scanner privilege analysis.
    source_states: Vec<cure_core::report::CoverageRow>,
}

fn collect_evidence(paths: &ResolvedPaths) -> EvidenceBundle {
    let collected = collect(paths);
    let scored = score_all(&collected.entries, &collected.services);

    let mut processes = Vec::new();
    #[cfg(windows)]
    {
        for info in cure_core::process_scan::enumerate_processes() {
            let sig = cure_core::signature::check_signature(Path::new(&info.exe_path));
            let hash = cure_core::hash_intel::check_hash(Path::new(&info.exe_path));
            let ps = cure_core::process_scan::score_process(&info, &sig, hash.as_deref());
            processes.push((info, ps));
        }
    }

    let profile_folders = user_profile_folders();
    let folder_entries: Vec<(PathBuf, Vec<cure_core::ransom_detect::DirEntry>)> = profile_folders
        .clone()
        .into_iter()
        .map(|f| (f.clone(), cure_core::ransom_detect::read_dir_entries(&f)))
        .collect();
    let ransom = if folder_entries.is_empty() {
        Vec::new()
    } else {
        cure_core::ransom_detect::scan_folders(&folder_entries)
    };

    EvidenceBundle {
        entries: collected.entries,
        services: collected.services,
        scored,
        processes,
        ransom,
        profile_folders,
        source_states: collected.source_states,
    }
}

fn cmd_report(paths: &ResolvedPaths, format: &str, redact: bool) -> Result<(), Box<dyn Error>> {
    use cure_core::hash_intel::ThreatIntelProvider;
    use cure_core::report::{CoverageRow, ReportOptions, ScanInput};

    let format = format.to_ascii_lowercase();
    if format != "json" && format != "txt" {
        return Err(format!("unknown format {format}: use json or txt").into());
    }
    println!("C.U.R.E security report — collecting evidence (nothing is remediated)…");

    let bundle = collect_evidence(paths);
    let EvidenceBundle {
        entries,
        services: _,
        scored,
        processes,
        ransom,
        profile_folders: folders,
        mut source_states,
    } = bundle;

    let on_windows = cfg!(windows);
    // State rows (registry/tasks/services/WMI) come from the evidence
    // bundle; the single-key scanners report counts below.
    let mut coverage = std::mem::take(&mut source_states);
    coverage.push(CoverageRow::checked(
        "Startup folder",
        format!(
            "{} entries",
            count_source(&entries, PersistenceSource::StartupFolder)
        ),
    ));
    coverage.push(CoverageRow::checked(
        "IFEO debuggers",
        format!(
            "{} entries",
            count_source(&entries, PersistenceSource::IfeoDebugger)
        ),
    ));
    coverage.push(CoverageRow::checked(
        "AppInit DLLs",
        format!(
            "{} entries",
            count_source(&entries, PersistenceSource::AppInitDlls)
        ),
    ));
    coverage.push(CoverageRow::checked(
        "COM hijacks (HKCU)",
        format!(
            "{} entries",
            count_source(&entries, PersistenceSource::ComHijack)
        ),
    ));
    if on_windows {
        coverage.push(CoverageRow::checked(
            "Running processes",
            format!("{} enumerated", processes.len()),
        ));
    } else {
        coverage.push(CoverageRow::unavailable(
            "Running processes",
            "Windows-only source",
        ));
    }
    if folders.is_empty() {
        coverage.push(CoverageRow::not_checked(
            "Ransom indicators",
            "no user profile folders found",
        ));
    } else {
        coverage.push(CoverageRow::checked(
            "Ransom indicators",
            format!("{} folders, {} findings", folders.len(), ransom.len()),
        ));
    }
    coverage.push(CoverageRow::not_checked(
        "Canary guard",
        "session feature — enable in the GUI or watcher",
    ));
    coverage.push(CoverageRow::checked(
        "Threat intel",
        cure_core::hash_intel::fixture_provider().provider_label(),
    ));

    let report = cure_core::report::assemble(
        ScanInput {
            persistence: scored,
            processes,
            ransom,
            canary_note: "session feature — enable in the GUI or watcher".to_string(),
        },
        coverage,
        &ReportOptions { redact },
    );
    let body = if format == "json" {
        cure_core::report::render_json(&report)
    } else {
        cure_core::report::render_txt(&report)
    };
    let stamp = cure_core::report::utc_stamp();
    let filename = format!("cure-report-{stamp}.{format}");
    let out_path = paths.data_dir.join(&filename);
    fs::write(&out_path, body)?;
    println!(
        "report written: {} ({} findings, {} coverage areas)",
        out_path.display(),
        report.findings.len(),
        report.coverage.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// post-login incident investigation (observation only)
// ---------------------------------------------------------------------------

fn cmd_incident(
    paths: &ResolvedPaths,
    duration_secs: u64,
    format: &str,
    redact: bool,
) -> Result<(), Box<dyn Error>> {
    use cure_core::incident;
    use cure_core::report::{CoverageRow, ReportOptions, ScanInput};

    let duration_secs = incident::validate_duration(duration_secs).map_err(|e| e.to_string())?;
    let format = format.to_ascii_lowercase();
    if format != "json" && format != "txt" {
        return Err(format!("unknown format {format}: use json or txt").into());
    }

    println!("C.U.R.E post-login incident investigation");
    println!("observation installs nothing and modifies nothing;");
    println!("it watches process/window activity for {duration_secs} s, then correlates.");
    println!("for true post-login capture, run this within a minute of logging in.");
    println!();

    println!("collecting current persistence findings (nothing is remediated)…");
    let bundle = collect_evidence(paths);
    let EvidenceBundle {
        entries,
        services,
        scored,
        mut source_states,
        ..
    } = bundle;
    let mut findings: Vec<incident::StartupRef> =
        entries.iter().map(incident::StartupRef::from).collect();
    // Attach hosting PIDs so shared service images (svchost) correlate by
    // pid equality instead of matching every instance with every service.
    for record in &services {
        if let Some(pid) = record.pid {
            if let Some(f) = findings.iter_mut().find(|f| f.id == record.entry.id) {
                f.aux_pid = Some(pid);
            }
        }
    }
    println!("{} startup findings loaded.", findings.len());

    println!("observing for {duration_secs} s — leave the machine alone (popups welcome)…");
    let started_at = std::time::SystemTime::now();
    let live = incident::observe_blocking(
        std::time::Duration::from_secs(duration_secs),
        |polls, procs, wins| {
            if polls % 10 == 1 {
                println!("  …{polls} polls, {procs} processes, {wins} windows tracked");
            }
        },
    );
    println!(
        "observed {} processes, {} windows{}.",
        live.processes.len(),
        live.windows.len(),
        if live.truncated {
            " (tracking caps hit — oldest dropped)"
        } else {
            ""
        }
    );

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
    let mut processes = live.processes;
    if !need_cmdline.is_empty() {
        println!(
            "reading command lines for {} correlated processes…",
            need_cmdline.len()
        );
        for (pid, cmd) in incident::fetch_command_lines(&need_cmdline) {
            if let Some(p) = processes.iter_mut().find(|p| p.pid == pid) {
                p.command_line = Some(cmd);
            }
        }
        correlations = incident::correlate(&findings, &processes);
    }

    let timeline = incident::build_timeline(
        started_at,
        duration_secs,
        &processes,
        &live.windows,
        &correlations,
    );
    let verdict = incident::decide_verdict(&correlations, &live.windows, &live.process_observation);

    println!();
    println!("TIMELINE");
    for event in &timeline {
        println!("  {} {}", event.wall_time, event.text);
    }
    println!();
    if correlations.is_empty() {
        println!("no process relates to a startup finding.");
    } else {
        println!("CORRELATIONS");
        for c in &correlations {
            println!(
                "  [{}] {} (pid {}) ↔ {} ({})",
                c.level.label(),
                c.process_name,
                c.process_pid,
                c.finding_name,
                c.finding_source
            );
            for e in &c.evidence {
                println!("    · {e}");
            }
        }
    }
    println!();
    println!("verdict: {}", verdict.label());
    println!("{}", verdict.explanation());
    println!("Correlation does not by itself establish malicious intent.");

    let result = incident::ObservationResult {
        investigation_id: incident::new_investigation_id(),
        started_at: incident::rfc3339(started_at),
        duration_secs,
        elevated: cure_core::elevation::is_elevated(),
        process_observation: live.process_observation,
        window_observation: live.window_observation,
        processes,
        windows: live.windows,
        correlations,
        timeline,
        verdict,
        truncated: live.truncated,
    };
    let mut report = cure_core::report::assemble(
        ScanInput {
            persistence: scored,
            processes: Vec::new(),
            ransom: Vec::new(),
            canary_note: "not assessed in this incident run".to_string(),
        },
        {
            let mut coverage = std::mem::take(&mut source_states);
            coverage.push(CoverageRow::checked(
                "Incident observation",
                format!(
                    "{} processes, {} windows in {duration_secs} s",
                    result.processes.len(),
                    result.windows.len()
                ),
            ));
            coverage.push(CoverageRow::not_checked(
                "Canary guard",
                "session feature — enable in the GUI or watcher".to_string(),
            ));
            coverage
        },
        &ReportOptions { redact },
    );
    report.incident = Some(cure_core::report::incident_section(&result, redact));
    let body = if format == "json" {
        cure_core::report::render_json(&report)
    } else {
        cure_core::report::render_txt(&report)
    };
    let stamp = cure_core::report::utc_stamp();
    let filename = format!("cure-incident-{stamp}.{format}");
    let out_path = paths.data_dir.join(&filename);
    fs::write(&out_path, body)?;
    println!();
    println!("incident report written: {}", out_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// disk cleanup
// ---------------------------------------------------------------------------

fn print_cleanup_row(label: &str, item_count: usize, bytes: u64) {
    println!(
        "{:<24} {:>4} item{} | {:>9}",
        label,
        item_count,
        if item_count == 1 { "" } else { "s" },
        disk_cleanup::format_size(bytes)
    );
}

fn cmd_cleanup_scan() -> Result<(), Box<dyn Error>> {
    println!("C.U.R.E disk cleanup - scan only, nothing is deleted");
    println!();

    let candidates = disk_cleanup::scan_all();
    let downloads = disk_cleanup::scan_old_downloads(30);
    let summary = disk_cleanup::summarize(&candidates);

    for row in &summary {
        match row.category {
            disk_cleanup::CleanupCategory::DownloadsInstaller => {}
            cat => print_cleanup_row(cat.label(), row.item_count, row.total_bytes),
        }
    }
    print_cleanup_row(
        "old installers (>30d)",
        downloads.len(),
        downloads.iter().map(|c| c.size_bytes).sum(),
    );

    let total: u64 = candidates.iter().map(|c| c.size_bytes).sum::<u64>()
        + downloads.iter().map(|c| c.size_bytes).sum::<u64>();
    println!("{:-<44}", "");
    println!(
        "{:<24} {:>4} item{} | {:>9}",
        "TOTAL reclaimable",
        candidates.len() + downloads.len(),
        if candidates.len() + downloads.len() == 1 {
            ""
        } else {
            "s"
        },
        disk_cleanup::format_size(total)
    );
    println!();
    println!("next: `cure cleanup run` (add --include-downloads / --dism for extras)");
    Ok(())
}

fn confirm(prompt: &str) -> bool {
    use std::io::Write as _;
    print!("{prompt} [y/N] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .is_ok_and(|_| matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

fn file_age_days(path: &Path) -> u64 {
    let Ok(md) = fs::metadata(path) else {
        return 0;
    };
    let age = md
        .modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .unwrap_or_default();
    age.as_secs() / 86_400
}

fn cmd_cleanup_run(
    include_downloads: bool,
    downloads_age_days: u32,
    dism: bool,
) -> Result<(), Box<dyn Error>> {
    let mut candidates = disk_cleanup::scan_all();

    println!("C.U.R.E disk cleanup - deletion plan");
    println!();
    for row in disk_cleanup::summarize(&candidates) {
        if row.item_count > 0 {
            print_cleanup_row(row.category.label(), row.item_count, row.total_bytes);
        }
    }
    let safe_total: u64 = candidates.iter().map(|c| c.size_bytes).sum();
    if candidates.is_empty() {
        println!("nothing reclaimable found.");
    } else if !confirm(&format!(
        "\nDelete {} item{} ({})? These are direct deletes - no quarantine.",
        candidates.len(),
        if candidates.len() == 1 { "" } else { "s" },
        disk_cleanup::format_size(safe_total)
    )) {
        println!("aborted - nothing was deleted.");
        return Ok(());
    }

    if include_downloads {
        let downloads = disk_cleanup::scan_old_downloads(downloads_age_days);
        if !downloads.is_empty() {
            println!();
            println!(
                "{} installer{} older than {downloads_age_days} day{} in Downloads:",
                downloads.len(),
                if downloads.len() == 1 { "" } else { "s" },
                if downloads_age_days == 1 { "" } else { "s" },
            );
            for candidate in &downloads {
                println!(
                    "  {:>9}  {:>4}d old  {}",
                    disk_cleanup::format_size(candidate.size_bytes),
                    file_age_days(&candidate.path),
                    candidate
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                );
            }
            if !confirm("\nAlso delete these installers? They may still be needed.") {
                println!("skipping Downloads installers.");
            } else {
                candidates.extend(downloads);
            }
        } else {
            println!("\nno installers older than {downloads_age_days} days in Downloads.");
        }
    }

    if candidates.is_empty() {
        println!("\nnothing selected - no deletions performed.");
        return Ok(());
    }

    println!();
    let result = disk_cleanup::delete_candidates(&candidates);
    println!(
        "deleted {} of {} item{}, freed {}.",
        result.deleted,
        result.attempted,
        if result.attempted == 1 { "" } else { "s" },
        disk_cleanup::format_size(result.bytes_freed)
    );
    if result.failed > 0 {
        println!("{} item(s) could not be deleted:", result.failed);
        for failure in &result.failures {
            println!("  {}: {}", failure.path.display(), failure.reason);
        }
    }

    if dism {
        println!();
        println!("running DISM component-store cleanup (can take several minutes)…");
        match disk_cleanup::run_dism_cleanup() {
            Ok(output) => {
                let tail: String = output
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                println!("{}", truncate_tail(&tail, 400));
            }
            Err(err) => {
                eprintln!("DISM failed: {err}");
            }
        }
    }
    Ok(())
}

fn truncate_tail(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text
        .chars()
        .skip(text.chars().count() - max_chars)
        .collect();
    format!("…{cut}")
}
