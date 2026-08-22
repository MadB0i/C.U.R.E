#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod detector;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod drives;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod trigger;

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
    self_install()?;
    println!("cure-watch is watching for rescue USBs (Ctrl+C to stop)...");

    let mut previous = drives::list_drives();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        let current = drives::list_drives();
        for drive in detector::newly_arrived(&previous, &current) {
            println!("drive appeared: {drive}");
            let root = std::path::PathBuf::from(&drive);
            if trigger::has_valid_trigger(&root) {
                println!("valid C.U.R.E trigger found on {drive}; launching GUI");
                launch_gui(&root);
            } else {
                println!("no C.U.R.E trigger on {drive}; ignoring");
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
        return Ok(());
    };
    std::fs::create_dir_all(&startup)?;
    let dest = startup.join(WATCHER_EXE_NAME);
    if dest.exists() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    if exe.canonicalize()? == dest {
        return Ok(());
    }
    std::fs::copy(&exe, &dest)?;
    println!("installed watcher to {}", dest.display());
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_gui(drive_root: &std::path::Path) {
    let beside_watcher = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .map(|dir| dir.join(GUI_EXE_NAME));
    let candidates = [
        Some(drive_root.join(GUI_EXE_NAME)),
        beside_watcher,
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.is_file() {
            match std::process::Command::new(&candidate)
                .arg("--data-dir")
                .arg(drive_root)
                .spawn()
            {
                Ok(_) => println!("launched {}", candidate.display()),
                Err(err) => println!("failed to launch {}: {err}", candidate.display()),
            }
            return;
        }
    }
    println!(
        "no {} found on the USB drive or next to the watcher; nothing to launch",
        GUI_EXE_NAME
    );
}
