#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod canary;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod consent;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod detector;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod drives;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod logger;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod pairing;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod self_update;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod uninstall;

use consent::{ConsentDecision, CONSENT_FILE_NAME};

#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};

const POLL_INTERVAL_MS: u64 = 1500;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "pair") {
        std::process::exit(cmd_pair(&args));
    }
    if args.iter().any(|a| a == "--uninstall") {
        std::process::exit(cmd_uninstall(&args));
    }
    #[cfg(not(target_os = "windows"))]
    {
        println!("cure-watch performs its USB auto-launch duty on Windows only.");
        println!("Nothing useful to do on this platform; exiting.");
    }
    #[cfg(target_os = "windows")]
    {
        if let Err(err) = run() {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}

/// `cure-watch pair E:` — stamp this machine's pairing token onto a drive so
/// later insertions of that drive can trigger a launch. Exit 0 = paired,
/// 1 = operational error, 2 = usage error. Never launches anything.
#[cfg(target_os = "windows")]
fn cmd_pair(args: &[String]) -> i32 {
    let Some(root) = pairing::parse_pair_drive(args) else {
        eprintln!("usage: cure-watch pair <drive>   (example: cure-watch pair E:)");
        return 2;
    };
    if !root.is_dir() {
        eprintln!(
            "error: drive {} not found or not a directory",
            root.display()
        );
        return 1;
    }
    ensure_pairing();
    let Some(pair) = pairing::load_pairing() else {
        eprintln!("error: this watcher is not paired yet.");
        eprintln!(
            "Run cure-watch.exe once from your rescue media (answer Yes at the \
consent prompt) to bootstrap pairing, then stamp media with this command."
        );
        return 1;
    };
    match std::fs::write(
        root.join(pairing::TRIGGER_FILE_NAME),
        pairing::format_trigger(&pair.token_hex),
    ) {
        Ok(()) => {
            println!(
                "paired {} (token stamped; the host-installed copy remains the \
only binary ever launched)",
                root.display()
            );
            0
        }
        Err(err) => {
            eprintln!(
                "error: could not write trigger to {}: {err}",
                root.display()
            );
            1
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn cmd_pair(_args: &[String]) -> i32 {
    println!("`cure-watch pair` is a Windows-only operation.");
    2
}

/// `cure-watch --uninstall [--yes] [--dry-run]`: remove exactly what
/// self-install created (see `uninstall::managed_paths`), then verify.
/// Exit 0 = clean, 1 = leftovers/refused/failed, 2 = usage error.
/// Idempotent: absent items report as already-absent, never as errors.
#[cfg(target_os = "windows")]
fn cmd_uninstall(args: &[String]) -> i32 {
    let mut yes = false;
    let mut dry_run = false;
    for arg in args {
        match arg.as_str() {
            "--uninstall" => {}
            "--yes" => yes = true,
            "--dry-run" => dry_run = true,
            other => {
                eprintln!("unknown flag for --uninstall: {other}");
                eprintln!("usage: cure-watch --uninstall [--yes] [--dry-run]");
                return 2;
            }
        }
    }

    let plan = uninstall::removal_plan();
    println!("cure-watch uninstall: {} removal target(s):", plan.len());
    for target in &plan {
        println!("  {:?} {}", target.kind, target.path.display());
    }
    if dry_run {
        println!("dry run: nothing will be deleted.");
    } else if !yes {
        if !stdin_is_tty() {
            eprintln!(
                "refusing to uninstall without confirmation in a non-interactive \
session (re-run with --yes, or use --dry-run to preview)"
            );
            return 1;
        }
        if !ask_confirm("Remove the C.U.R.E watcher (files above)?") {
            println!("cancelled: nothing was removed.");
            return 0;
        }
    }

    let mut leftovers: Vec<String> = Vec::new();
    for target in &plan {
        for (path, outcome) in uninstall::execute_target(target, dry_run) {
            match &outcome {
                uninstall::RemovalOutcome::Removed => {
                    println!("removed: {}", path.display())
                }
                uninstall::RemovalOutcome::AlreadyAbsent
                | uninstall::RemovalOutcome::WouldKeepAbsent => {
                    println!("absent: {}", path.display())
                }
                uninstall::RemovalOutcome::WouldRemove => {
                    println!("would remove: {}", path.display())
                }
                uninstall::RemovalOutcome::RefusedReparsePoint => {
                    let msg = format!("reparse point left for manual review: {}", path.display());
                    println!("{msg}");
                    leftovers.push(msg);
                }
                uninstall::RemovalOutcome::RefusedUnknownName => {
                    let msg = format!("refused (not a managed name): {}", path.display());
                    println!("{msg}");
                    leftovers.push(msg);
                }
                uninstall::RemovalOutcome::Failed(err) => {
                    let msg = format!("failed {}: {err}", path.display());
                    println!("{msg}");
                    leftovers.push(msg);
                }
            }
        }
    }

    if dry_run {
        println!("dry run complete: nothing was changed.");
        return 0;
    }
    // Verification pass: re-check every file/dir target is gone.
    for target in &plan {
        match target.kind {
            uninstall::RemovalKind::File | uninstall::RemovalKind::DirIfEmpty => {
                if !uninstall::verify_gone(&target.path, &target.kind) {
                    leftovers.push(format!("still present: {}", target.path.display()));
                }
            }
            // DecoySweep targets verify via the marker re-scan below.
            uninstall::RemovalKind::DecoySweep => {}
        }
    }
    // Re-scan decoy dirs directly for anything still matching the marker.
    for target in plan
        .iter()
        .filter(|t| matches!(t.kind, uninstall::RemovalKind::DecoySweep))
    {
        if let Ok(entries) = std::fs::read_dir(&target.path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if cure_core::canary::is_canary_decoy(&name) {
                    leftovers.push(format!("decoy still present: {}", entry.path().display()));
                }
            }
        }
    }

    if leftovers.is_empty() {
        println!("clean — every cure-watch artifact verified gone.");
        0
    } else {
        println!("leftovers — {} item(s) need attention:", leftovers.len());
        for item in &leftovers {
            println!("  - {item}");
        }
        1
    }
}

#[cfg(not(target_os = "windows"))]
fn cmd_uninstall(_args: &[String]) -> i32 {
    println!("`cure-watch --uninstall` is a Windows-only operation.");
    2
}

/// True when stdin is an interactive terminal (std only, no extra dep).
#[cfg(target_os = "windows")]
fn stdin_is_tty() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal()
}

/// Strict `[y/N]` prompt: only an explicit `y`/`yes` counts as consent.
#[cfg(target_os = "windows")]
fn ask_confirm(prompt: &str) -> bool {
    use std::io::Write as _;
    print!("{prompt} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .is_ok_and(|_| matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

#[cfg(target_os = "windows")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    match consent::decide_consent(read_consent_marker().as_deref()) {
        ConsentDecision::SkipDeclined => {
            logger::log("consent", "previously declined; exiting without watching");
            println!(
                "background watching is declined on this machine ({}). \
Delete that file, then run cure-watch.exe again to be asked once more.",
                CONSENT_FILE_NAME
            );
            return Ok(());
        }
        ConsentDecision::ProceedEnabled => {
            logger::log("consent", "previously enabled; proceeding");
            start_watching()?;
        }
        ConsentDecision::AskNow => {
            logger::log("consent", "first run: asking for consent");
            if prompt_enable() {
                logger::log("consent", "user ENABLED background watching");
                write_consent_marker(true);
                start_watching()?;
            } else {
                logger::log("consent", "user DECLINED background watching");
                write_consent_marker(false);
                println!(
                    "declined — nothing was installed and the watcher is not running. \
Delete {} and re-run to be asked again.",
                    CONSENT_FILE_NAME
                );
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn read_consent_marker() -> Option<String> {
    std::fs::read_to_string(consent::marker_path()?).ok()
}

#[cfg(target_os = "windows")]
fn write_consent_marker(enabled: bool) {
    if let Some(path) = consent::marker_path() {
        if let Err(err) = std::fs::write(&path, consent::marker_body(enabled)) {
            logger::log("consent", &format!("failed to write marker: {err}"));
        }
    }
}

#[cfg(target_os = "windows")]
fn prompt_enable() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    };

    const TEXT: &str = "C.U.R.E Watcher wants to run quietly in the background:\n\n\
  \u{2022} It watches for newly inserted USB drives.\n\
  \u{2022} When a drive carrying a trigger paired with THIS machine is \
detected, it launches the rescue GUI installed on this machine \
(%LOCALAPPDATA%\\CURE\\cure-gui.exe) — never any program from the drive \
itself. Drives are paired with `cure-watch pair E:` (or, at install, the \
removable media you start the watcher from).\n\
  \u{2022} At install, the GUI copy sitting next to the watcher (your rescue \
media, which you deliberately executed) is pinned for later launches; USB \
executables are never run.\n\
  \u{2022} It also runs the experimental canary guard: it plants small decoy \
files (~cure-canary-*) in Desktop/Documents/Downloads, watches those folders \
for mass-encryption behaviour, and polls running processes for known \
shadow-copy-wiping tools. Alerts go to the watcher log; nothing is \
quarantined or killed automatically.\n\
  \u{2022} Nothing is scanned, launched or changed until such a trigger drive \
is inserted (except the decoy files above, once enabled).\n\n\
Enable background watching? (Yes = enable and start on login; No = decline, \
nothing gets installed)";

    const CAPTION: &str = "C.U.R.E — background rescue watcher";

    let text: Vec<u16> = TEXT.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = CAPTION.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    result == IDYES
}

#[cfg(target_os = "windows")]
fn start_watching() -> Result<(), Box<dyn std::error::Error>> {
    self_install()?;
    // Pin the host GUI copy + pairing token (best effort: watching and the
    // canary guard work regardless; launches stay disabled until paired).
    ensure_pairing();
    logger::log(
        "startup",
        &format!(
            "watcher started (pid {}, polling every {} ms)",
            std::process::id(),
            POLL_INTERVAL_MS
        ),
    );
    println!("cure-watch is watching for rescue USBs (Ctrl+C to stop)...");

    // Start canary guard: plant decoys + watch user folders + poll tripwire
    let _canary_stop = canary::start();

    let mut previous = drives::list_drives();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        let current = drives::list_drives();
        for drive in detector::newly_arrived(&previous, &current) {
            println!("drive appeared: {drive}");
            logger::log("drive", &format!("new drive appeared: {drive}"));
            let root = std::path::PathBuf::from(&drive);
            launch_gui(&root);
        }
        previous = current;
    }
}

#[cfg(target_os = "windows")]
fn self_install() -> Result<(), Box<dyn std::error::Error>> {
    use self_update::InstallDecision;

    let Some(startup) = self_update::startup_dir() else {
        println!("APPDATA not set; skipping self-install (portable mode)");
        logger::log(
            "install",
            "APPDATA not set; skipped self-install (portable mode)",
        );
        return Ok(());
    };
    std::fs::create_dir_all(&startup)?;
    let dest = startup.join(self_update::WATCHER_EXE_NAME);
    let exe = std::env::current_exe()?;
    if self_update::is_running_from(&exe, &dest) {
        // Already running from the installed copy — nothing to install.
        return Ok(());
    }
    // Compare bytes so a stale copy from an older release gets refreshed,
    // while an identical copy is never needlessly rewritten.
    let current = std::fs::read(&exe)?;
    let installed = std::fs::read(&dest).ok();
    match self_update::decide(installed.as_deref(), &current) {
        InstallDecision::FreshInstall => {
            std::fs::copy(&exe, &dest)?;
            println!("installed watcher to {}", dest.display());
            logger::log(
                "install",
                &format!("installed watcher to {}", dest.display()),
            );
        }
        InstallDecision::UpToDate => {
            logger::log(
                "install",
                &format!("installed copy already up to date at {}", dest.display()),
            );
        }
        InstallDecision::UpdateAvailable => {
            match self_update::replace(&dest, &exe) {
                Ok(()) => {
                    println!("updated installed watcher at {}", dest.display());
                    logger::log(
                        "install",
                        &format!("updated stale watcher at {}", dest.display()),
                    );
                }
                Err(err) => {
                    // Never destructive: the old copy stays in place and this
                    // process keeps watching with its own (newer) code.
                    // Typical cause: another watcher instance running from it.
                    logger::log(
                        "install",
                        &format!(
                            "deferred update of {} ({err}); old copy left intact",
                            dest.display()
                        ),
                    );
                }
            }
        }
    }
    Ok(())
}

/// True when the host GUI copy at `host_exe` matches `pair` on every axis:
/// plain file, not a reparse point, SHA-256 pinned hash intact.
#[cfg(target_os = "windows")]
fn host_copy_verified(host_exe: &Path, pair: &pairing::Pairing) -> bool {
    host_exe.is_file()
        && !pairing::is_reparse_point(host_exe)
        && cure_core::hash_intel::sha256_file_hex(host_exe).as_deref()
            == Some(pair.gui_sha256_hex.as_str())
}

/// The GUI copy next to the running watcher, but ONLY when it sits on
/// removable media (the operator's rescue USB being deliberately executed).
/// A copy next to a fixed-drive install (Startup, Downloads, …) is never a
/// pairing source: trusting it would let anything able to drop a file next
/// to the watcher re-pin the launched binary.
#[cfg(target_os = "windows")]
fn removable_beside_gui() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let root = pairing::drive_root_of(&exe)?;
    if !pairing::is_removable_drive(&root) {
        return None;
    }
    let gui = dir.join(pairing::HOST_GUI_EXE_NAME);
    if gui.is_file() && !pairing::is_reparse_point(&gui) {
        Some((root, gui))
    } else {
        None
    }
}

/// Bootstrap or repair the host pairing (best effort, never fatal).
///
/// - Already paired and host copy verifies: nothing to do.
/// - Otherwise the operator must be running from removable rescue media: its
///   GUI copy is pinned to `%LOCALAPPDATA%\CURE\` (verified by re-hashing
///   after the copy) and the media is stamped with the token. An existing
///   token is KEPT across GUI updates so already-paired USBs keep working;
///   only a missing pairing mints a fresh token.
/// - Anything else (fixed-drive launch, no media, IO failure): log and carry
///   on watching — launches simply stay disabled until pairing completes.
#[cfg(target_os = "windows")]
fn ensure_pairing() {
    use pairing::Pairing;

    let Some(host_exe) = pairing::host_gui_exe() else {
        logger::log(
            "pairing",
            "LOCALAPPDATA unset; pairing unavailable (portable mode, launches disabled)",
        );
        return;
    };
    if let Some(pair) = pairing::load_pairing() {
        if host_copy_verified(&host_exe, &pair) {
            return;
        }
        logger::log(
            "pairing",
            "pairing record present but host copy does not verify; attempting re-pin",
        );
    }
    let Some((media_root, source)) = removable_beside_gui() else {
        logger::log(
            "pairing",
            "no removable rescue media with a GUI copy alongside; launches disabled \
until pairing completes (run once from rescue media, or `cure-watch pair E:`)",
        );
        return;
    };
    let Some(hash) = cure_core::hash_intel::sha256_file_hex(&source) else {
        logger::log(
            "pairing",
            &format!("cannot hash media GUI copy at {}", source.display()),
        );
        return;
    };
    if std::fs::copy(&source, &host_exe).is_err() {
        logger::log(
            "pairing",
            &format!("cannot stage host GUI copy at {}", host_exe.display()),
        );
        return;
    }
    // Confirm the staged bytes before pinning their hash — a short write or a
    // swap between copy and pin must never become the trusted record.
    if cure_core::hash_intel::sha256_file_hex(&host_exe).as_deref() != Some(hash.as_str()) {
        logger::log("pairing", "staged host copy failed verification; removed");
        let _ = std::fs::remove_file(&host_exe);
        return;
    }
    let token_hex = pairing::load_pairing().map_or_else(
        || {
            pairing::generate_token_hex().unwrap_or_else(|err| {
                logger::log("pairing", &format!("CSPRNG failure: {err}"));
                String::new()
            })
        },
        |existing| existing.token_hex,
    );
    if token_hex.is_empty() {
        let _ = std::fs::remove_file(&host_exe);
        return;
    }
    let record = Pairing {
        token_hex: token_hex.clone(),
        gui_sha256_hex: hash,
    };
    if pairing::save_pairing(&record).is_err() {
        logger::log("pairing", "cannot persist pairing record");
        let _ = std::fs::remove_file(&host_exe);
        return;
    }
    logger::log(
        "pairing",
        &format!("host GUI copy pinned at {}", host_exe.display()),
    );
    match std::fs::write(
        media_root.join(pairing::TRIGGER_FILE_NAME),
        pairing::format_trigger(&token_hex),
    ) {
        Ok(()) => logger::log(
            "pairing",
            &format!("paired removable media at {}", media_root.display()),
        ),
        Err(err) => logger::log(
            "pairing",
            &format!(
                "pinned host copy but could not stamp trigger on {}: {err}",
                media_root.display()
            ),
        ),
    }
}

/// React to one newly arrived drive. Launches ONLY the pinned host copy, and
/// only when the drive's trigger carries this machine's token. Every Ignore
/// path is silent on the console (no feedback to a probing device) and
/// recorded in the local log; only setup states (unpaired watcher, broken
/// host copy) print, because the operator must fix those.
#[cfg(target_os = "windows")]
fn launch_gui(drive_root: &std::path::Path) {
    let Some(host_exe) = pairing::host_gui_exe() else {
        println!("watcher is not paired (LOCALAPPDATA unavailable); ignoring drive");
        logger::log(
            "trigger",
            "arrival ignored: LOCALAPPDATA unavailable, no pairing possible",
        );
        return;
    };
    let Some(pair) = pairing::load_pairing() else {
        println!("watcher is not paired yet; ignoring drive (run once from rescue media)");
        logger::log(
            "trigger",
            "arrival ignored: no pairing record (bootstrap by running once from rescue media)",
        );
        return;
    };
    let trigger = pairing::read_trigger_bytes(drive_root);
    let host = pairing::HostState {
        exists: host_exe.is_file(),
        sha256_matches_pin: cure_core::hash_intel::sha256_file_hex(&host_exe).as_deref()
            == Some(pair.gui_sha256_hex.as_str()),
        is_reparse_point: pairing::is_reparse_point(&host_exe),
    };
    match pairing::decide_launch(trigger.as_deref(), &pair.token_hex, &host_exe, &host) {
        pairing::LaunchDecision::Launch { host_exe } => {
            // Absolute path, argument list, no shell: this resolves to
            // CreateProcessW on the pinned copy — never a drive-supplied path.
            match std::process::Command::new(&host_exe)
                .arg("--data-dir")
                .arg(drive_root)
                .spawn()
            {
                Ok(_) => {
                    println!("launched pinned GUI for {}", drive_root.display());
                    logger::log(
                        "launch",
                        &format!(
                            "launched pinned {} for {}",
                            host_exe.display(),
                            drive_root.display()
                        ),
                    );
                }
                Err(err) => {
                    println!("failed to launch pinned GUI: {err}");
                    logger::log(
                        "launch-error",
                        &format!("failed to launch pinned GUI: {err}"),
                    );
                }
            }
        }
        pairing::LaunchDecision::Ignore { reason } => {
            logger::log(
                "trigger",
                &format!("drive {} ignored: {reason}", drive_root.display()),
            );
        }
    }
}
