#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod consent;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod detector;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod drives;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod logger;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod trigger;

use consent::{ConsentDecision, CONSENT_FILE_NAME};

const POLL_INTERVAL_MS: u64 = 1500;
const WATCHER_EXE_NAME: &str = "cure-watch.exe";
const GUI_EXE_NAME: &str = "cure-gui.exe";

fn main() {
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
  \u{2022} When a drive carrying a valid C.U.R.E trigger file is detected, it \
auto-launches a one-click rescue scan from that drive.\n\
  \u{2022} Nothing is scanned, launched or changed until such a trigger drive \
is inserted.\n\n\
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
    logger::log(
        "startup",
        &format!(
            "watcher started (pid {}, polling every {} ms)",
            std::process::id(),
            POLL_INTERVAL_MS
        ),
    );
    println!("cure-watch is watching for rescue USBs (Ctrl+C to stop)...");

    let mut previous = drives::list_drives();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        let current = drives::list_drives();
        for drive in detector::newly_arrived(&previous, &current) {
            println!("drive appeared: {drive}");
            logger::log("drive", &format!("new drive appeared: {drive}"));
            let root = std::path::PathBuf::from(&drive);
            if trigger::has_valid_trigger(&root) {
                println!("valid C.U.R.E trigger found on {drive}; launching GUI");
                logger::log(
                    "trigger",
                    &format!("VALID C.U.R.E trigger on {drive}; launching GUI"),
                );
                launch_gui(&root);
            } else {
                println!("no C.U.R.E trigger on {drive}; ignoring");
                logger::log(
                    "trigger",
                    &format!("invalid/missing trigger on {drive}; ignoring"),
                );
            }
        }
        previous = current;
    }
}

#[cfg(target_os = "windows")]
fn startup_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| {
        std::path::PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup")
    })
}

#[cfg(target_os = "windows")]
fn self_install() -> Result<(), Box<dyn std::error::Error>> {
    let Some(startup) = startup_dir() else {
        println!("APPDATA not set; skipping self-install (portable mode)");
        logger::log(
            "install",
            "APPDATA not set; skipped self-install (portable mode)",
        );
        return Ok(());
    };
    std::fs::create_dir_all(&startup)?;
    let dest = startup.join(WATCHER_EXE_NAME);
    if dest.exists() {
        logger::log(
            "install",
            &format!("already installed at {}", dest.display()),
        );
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    if exe.canonicalize()? == dest {
        return Ok(());
    }
    std::fs::copy(&exe, &dest)?;
    println!("installed watcher to {}", dest.display());
    logger::log(
        "install",
        &format!("installed watcher to {}", dest.display()),
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_gui(drive_root: &std::path::Path) {
    let beside_watcher = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .map(|dir| dir.join(GUI_EXE_NAME));
    let candidates = [Some(drive_root.join(GUI_EXE_NAME)), beside_watcher];
    for candidate in candidates.into_iter().flatten() {
        if candidate.is_file() {
            match std::process::Command::new(&candidate)
                .arg("--data-dir")
                .arg(drive_root)
                .spawn()
            {
                Ok(_) => {
                    println!("launched {}", candidate.display());
                    logger::log("launch", &format!("launched {}", candidate.display()));
                }
                Err(err) => {
                    println!("failed to launch {}: {err}", candidate.display());
                    logger::log(
                        "launch-error",
                        &format!("failed to launch {}: {err}", candidate.display()),
                    );
                }
            }
            return;
        }
    }
    println!(
        "no {} found on the USB drive or next to the watcher; nothing to launch",
        GUI_EXE_NAME
    );
    logger::log(
        "launch-error",
        &format!(
            "no {GUI_EXE_NAME} found on the USB drive or next to the watcher; nothing to launch"
        ),
    );
}
