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

    #[arg(long, global = true, value_name = "DIR", help = "Override the Startup folder root")]
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
    Quarantine { id: String },
    Undo { id: String },
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
        #[arg(long, help = "also offer old .exe/.msi files in Downloads (extra explicit confirmation)")]
        include_downloads: bool,
        #[arg(long, value_name = "DAYS", default_value_t = 30, help = "Downloads installers older than this many days")]
        downloads_age_days: u32,
        #[arg(long, help = "run DISM component-store cleanup afterwards (elevated shell required)")]
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

fn collect(paths: &ResolvedPaths) -> Vec<PersistenceEntry> {
    scanners::collect_all(&paths.startup_root, &paths.tasks_root)
}

fn score_all(entries: &[PersistenceEntry]) -> Vec<ScoredEntry> {
    let mut scored: Vec<ScoredEntry> = entries
        .iter()
        .map(|e| {
            let exe_path = cure_core::signature::resolve_executable_path(&e.command);
            risk::score_entry(e, exe_path.as_deref())
        })
        .collect();
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
        for reason in &s.reasons {
            println!("             why: {reason}");
        }
        if quarantine::is_quarantined(data_dir, &s.entry.id) {
            println!("             status: ALREADY IN QUARANTINE (cure undo {})", s.entry.id);
        }
    }
}

fn summarize(scored: &[ScoredEntry]) -> (usize, usize, usize) {
    let high = scored.iter().filter(|s| s.risk == RiskLevel::HighRisk).count();
    let susp = scored.iter().filter(|s| s.risk == RiskLevel::Suspicious).count();
    let safe = scored.len() - high - susp;
    (high, susp, safe)
}

fn cmd_scan(paths: &ResolvedPaths) -> Result<(), Box<dyn Error>> {
    println!("C.U.R.E - Clean USB Rescue Engine");
    println!("startup root : {}", paths.startup_root.display());
    println!("tasks root   : {}", paths.tasks_root.display());
    if cfg!(windows) {
        println!("registry     : HKCU + HKLM Run / RunOnce");
    } else {
        println!("registry     : unavailable on this OS (Windows-only source)");
    }
    println!();

    let entries = collect(paths);
    let scored = score_all(&entries);
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

    let baseline_path = paths.data_dir.join("baseline.json");
    baseline::save(&baseline_path, &entries)?;
    println!("baseline saved: {}", baseline_path.display());
    println!("next: `cure diff`, then `cure quarantine <id>` / `cure undo <id>`");
    Ok(())
}

fn cmd_diff(paths: &ResolvedPaths) -> Result<(), Box<dyn Error>> {
    let baseline_path = paths.data_dir.join("baseline.json");
    let baseline = match baseline::load(&baseline_path) {
        Ok(baseline) => baseline,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("no baseline at {} - run `cure scan` first.", baseline_path.display());
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

    let entries = collect(paths);
    let scored = score_all(&entries);
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
    let entries = collect(paths);
    let scored = score_all(&entries);

    let Some(found) = scored.iter().find(|s| s.entry.id == id) else {
        if quarantine::is_quarantined(&paths.data_dir, id) {
            println!("id {id} is already in quarantine (restore with `cure undo {id}`).");
            return Ok(());
        }
        return Err(format!(
            "unknown id {id}: run `cure scan` and pick an id from the report"
        )
        .into());
    };

    match found.entry.source {
        PersistenceSource::RegistryRun => {
            println!("registry autoruns are detected and scored but NOT auto-disabled in this MVP.");
            println!("remove manually:");
            println!("  key   : {}", found.entry.location);
            println!("  value : {}", found.entry.name);
            println!(
                "  e.g.  : reg delete \"{}\" /v \"{}\" /f",
                found.entry.location, found.entry.name
            );
            println!("back up first with: reg export \"{}\" backup.reg", found.entry.location);
        }
        PersistenceSource::StartupFolder => {
            let record = quarantine::quarantine_entry(&paths.data_dir, &found.entry)?;
            println!("moved: {}", record.original_path.display());
            println!("to   : {}", record.quarantine_path.display());
            println!("restore anytime with: cure undo {}", record.id);
        }
        PersistenceSource::ScheduledTask => {
            let record = quarantine::quarantine_entry(&paths.data_dir, &found.entry)?;
            println!("moved task definition: {}", record.original_path.display());
            println!("to                   : {}", record.quarantine_path.display());
            println!("note: an already-running instance keeps running until reboot;");
            println!("      the task disappears from Task Scheduler after refresh.");
            println!("restore anytime with: cure undo {}", record.id);
        }
    }
    Ok(())
}

fn cmd_undo(paths: &ResolvedPaths, id: &str) -> Result<(), Box<dyn Error>> {
    match quarantine::undo(&paths.data_dir, id) {
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
        if candidates.len() + downloads.len() == 1 { "" } else { "s" },
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

fn cmd_cleanup_run(include_downloads: bool, downloads_age_days: u32, dism: bool) -> Result<(), Box<dyn Error>> {
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
                    candidate.path.file_name().unwrap_or_default().to_string_lossy(),
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
                let tail: String = output.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n");
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
    let cut: String = text.chars().skip(text.chars().count() - max_chars).collect();
    format!("…{cut}")
}
