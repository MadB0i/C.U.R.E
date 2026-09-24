use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{PersistenceEntry, PersistenceSource};

pub fn default_startup_root() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(r"Microsoft\Windows\Start Menu\Programs\Startup")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".config/autostart")
    }
}

/// Machine-wide Startup folder (`%ProgramData%\…\Startup`). `None`
/// off-Windows or when `%ProgramData%` is unset — callers treat that as
/// "no common root" rather than an error.
pub fn default_common_startup_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .map(|base| common_startup_root_from(&base))
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn common_startup_root_from(base: &Path) -> PathBuf {
    base.join(r"Microsoft\Windows\Start Menu\Programs\Startup")
}

/// Scan the machine-wide Startup folder with the same file-only,
/// never-execute treatment as the per-user one. Empty when there is no
/// common root (non-Windows, unset `%ProgramData%`) or it is unreadable.
pub fn scan_common() -> Vec<PersistenceEntry> {
    match default_common_startup_root() {
        Some(root) => scan(&root),
        None => Vec::new(),
    }
}

pub fn scan(root: &Path) -> Vec<PersistenceEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = fs::read_dir(root) else {
        return entries;
    };
    let mut paths: Vec<PathBuf> = read_dir.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let location = path.to_string_lossy().into_owned();
        // For shortcuts, command is the resolved target + args (what gets
        // executed), while location stays the .lnk file itself (what gets
        // quarantined). This separation is load-bearing: risk.rs, signature
        // checks, and reports must score the target, never the .lnk path.
        let command = if crate::entry_details::is_shortcut_name(&name) {
            resolve_lnk_command(&path).unwrap_or_else(|| location.clone())
        } else {
            location.clone()
        };
        entries.push(PersistenceEntry::new(
            PersistenceSource::StartupFolder,
            name,
            command,
            location,
        ));
    }
    entries
}

/// Resolve a Startup .lnk to its target + arguments, expanded and made
/// absolute. Returns `None` when the shortcut is unparseable or has no
/// target — caller falls back to the .lnk path itself.
fn resolve_lnk_command(link_path: &Path) -> Option<String> {
    let info = crate::lnk::analyze(link_path)?;
    let mut target = info.target?;
    // Environment strings (%TEMP%, %APPDATA%, ...) are untrusted input;
    // expand them via the same helper services use for ImagePath.
    target = crate::scanners::services::expand_env_vars(&target);
    // If still relative, resolve against the shortcut's working dir or its
    // parent directory. The shell already does this on Windows, but the raw
    // fallback (and non-Windows) leaves relative strings as-is.
    let target_path = Path::new(&target);
    let mut absolute = if target_path.is_absolute() {
        target
    } else {
        let base = info
            .working_dir
            .as_deref()
            .map(Path::new)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| link_path.parent().unwrap_or_else(|| Path::new(".")));
        base.join(target_path).to_string_lossy().into_owned()
    };
    // Shell may return 8.3 short names (CURE_T~1.EXE); expand to long for
    // scoring heuristics (random-name detection needs the long name) and
    // for stable display. Best-effort: if the file doesn't exist, keep as-is.
    absolute = to_long_path_if_exists(&absolute);
    if let Some(args) = info.arguments.filter(|a| !a.is_empty()) {
        Some(format!("{absolute} {args}"))
    } else {
        Some(absolute)
    }
}

#[cfg(windows)]
fn to_long_path_if_exists(path: &str) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    let len = unsafe { GetLongPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf)) };
    if len > 0 && (len as usize) < buf.len() {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        path.to_string()
    }
}

