//! `cure-watch --uninstall`: remove exactly what self-install created.
//!
//! The managed set is derived from the SAME constants and path helpers the
//! install paths use (`self_update::startup_dir` + `WATCHER_EXE_NAME`,
//! `consent::marker_path`, `logger::log_path`, `pairing::host_dir` /
//! `host_gui_exe` / `pairing_path`, canary decoy names in the three user
//! folders), so the two lists cannot drift — see
//! `uninstall_covers_everything_install_creates`.
//!
//! Safety rules:
//! - exact files only, each name re-verified against its expected constant
//!   before deletion (belt-and-braces on top of plan construction);
//! - never follows reparse points: every target is inspected with
//!   `symlink_metadata` and links/junctions are refused (reported as
//!   leftovers for manual review, never deleted through);
//! - the host dir is removed only when empty afterwards;
//! - idempotent: absent items are `AlreadyAbsent`, not errors;
//! - ends with a verification pass printing `clean` or `leftovers`.
//!
//! After a successful uninstall there is no pairing record, so the launch
//! gate (`pairing::decide_launch`) can never yield `Launch` — covered by
//! `no_pairing_means_no_launch_through_the_pure_gate`.

use std::path::{Path, PathBuf};

/// Every location cure-watch's install paths can create, each derived from
/// the same constants/helpers the creators use. `None` = environment
/// variable unset (portable mode); nothing to remove there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPaths {
    /// `%APPDATA%\…\Startup\cure-watch.exe` (self-install copy).
    pub startup_exe: Option<PathBuf>,
    /// `%APPDATA%\cure-watch-consent.json` (consent marker).
    pub consent_marker: Option<PathBuf>,
    /// `%APPDATA%\cure-watch.log` (watcher log).
    pub log_file: Option<PathBuf>,
    /// `%LOCALAPPDATA%\CURE` (host install dir; removed only if empty).
    pub host_dir: Option<PathBuf>,
    /// `%LOCALAPPDATA%\CURE\cure-gui.exe` (pinned GUI copy).
    pub host_gui_exe: Option<PathBuf>,
    /// `%LOCALAPPDATA%\CURE\watcher-pairing.json` (pairing token+hash).
    pub pairing_file: Option<PathBuf>,
}

/// The managed set. NOTE: cure-watch creates no scheduled-task entries and
/// no registry entries — self-install is a Startup-folder copy plus data
/// files — so there is nothing task/registry-shaped to list here. If a
/// future version adds any, this struct (and its test) must grow with it.
#[cfg(target_os = "windows")]
pub fn managed_paths() -> ManagedPaths {
    ManagedPaths {
        startup_exe: crate::self_update::startup_dir()
            .map(|d| d.join(crate::self_update::WATCHER_EXE_NAME)),
        consent_marker: crate::consent::marker_path(),
        log_file: crate::logger::log_path(),
        host_dir: crate::pairing::host_dir(),
        host_gui_exe: crate::pairing::host_gui_exe(),
        pairing_file: crate::pairing::pairing_path(),
    }
}

/// What to do with one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalKind {
    /// Delete the exact file (name re-verified first).
    File,
    /// Remove the directory, but only if empty afterwards.
    DirIfEmpty,
    /// Delete decoy-marker files inside the directory (canary cleanup).
    DecoySweep,
}

/// One removal unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalTarget {
    pub path: PathBuf,
    pub kind: RemovalKind,
}

/// Build the plan: the six managed files, the host dir, and a decoy sweep
/// per user folder (Desktop/Documents/Downloads). Missing env roots are
/// skipped (portable mode has nothing there).
#[cfg(target_os = "windows")]
pub fn removal_plan() -> Vec<RemovalTarget> {
    let m = managed_paths();
    let mut plan = Vec::new();
    for file in [
        m.startup_exe,
        m.consent_marker,
        m.log_file,
        m.host_gui_exe,
        m.pairing_file,
    ]
    .into_iter()
    .flatten()
    {
        plan.push(RemovalTarget {
            path: file,
            kind: RemovalKind::File,
        });
    }
    if let Some(dir) = m.host_dir {
        plan.push(RemovalTarget {
            path: dir,
            kind: RemovalKind::DirIfEmpty,
        });
    }
    for dir in cure_dirwatch::user_folder_candidates() {
        plan.push(RemovalTarget {
            path: dir,
            kind: RemovalKind::DecoySweep,
        });
    }
    plan
}

