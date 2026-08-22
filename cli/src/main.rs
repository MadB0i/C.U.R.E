use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use cure_core::baseline;
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