#[cfg(not(windows))]
fn to_long_path_if_exists(path: &str) -> String {
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::make_id;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn lists_files_as_startup_entries() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("legit-update.bat"),
            "@echo off\r\nrem ok\r\n",
        )
        .unwrap();
        fs::write(dir.path().join("a7x9k2p9.cmd"), "start evil.exe").unwrap();
        fs::create_dir(dir.path().join("subfolder")).unwrap();
        fs::write(dir.path().join("subfolder").join("nested.txt"), "skip me").unwrap();

        let entries = scan(dir.path());

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a7x9k2p9.cmd");
        assert_eq!(entries[1].name, "legit-update.bat");
        for e in &entries {
            assert_eq!(e.source, PersistenceSource::StartupFolder);
            assert_eq!(e.command, e.location);
            assert_eq!(
                e.id,
                make_id(&PersistenceSource::StartupFolder, &e.name, &e.command)
            );
        }

        let again = scan(dir.path());
        assert_eq!(entries, again);
    }

    #[test]
    fn missing_directory_yields_no_entries() {
        assert!(scan(Path::new("Z:/definitely/not/here")).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn common_root_lives_under_programdata() {
        let base = Path::new(r"C:\ProgramData");
        assert_eq!(
            common_startup_root_from(base),
            PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Startup")
        );
    }

    #[test]
    fn common_scan_never_panics_and_uses_startup_source() {
        // Live machine-wide folder when present; empty elsewhere. Either
        // way every row is a StartupFolder entry with command == location.
        for e in scan_common() {
            assert_eq!(e.source, PersistenceSource::StartupFolder);
            assert_eq!(e.command, e.location);
        }
    }

    // -----------------------------------------------------------------
    // F-LNK-1 scoring bug — these MUST fail on the current scanner
    // (command == lnk path) and pass after the fix (command == target).
    // -----------------------------------------------------------------

    #[cfg(windows)]
    fn create_shell_link(link: &Path, target: &str, args: &str, workdir: &str) {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::{Interface, PCWSTR};
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

        fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
            s.encode_wide().chain(std::iter::once(0)).collect()
        }
        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            // Borrowed already-initialized apartments (S_FALSE) are left as-is;
            // only our own init (S_OK) gets torn down.
            let init_s_ok = init == windows::Win32::Foundation::S_OK;
            let res = (|| -> windows::core::Result<()> {
                let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                let t = wide(std::ffi::OsStr::new(target));
                shell.SetPath(PCWSTR(t.as_ptr()))?;
                let a = wide(std::ffi::OsStr::new(args));
                shell.SetArguments(PCWSTR(a.as_ptr()))?;
                let w = wide(std::ffi::OsStr::new(workdir));
                shell.SetWorkingDirectory(PCWSTR(w.as_ptr()))?;
                let persist: IPersistFile = shell.cast()?;
                let l = wide(link.as_os_str());
                persist.Save(PCWSTR(l.as_ptr()), BOOL::from(false))?;
                persist.SaveCompleted(PCWSTR(l.as_ptr()))?;
                Ok(())
            })();
            if init_s_ok {
                CoUninitialize();
            }
            res.expect("IShellLink fixture");
        }
    }

    #[cfg(windows)]
    fn temp_startup_in_repo() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("cure-startup-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn lnk_command_is_resolved_target_not_lnk_path() {
        let startup = temp_startup_in_repo();
        let target_dir = tempdir().unwrap();
        let target = target_dir.path().join("dummy-target.exe");
        std::fs::write(&target, b"MZ dummy").unwrap();
        let link = startup.path().join("probe.lnk");
        create_shell_link(&link, &target.to_string_lossy(), "--flag", "");

        let entries = scan(startup.path());
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        // location stays the persistence mechanism (the .lnk file itself)
        assert_eq!(e.location, link.to_string_lossy().as_ref());
        // command must be the resolved target + args, never the .lnk path
        assert_eq!(
            e.command,
            format!("{} --flag", target.to_string_lossy()),
            "F-LNK-1: command still points at .lnk, not target"
        );
    }

    #[cfg(windows)]
    #[test]
    fn lnk_unsigned_dummy_must_not_score_safe() {
        let startup = temp_startup_in_repo();
        // Place unsigned target in a real drop zone (Temp) with a long
        // random-looking name so heuristics fire. The .lnk itself lives in
        // the repo startup dir (no drop zone), so scoring the .lnk path
        // would incorrectly be Safe.
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path().join("a7x9k2p9q1w2b3.exe");
        // Copy current test binary (unsigned PE) as dummy; signature will be Unsigned
        let src = std::env::current_exe().unwrap();
        std::fs::copy(&src, &target).unwrap();
        let link = startup.path().join("evil.lnk");
        create_shell_link(&link, &target.to_string_lossy(), "", "");

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "evil.lnk").unwrap();
        let scored = crate::risk::score_entry(
            e,
            crate::signature::resolve_executable_path(&e.command).as_deref(),
        );
        assert_ne!(
            scored.risk,
            crate::model::RiskLevel::Safe,
            "unsigned dummy via .lnk scored Safe (score {}): F-LNK-1 scoring used lnk path, not target",
            scored.score
        );
    }

    #[cfg(windows)]
    #[test]
    fn lnk_signed_target_scores_safe() {
        let startup = temp_startup_in_repo();
        let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
        if !notepad.is_file() {
            return;
        }
        let link = startup.path().join("signed.lnk");
        create_shell_link(&link, &notepad.to_string_lossy(), "", "");

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "signed.lnk").unwrap();
        let scored = crate::risk::score_entry(
            e,
            crate::signature::resolve_executable_path(&e.command).as_deref(),
        );
        assert_eq!(
            scored.risk,
            crate::model::RiskLevel::Safe,
            "signed notepad via .lnk should be Safe, got {:?} score {}",
            scored.risk,
            scored.score
        );
    }

    #[cfg(windows)]
    #[test]
    fn lnk_powershell_args_scored_like_run_key() {
        let startup = temp_startup_in_repo();
        // Use a non-trusted synthetic path so the sneaky-powershell heuristic is visible.
        // Full System32 path is trusted (-20) and would cancel the +25, making both Safe.
        let ps = r"C:\cure-synth\powershell.exe";
        let args = "-WindowStyle Hidden -EncodedCommand dGVzdA==";
        let link = startup.path().join("ps.lnk");
        create_shell_link(&link, ps, args, "");

        let entries = scan(startup.path());
        let lnk_entry = entries.iter().find(|e| e.name == "ps.lnk").unwrap();

        let run_entry = crate::model::PersistenceEntry::new(
            crate::model::PersistenceSource::RegistryRun,
            "ps-test",
            format!("{ps} {args}"),
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        );
        let lnk_scored = crate::risk::score_entry(
            lnk_entry,
            crate::signature::resolve_executable_path(&lnk_entry.command).as_deref(),
        );
        let run_scored = crate::risk::score_entry(
            &run_entry,
            crate::signature::resolve_executable_path(&run_entry.command).as_deref(),
        );
        assert_eq!(
            lnk_scored.score, run_scored.score,
            "F-LNK-1: lnk powershell score {} != Run key score {} (lnk command was {:?}, expected target+args)",
            lnk_scored.score, run_scored.score, lnk_entry.command
        );
        assert_eq!(lnk_scored.risk, run_scored.risk);
    }

    #[cfg(windows)]
    #[test]
    fn lnk_relative_target_resolved_via_workdir() {
        let startup = temp_startup_in_repo();
        let work = tempdir().unwrap();
        let target = work.path().join("rel.exe");
        std::fs::write(&target, b"MZ rel").unwrap();
        let link = startup.path().join("rel.lnk");
        // Relative target + working dir containing it — shell must resolve to absolute
        create_shell_link(&link, "rel.exe", "", &work.path().to_string_lossy());

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "rel.lnk").unwrap();
        assert!(
            Path::new(&e.command).is_absolute(),
            "relative target not resolved to absolute: command={:?}",
            e.command
        );
        assert_eq!(
            Path::new(&e.command).file_name().unwrap().to_string_lossy(),
            "rel.exe"
        );
        // Display path must be absolute, is_file check must not use cwd
        assert!(!e.command.contains(r".\rel.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn lnk_env_var_target_expanded() {
        let startup = temp_startup_in_repo();
        // Use %TEMP% which is guaranteed set
        let temp_expanded = std::env::var("TEMP").unwrap();
        let target_name = "CURE_TEST_env_target.exe";
        let real_target = Path::new(&temp_expanded).join(target_name);
        std::fs::write(&real_target, b"MZ env").unwrap();
        let link = startup.path().join("env.lnk");
        let raw_target = format!("%TEMP%\\{target_name}");
        create_shell_link(&link, &raw_target, "", "");

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "env.lnk").unwrap();
        // Command must be the expanded absolute target, not the raw %VAR% and not the .lnk path
        assert_eq!(
            e.command,
            real_target.to_string_lossy().as_ref(),
            "env var not expanded to absolute: command={:?}",
            e.command
        );
        assert_ne!(e.command, e.location);
        std::fs::remove_file(&real_target).ok();
    }

    #[cfg(windows)]
    #[test]
    fn lnk_with_arguments_preserved() {
        let startup = temp_startup_in_repo();
        let target = startup.path().join("app.exe");
        std::fs::write(&target, b"MZ app").unwrap();
        let link = startup.path().join("args.lnk");
        create_shell_link(
            &link,
            &target.to_string_lossy(),
            "--flag \"a b\" /q",
            r"C:\Windows",
        );

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "args.lnk").unwrap();
        assert!(
            e.command.contains("--flag"),
            "arguments lost: command={:?}",
            e.command
        );
        assert!(
            e.command.contains("/q"),
            "arguments lost: command={:?}",
            e.command
        );
    }

    #[cfg(windows)]
    #[test]
    fn lnk_missing_target_reported_missing_with_absolute_path() {
        let startup = temp_startup_in_repo();
        let ghost = r"C:\nonexistent\CURE_TEST_ghost_e7f3a1.exe";
        let link = startup.path().join("ghost.lnk");
        // Use raw fixture for ghost (shell GetPath may not return non-existent absolute)
        std::fs::write(&link, crate::fixtures::minimal_lnk_unicode(ghost, "", "")).unwrap();

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "ghost.lnk").unwrap();
        assert_eq!(e.command, ghost);
        // entry_details shortcut path must be absolute and not exist
        let details = crate::entry_details::for_entry(e);
        let sc = details.shortcut.expect("shortcut details");
        assert_eq!(sc.expanded_target.as_deref(), Some(ghost));
        assert!(!sc.target_exists, "ghost target should be missing");
    }

    #[cfg(windows)]
    #[test]
    fn lnk_hash_ioc_matches_target_not_lnk() {
        let startup = temp_startup_in_repo();
        let payload = b"CURE-TEST-MALWARE-SIGNATURE-DO-NOT-USE";
        let target = startup.path().join("bad.exe");
        std::fs::write(&target, payload).unwrap();
        let link = startup.path().join("bad.lnk");
        create_shell_link(&link, &target.to_string_lossy(), "", "");

        let entries = scan(startup.path());
        let e = entries.iter().find(|e| e.name == "bad.lnk").unwrap();
        let exe_path = crate::signature::resolve_executable_path(&e.command);
        assert!(
            exe_path.is_some(),
            "target not resolved for hash check: command={:?}",
            e.command
        );
        let hash_hit = crate::hash_intel::check_hash(exe_path.as_deref().unwrap());
        assert!(
            hash_hit.is_some(),
            "hash IOC should match target bytes, not lnk bytes"
        );
        let scored = crate::risk::score_entry(e, exe_path.as_deref());
        assert_eq!(
            scored.risk,
            crate::model::RiskLevel::HighRisk,
            "hash hit on target must force High"
        );
    }
}