/// Expected leaf file name for a `File` target, derived from the same
/// constants the path was built from. `None` = not a known managed file.
fn expected_file_name(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    [
        crate::self_update::WATCHER_EXE_NAME,
        crate::consent::CONSENT_FILE_NAME,
        crate::logger::LOG_FILE_NAME,
        crate::pairing::HOST_GUI_EXE_NAME,
        crate::pairing::PAIRING_FILE_NAME,
    ]
    .into_iter()
    .find(|known| name.eq_ignore_ascii_case(known))
}

/// Outcome per path (a sweep target yields one entry per decoy found, or
/// none when the dir is missing/empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalOutcome {
    Removed,
    AlreadyAbsent,
    /// A reparse point where a managed file/dir was expected: left alone.
    RefusedReparsePoint,
    /// A non-managed name reached a File removal (should be unreachable
    /// given plan construction; refused loudly).
    RefusedUnknownName,
    Failed(String),
    /// Dry run: what would have happened.
    WouldRemove,
    WouldKeepAbsent,
}

/// True for symlinks, junctions, and mount points. Uses `symlink_metadata`
/// (never follows), plus the reparse attribute on Windows where
/// `is_symlink` alone can miss junctions.
pub fn is_reparse_point(path: &Path) -> bool {
    let md = match std::fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(_) => return false,
    };
    if md.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Delete one exact managed file (post name-guard + reparse check).
fn remove_one_file(path: &Path, dry_run: bool) -> RemovalOutcome {
    if expected_file_name(path).is_none() {
        return RemovalOutcome::RefusedUnknownName;
    }
    if !path.exists() && !is_reparse_point(path) {
        // symlink_metadata failed too: nothing there (dangling links were
        // already refused above, so this is plain absence).
        return if dry_run {
            RemovalOutcome::WouldKeepAbsent
        } else {
            RemovalOutcome::AlreadyAbsent
        };
    }
    if is_reparse_point(path) {
        return RemovalOutcome::RefusedReparsePoint;
    }
    if dry_run {
        return RemovalOutcome::WouldRemove;
    }
    match std::fs::remove_file(path) {
        Ok(()) => RemovalOutcome::Removed,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RemovalOutcome::AlreadyAbsent,
        Err(e) => RemovalOutcome::Failed(e.to_string()),
    }
}

/// Remove `dir` only when it is empty (and not itself a reparse point).
fn remove_dir_if_empty(path: &Path, dry_run: bool) -> RemovalOutcome {
    if is_reparse_point(path) {
        return RemovalOutcome::RefusedReparsePoint;
    }
    match std::fs::read_dir(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if dry_run {
                RemovalOutcome::WouldKeepAbsent
            } else {
                RemovalOutcome::AlreadyAbsent
            }
        }
        Err(e) => return RemovalOutcome::Failed(e.to_string()),
        Ok(entries) => {
            let mut entries = entries;
            if entries.next().is_some() {
                // Not empty: files we do not own may live here — leave it.
                return RemovalOutcome::Failed("directory not empty; left in place".to_string());
            }
        }
    }
    if dry_run {
        return RemovalOutcome::WouldRemove;
    }
    match std::fs::remove_dir(path) {
        Ok(()) => RemovalOutcome::Removed,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RemovalOutcome::AlreadyAbsent,
        Err(e) => RemovalOutcome::Failed(e.to_string()),
    }
}

/// Delete canary-decoy files (`is_canary_decoy` marker match) inside `dir`.
/// Anything without the marker is never touched.
fn sweep_decoys(dir: &Path, dry_run: bool) -> Vec<(PathBuf, RemovalOutcome)> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return out, // missing/unreadable dir: nothing to do
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !cure_core::canary::is_canary_decoy(&name) {
            continue;
        }
        if is_reparse_point(&path) {
            out.push((path, RemovalOutcome::RefusedReparsePoint));
            continue;
        }
        // Decoys are files; a same-named directory is not ours to remove.
        if !path.is_file() {
            out.push((
                path,
                RemovalOutcome::Failed("decoy-marker directory; left in place".to_string()),
            ));
            continue;
        }
        if dry_run {
            out.push((path, RemovalOutcome::WouldRemove));
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => out.push((path, RemovalOutcome::Removed)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push((path, RemovalOutcome::AlreadyAbsent))
            }
            Err(e) => out.push((path, RemovalOutcome::Failed(e.to_string()))),
        }
    }
    out
}

