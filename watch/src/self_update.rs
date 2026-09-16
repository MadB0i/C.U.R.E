//! Self-install / self-update of the watcher's Startup-folder copy.
//!
//! The watcher may be launched from anywhere (rescue USB, Downloads folder,
//! or the installed copy itself). On every consented start it makes sure the
//! installed copy matches the binary that is actually running, so an older
//! release left in the Startup folder can never linger forever.
//!
//! Safety rules:
//! - Byte-identical copies are never rewritten (no churn, no risk).
//! - Replacement goes through a temp file plus an atomic-style
//!   `MOVEFILE_REPLACE_EXISTING` move; the installed file is never deleted
//!   first, so a crash mid-update cannot leave *no* watcher behind.
//! - Any failure (installed copy in use by another watcher instance, access
//!   denied, missing rights) leaves the old copy intact and merely logs: the
//!   current process keeps watching with its own (newer) code either way.
//! - The watcher never launches the installed copy itself — Windows launches
//!   it at login — so there is no self-launch / update recursion loop.

use std::path::{Path, PathBuf};

pub const WATCHER_EXE_NAME: &str = "cure-watch.exe";

/// What the installer should do, given what it found on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallDecision {
    /// No installed copy yet — copy ourselves there.
    FreshInstall,
    /// Installed copy is byte-identical to the running binary — do nothing.
    UpToDate,
    /// Installed copy differs — replace it (best effort, never destructive).
    UpdateAvailable,
}

/// Pure decision function: `installed` is the on-disk copy's bytes
/// (`None` = no installed copy), `current` is the running binary's bytes.
pub fn decide(installed: Option<&[u8]>, current: &[u8]) -> InstallDecision {
    match installed {
        None => InstallDecision::FreshInstall,
        Some(old) if old == current => InstallDecision::UpToDate,
        Some(_) => InstallDecision::UpdateAvailable,
    }
}

/// Directory Windows launches at login (`%APPDATA%\...\Startup`).
#[cfg(target_os = "windows")]
pub fn startup_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| {
        PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup")
    })
}

/// True when `running_exe` and the installed copy are the same file
/// (i.e. this process already runs from the Startup folder — nothing to do).
/// Missing files compare as "not the same" (the caller then installs fresh).
pub fn is_running_from(running_exe: &Path, installed: &Path) -> bool {
    match (std::fs::canonicalize(running_exe), std::fs::canonicalize(installed)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Replace `dest` with the bytes of `src` via temp-file + replacing move.
/// On any failure `dest` is left untouched (the temp file is cleaned up).
#[cfg(target_os = "windows")]
pub fn replace(dest: &Path, src: &Path) -> std::io::Result<()> {
    let tmp = dest.with_extension("exe.new");
    std::fs::copy(src, &tmp)?;
    let moved = move_file_replace(&tmp, dest);
    if moved.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    moved
}

#[cfg(target_os = "windows")]
fn move_file_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let from_w = wide(from);
    let to_w = wide(to);
    unsafe {
        MoveFileExW(
            PCWSTR(from_w.as_ptr()),
            PCWSTR(to_w.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_installed_copy_means_fresh_install() {
        assert_eq!(decide(None, b"current-bytes"), InstallDecision::FreshInstall);
    }

    #[test]
    fn identical_bytes_mean_up_to_date() {
        let bytes = b"cure-watch fake binary v1";
        assert_eq!(
            decide(Some(bytes), bytes),
            InstallDecision::UpToDate
        );
    }

    #[test]
    fn different_bytes_mean_update_available() {
        assert_eq!(
            decide(Some(b"old build"), b"new build!"),
            InstallDecision::UpdateAvailable
        );
    }

    #[test]
    fn same_length_but_different_content_still_updates() {
        // Length-only comparison would miss this; we compare full bytes.
        assert_eq!(
            decide(Some(b"aaaa"), b"aaab"),
            InstallDecision::UpdateAvailable
        );
    }

    #[test]
    fn empty_binaries_compare_equal() {
        assert_eq!(decide(Some(b""), b""), InstallDecision::UpToDate);
    }

    #[test]
    fn missing_paths_are_not_running_from() {
        assert!(!is_running_from(
            Path::new(r"C:\definitely\not\here\a.exe"),
            Path::new(r"C:\definitely\not\here\b.exe"),
        ));
    }

    #[test]
    fn same_existing_file_is_running_from_itself() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("cure-watch.exe");
        std::fs::write(&file, b"bytes").unwrap();
        assert!(is_running_from(&file, &file));
    }

    #[test]
    fn different_existing_files_are_not_running_from() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.exe");
        let b = dir.path().join("b.exe");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        assert!(!is_running_from(&a, &b));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn replace_installs_fresh_copy() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src.exe");
        let dest = dir.path().join(WATCHER_EXE_NAME);
        std::fs::write(&src, b"new-binary-bytes").unwrap();
        replace(&dest, &src).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new-binary-bytes");
        // No temp file left behind.
        assert!(!dest.with_extension("exe.new").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn replace_overwrites_stale_copy_without_touching_source() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src.exe");
        let dest = dir.path().join(WATCHER_EXE_NAME);
        std::fs::write(&src, b"new-binary-bytes").unwrap();
        std::fs::write(&dest, b"stale-binary-bytes").unwrap();
        replace(&dest, &src).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new-binary-bytes");
        assert_eq!(std::fs::read(&src).unwrap(), b"new-binary-bytes");
        assert!(!dest.with_extension("exe.new").exists());
    }
}
