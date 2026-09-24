//! File security snapshots for quarantine undo (P0-4).
//!
//! A quarantined file must come back as it was: same bytes, same timestamps,
//! same attributes, same owner/group/DACL. This module captures that state
//! into plain data ([`FileSecurity`]) and restores it best-effort.
//!
//! Platform split: the snapshot shape is cross-platform so records serialize
//! identically everywhere; only the capture/restore bodies are
//! Windows-gated. Off Windows both are no-ops (`sddl`/`attributes` stay
//! `None`, timestamps fall back to `std` metadata where available).
//!
//! Security notes, not errors: [`restore`] returns a list of human-readable
//! notes for anything it could not put back (e.g. ACL restore without
//! privilege). Callers surface those notes next to the restored file — the
//! bytes are authoritative, the notes keep the operator informed.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Portable file-security snapshot stored inside [`crate::quarantine::QuarantineRecord`].
///
/// Fidelity note: owner, group, and the ACE list round-trip exactly; Windows
/// may normalize DACL *control* flags on write (e.g. add `AI` for inherited
/// ACEs). The effective access is identical — only future-inheritance
/// bookkeeping can differ, which is outside this tool's threat model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileSecurity {
    /// Owner+group+DACL as SDDL (Windows only; `None` elsewhere or on
    /// capture failure).
    pub sddl: Option<String>,
    /// `GetFileAttributesW` bits (readonly/hidden/system/…; Windows only).
    pub attributes: Option<u32>,
    /// Modification time, nanoseconds since the Unix epoch.
    pub modified_ns: Option<u64>,
    /// Last-access time, nanoseconds since the Unix epoch.
    pub accessed_ns: Option<u64>,
    /// Creation (birth) time, nanoseconds since the Unix epoch.
    pub created_ns: Option<u64>,
}

fn system_time_ns(t: std::time::SystemTime) -> Option<u64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| d.as_nanos().try_into().ok())
}

fn metadata_times(path: &Path, sec: &mut FileSecurity) {
    if let Ok(md) = std::fs::metadata(path) {
        sec.modified_ns = md.modified().ok().and_then(system_time_ns);
        sec.accessed_ns = md.accessed().ok().and_then(system_time_ns);
        sec.created_ns = md.created().ok().and_then(system_time_ns);
    }
}