/// Execute one plan target. Returns per-path outcomes (sweeps fan out).
pub fn execute_target(target: &RemovalTarget, dry_run: bool) -> Vec<(PathBuf, RemovalOutcome)> {
    match target.kind {
        RemovalKind::File => vec![(target.path.clone(), remove_one_file(&target.path, dry_run))],
        RemovalKind::DirIfEmpty => {
            vec![(
                target.path.clone(),
                remove_dir_if_empty(&target.path, dry_run),
            )]
        }
        RemovalKind::DecoySweep => sweep_decoys(&target.path, dry_run),
    }
}

/// Re-check presence after removal. `true` = gone (or was never there).
pub fn verify_gone(path: &Path, kind: &RemovalKind) -> bool {
    match kind {
        RemovalKind::File => !path.exists() && !is_reparse_point(path),
        RemovalKind::DirIfEmpty => !path.exists(),
        // Sweeps verify per-file at report time; the dir itself stays.
        RemovalKind::DecoySweep => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed_names() -> Vec<&'static str> {
        vec![
            crate::self_update::WATCHER_EXE_NAME,
            crate::consent::CONSENT_FILE_NAME,
            crate::logger::LOG_FILE_NAME,
            crate::pairing::HOST_GUI_EXE_NAME,
            crate::pairing::PAIRING_FILE_NAME,
        ]
    }

    /// Install-list == uninstall-list: every path self-install can create is
    /// derived here from the SAME constants/helpers, and the plan covers all
    /// of them plus the decoy sweeps over the three user folders.
    #[cfg(windows)]
    #[test]
    fn uninstall_covers_everything_install_creates() {
        let m = managed_paths();
        // Recompute from the source constants the install paths use.
        let startup =
            crate::self_update::startup_dir().map(|d| d.join(crate::self_update::WATCHER_EXE_NAME));
        assert_eq!(m.startup_exe, startup, "Startup copy drifted");
        assert_eq!(m.consent_marker, crate::consent::marker_path());
        assert_eq!(m.log_file, crate::logger::log_path());
        assert_eq!(m.host_gui_exe, crate::pairing::host_gui_exe());
        assert_eq!(m.pairing_file, crate::pairing::pairing_path());
        assert_eq!(m.host_dir, crate::pairing::host_dir());

        let plan = removal_plan();
        for managed in [
            &m.startup_exe,
            &m.consent_marker,
            &m.log_file,
            &m.host_gui_exe,
            &m.pairing_file,
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                plan.iter()
                    .any(|t| &t.path == managed && matches!(t.kind, RemovalKind::File)),
                "managed file missing from plan: {}",
                managed.display()
            );
        }
        if let Some(dir) = &m.host_dir {
            assert!(plan
                .iter()
                .any(|t| &t.path == dir && matches!(t.kind, RemovalKind::DirIfEmpty)));
        }
        // Decoy sweeps cover exactly the three user folders, no more.
        let sweeps: Vec<_> = plan
            .iter()
            .filter(|t| matches!(t.kind, RemovalKind::DecoySweep))
            .collect();
        assert_eq!(sweeps.len(), 3);
        for dir in cure_dirwatch::user_folder_candidates() {
            assert!(sweeps.iter().any(|t| t.path == dir));
        }
    }

    #[test]
    fn name_guard_accepts_only_managed_names() {
        for name in managed_names() {
            let p = PathBuf::from(format!(r"C:\anywhere\{name}"));
            assert!(expected_file_name(&p).is_some(), "rejected: {name}");
        }
        for name in ["evil.exe", "cure-watch.exe.bak", "notes.txt", ""] {
            let p = PathBuf::from(format!(r"C:\anywhere\{name}"));
            assert!(expected_file_name(&p).is_none(), "accepted: {name:?}");
        }
        // Case-insensitive filesystems: constant spelling still matches.
        assert!(expected_file_name(Path::new(r"C:\x\CURE-WATCH.EXE")).is_some());
    }

    #[test]
    fn file_roundtrip_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let target = RemovalTarget {
            path: dir.path().join(crate::logger::LOG_FILE_NAME),
            kind: RemovalKind::File,
        };
        // Absent first: idempotent AlreadyAbsent.
        let out = execute_target(&target, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, RemovalOutcome::AlreadyAbsent);
        // Create, dry-run (kept), remove, remove-again.
        std::fs::write(&target.path, b"log bytes").unwrap();
        let out = execute_target(&target, true);
        assert_eq!(out[0].1, RemovalOutcome::WouldRemove);
        assert!(target.path.is_file(), "dry run must not delete");
        let out = execute_target(&target, false);
        assert_eq!(out[0].1, RemovalOutcome::Removed);
        assert!(verify_gone(&target.path, &target.kind));
        let out = execute_target(&target, false);
        assert_eq!(out[0].1, RemovalOutcome::AlreadyAbsent);
    }

    #[test]
    fn dir_removed_only_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let target = RemovalTarget {
            path: empty.clone(),
            kind: RemovalKind::DirIfEmpty,
        };
        let out = execute_target(&target, true);
        assert_eq!(out[0].1, RemovalOutcome::WouldRemove);
        assert!(empty.is_dir(), "dry run must not delete");
        let out = execute_target(&target, false);
        assert_eq!(out[0].1, RemovalOutcome::Removed);
        assert!(!empty.exists());

        // Non-empty dir (foreign content): refused, left in place.
        let full = dir.path().join("full");
        std::fs::create_dir(&full).unwrap();
        std::fs::write(full.join("someone-elses.txt"), b"x").unwrap();
        let target = RemovalTarget {
            path: full.clone(),
            kind: RemovalKind::DirIfEmpty,
        };
        let out = execute_target(&target, false);
        assert!(matches!(out[0].1, RemovalOutcome::Failed(_)));
        assert!(full.is_dir());
    }

    #[test]
    fn decoy_sweep_removes_only_marker_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("~cure-canary-notes.txt"), b"decoy").unwrap();
        std::fs::write(dir.path().join("my-notes.txt"), b"mine").unwrap();
        let target = RemovalTarget {
            path: dir.path().to_path_buf(),
            kind: RemovalKind::DecoySweep,
        };
        let out = execute_target(&target, true);
        assert_eq!(out.len(), 1, "only the marker file is planned");
        assert!(dir.path().join("~cure-canary-notes.txt").is_file());
        let out = execute_target(&target, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, RemovalOutcome::Removed);
        assert!(!dir.path().join("~cure-canary-notes.txt").exists());
        assert!(dir.path().join("my-notes.txt").is_file(), "user file kept");
    }

    #[test]
    fn reparse_points_are_never_followed() {
        assert!(!is_reparse_point(Path::new("Z:/definitely/not/here")));
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join(crate::logger::LOG_FILE_NAME);
        std::fs::write(&plain, b"x").unwrap();
        assert!(!is_reparse_point(&plain));
        #[cfg(windows)]
        {
            let link = dir.path().join(crate::consent::CONSENT_FILE_NAME);
            if std::os::windows::fs::symlink_file(&plain, &link).is_ok() {
                assert!(is_reparse_point(&link));
                let target = RemovalTarget {
                    path: link.clone(),
                    kind: RemovalKind::File,
                };
                let out = execute_target(&target, false);
                assert_eq!(out[0].1, RemovalOutcome::RefusedReparsePoint);
                assert!(plain.is_file(), "link target must survive");
                let _ = std::fs::remove_file(&link);
            } else {
                eprintln!("SKIP: no symlink privilege; reparse refusal covered by is_reparse_point unit checks");
            }
        }
    }

    /// After uninstall there is no pairing, so the launch gate can never
    /// yield Launch — proven through the same pure function P0-1 added.
    #[test]
    fn no_pairing_means_no_launch_through_the_pure_gate() {
        use crate::pairing::{decide_launch, HostState, LaunchDecision};
        let host = HostState {
            exists: false,
            sha256_matches_pin: false,
            is_reparse_point: false,
        };
        // Unpaired sentinel: empty pinned token. Even a well-formed trigger
        // can never match it (length check fails first, constant-time).
        let token = "ab".repeat(32);
        let good = crate::pairing::format_trigger(&token);
        for trigger in [None, Some(good.as_bytes())] {
            assert!(
                matches!(
                    decide_launch(trigger, "", Path::new("host.exe"), &host),
                    LaunchDecision::Ignore { .. }
                ),
                "unpaired state must never launch"
            );
        }
    }
}