/// Capture timestamps (+ attributes/SDDL on Windows). Never fails: whatever
/// cannot be read stays `None` and is simply skipped at restore time.
pub fn capture(path: &Path) -> FileSecurity {
    let mut sec = FileSecurity::default();
    metadata_times(path, &mut sec);
    #[cfg(windows)]
    {
        sec.attributes = imp::file_attributes(path);
        sec.sddl = imp::capture_sddl(path);
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    sec
}

/// Restore a snapshot onto `path` (which must already hold the right bytes).
/// Returns notes for anything skipped or failed — empty means full fidelity.
/// Never fails outright: a partial restore beats no restore, and the caller
/// reports the notes.
pub fn restore(path: &Path, sec: &FileSecurity) -> Vec<String> {
    let mut notes = Vec::new();
    #[cfg(windows)]
    {
        imp::restore_all(path, sec, &mut notes);
    }
    #[cfg(not(windows))]
    {
        restore_modified_fallback(path, sec, &mut notes);
    }
    notes
}

#[cfg(not(windows))]
fn restore_modified_fallback(path: &Path, sec: &FileSecurity, notes: &mut Vec<String>) {
    if let Some(ns) = sec.modified_ns {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(ns);
        if std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|f| f.set_modified(t))
            .is_err()
        {
            notes.push("could not restore modification time".to_string());
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW,
        ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
        SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION,
        OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    };
    use windows::Win32::Storage::FileSystem::{GetFileAttributesW, SetFileAttributesW};

    use super::FileSecurity;

    const SNAPSHOT_INFO: OBJECT_SECURITY_INFORMATION = OBJECT_SECURITY_INFORMATION(
        OWNER_SECURITY_INFORMATION.0 | GROUP_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0,
    );

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn file_attributes(path: &Path) -> Option<u32> {
        let wide = wide(path);
        // SAFETY: NUL-terminated buffer outlives the call; no state retained.
        let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
        if attrs == u32::MAX {
            None
        } else {
            Some(attrs)
        }
    }

    pub(super) fn capture_sddl(path: &Path) -> Option<String> {
        let wide = wide(path);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wide` outlives the call; `sd` receives an allocated
        // descriptor that we free below on every path.
        let err = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                SNAPSHOT_INFO,
                None,
                None,
                None,
                None,
                &mut sd,
            )
        };
        if err != ERROR_SUCCESS {
            return None;
        }
        // From here every exit must free `sd`.
        let sddl = sddl_from_sd(sd);
        // SAFETY: `sd` was allocated by GetNamedSecurityInfoW above.
        unsafe {
            let _ = LocalFree(HLOCAL(sd.0));
        }
        sddl
    }

    fn sddl_from_sd(sd: PSECURITY_DESCRIPTOR) -> Option<String> {
        let mut out = windows::core::PWSTR::null();
        // SAFETY: `sd` is a valid descriptor from GetNamedSecurityInfoW;
        // `out` receives an allocated string freed below on every path.
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                SNAPSHOT_INFO,
                &mut out,
                None,
            )
        }
        .ok()?;
        // From here every exit must free `out`.
        let text = (!out.is_null())
            .then(|| unsafe { out.to_string() }.ok())
            .flatten();
        // SAFETY: `out` was allocated by the Convert call above.
        unsafe {
            let _ = LocalFree(HLOCAL(out.as_ptr() as _));
        }
        let text = text?;
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    pub(super) fn restore_all(path: &Path, sec: &FileSecurity, notes: &mut Vec<String>) {
        // 1. Security descriptor first: it may itself gate later writes, and
        //    a file whose ACL we cannot restore is still worth timestamping.
        if let Some(sddl) = sec.sddl.as_deref() {
            if let Err(err) = apply_sddl(path, sddl) {
                notes.push(format!("security descriptor not restored: {err}"));
            }
        }
        // 2. Timestamps (created/access need the Windows extension trait;
        //    mtime is plain std).
        restore_times(path, sec, notes);
        // 3. Attributes last: a restored READONLY bit must not precede the
        //    writes above.
        if let Some(attrs) = sec.attributes {
            let wide = wide(path);
            // SAFETY: NUL-terminated buffer outlives the call.
            if let Err(err) = unsafe {
                SetFileAttributesW(
                    PCWSTR(wide.as_ptr()),
                    windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(attrs),
                )
            } {
                notes.push(format!("file attributes not restored: {err}"));
            }
        }
    }

    fn apply_sddl(path: &Path, sddl: &str) -> windows::core::Result<()> {
        let wide_path = wide(path);
        let wide_sddl: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: both buffers outlive their calls; `sd` is freed below on
        // every path after a successful conversion.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide_sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )?;
        }
        // SAFETY: `wide_path` outlives the call; `sd` is the converted
        // descriptor, freed afterwards.
        let result = unsafe {
            if SetFileSecurityW(PCWSTR(wide_path.as_ptr()), SNAPSHOT_INFO, sd).as_bool() {
                Ok(())
            } else {
                Err(windows::core::Error::from_win32())
            }
        };
        unsafe {
            let _ = LocalFree(HLOCAL(sd.0));
        }
        result
    }

    fn restore_times(path: &Path, sec: &FileSecurity, notes: &mut Vec<String>) {
        use std::os::windows::fs::FileTimesExt;
        use std::time::{Duration, UNIX_EPOCH};

        let to_time = |ns: u64| UNIX_EPOCH + Duration::from_nanos(ns);
        let mut times = std::fs::FileTimes::new();
        let mut any = false;
        if let Some(ns) = sec.modified_ns {
            times = times.set_modified(to_time(ns));
            any = true;
        }
        if let Some(ns) = sec.accessed_ns {
            times = times.set_accessed(to_time(ns));
            any = true;
        }
        if let Some(ns) = sec.created_ns {
            times = times.set_created(to_time(ns));
            any = true;
        }
        if !any {
            return;
        }
        match std::fs::File::options().write(true).open(path) {
            Ok(f) => {
                if f.set_times(times).is_err() {
                    notes.push("file timestamps not restored".to_string());
                }
            }
            Err(_) => notes.push("file timestamps not restored".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_records_times_and_restores_modified() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"bytes").unwrap();

        let sec = capture(&file);
        assert!(sec.modified_ns.is_some());

        // Restore onto a different file: mtime must follow the snapshot.
        let other = dir.path().join("other.txt");
        std::fs::write(&other, b"other").unwrap();
        let notes = restore(&other, &sec);
        assert!(
            notes.iter().all(|n| !n.contains("modification")),
            "unexpected notes: {notes:?}"
        );
        let a = std::fs::metadata(&file).unwrap().modified().unwrap();
        let b = std::fs::metadata(&other).unwrap().modified().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn missing_file_captures_nothing() {
        let sec = capture(Path::new("Z:/definitely/not/here.bin"));
        assert_eq!(sec.sddl, None);
        assert_eq!(sec.attributes, None);
        assert_eq!(sec.modified_ns, None);
    }

    #[cfg(windows)]
    #[test]
    fn sddl_and_attributes_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.exe");
        std::fs::write(&src, b"payload").unwrap();
        let before = capture(&src);
        let sddl = before.sddl.clone().expect("temp file must have an SDDL");
        assert!(sddl.contains("O:") && sddl.contains("G:") && sddl.contains("D:"));

        // Diverge the target: different bytes, attributes, timestamps.
        let dst = dir.path().join("dst.exe");
        std::fs::write(&dst, b"other-bytes").unwrap();
        let wide: Vec<u16> = {
            use std::os::windows::ffi::OsStrExt;
            dst.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        unsafe {
            use windows::core::PCWSTR;
            use windows::Win32::Storage::FileSystem::SetFileAttributesW;
            SetFileAttributesW(
                PCWSTR(wide.as_ptr()),
                windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN,
            )
            .unwrap();
        }

        let notes = restore(&dst, &before);
        assert!(notes.is_empty(), "unexpected notes: {notes:?}");
        let after = capture(&dst);
        // Owner/group/ACEs must match exactly; DACL control flags (AI/SI/P)
        // are Windows bookkeeping and may be normalized on write.
        assert_eq!(
            canon_sddl(after.sddl.as_deref()),
            canon_sddl(before.sddl.as_deref())
        );
        assert_eq!(after.attributes, before.attributes);
        assert_eq!(after.modified_ns, before.modified_ns);
    }

    /// Strip DACL/SACL control-flag runs (`D:AI(` → `D:(`) for comparison.
    fn canon_sddl(sddl: Option<&str>) -> Option<String> {
        sddl.map(|s| {
            let mut out = s.to_string();
            for prefix in ["D:", "S:"] {
                if let Some(pos) = out.find(prefix) {
                    let start = pos + prefix.len();
                    let paren = out[start..].find('(').unwrap_or(0);
                    out.replace_range(start..start + paren, "");
                }
            }
            out
        })
    }
}
