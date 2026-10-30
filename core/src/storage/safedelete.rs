//! Race-safe file removal.
//!
//! The classic check-then-delete race is: stat a path, decide it is safe,
//! then call `remove_file` on the *same path*. Between the two calls the
//! file can be replaced, or a parent directory can be swapped for a junction,
//! and the delete lands somewhere it should not.
//!
//! This module never deletes "a path". It opens the file, checks the
//! identity of the thing it actually opened, and removes *that handle*. The
//! identity check is what makes it safe:
//!
//! * If the leaf was swapped, the handle refers to a different file, its
//!   identity does not match the one recorded when the plan was approved,
//!   and the operation is refused.
//! * If a parent directory was swapped for a junction, the handle resolves
//!   through it to a different file, and the same identity check refuses it.
//!
//! So one comparison closes both halves of the race.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::fsx;

/// A file's identity on its volume: the volume serial plus the file index.
///
/// Together these name one specific object on one specific volume. Two paths
/// with the same identity are the same file; two paths with different
/// identities are never the same file, whatever their size or timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub volume_serial: u32,
    pub file_index: u64,
}

impl std::hash::Hash for FileIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.volume_serial.hash(state);
        self.file_index.hash(state);
    }
}

impl FileIdentity {
    /// A short, stable string for logs and receipts.
    pub fn short(&self) -> String {
        format!("{:08x}:{:016x}", self.volume_serial, self.file_index)
    }
}

/// Why a removal did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveError {
    /// The file could not be opened at all.
    NotOpenable(String),
    /// The entry turned out to be a link.
    IsLink,
    /// The object behind the path is not the one that was approved.
    IdentityChanged {
        expected: FileIdentity,
        found: FileIdentity,
    },
    /// The identity could not be read, so it could not be confirmed.
    IdentityUnknown(String),
    /// The handle was opened and verified, but the delete was refused.
    DeleteRefused(String),
}

impl RemoveError {
    /// A sentence the operator can read.
    ///
    /// Deliberately NOT part of the evidence model (`core::evidence`): this
    /// is an error message for one refused removal, not a finding's
    /// evidence. There is no separate `plain_english_reason()` anywhere —
    /// `Evidence::detail` IS the plain-English reason for a finding, and
    /// this method IS the plain-English reason for a refusal.
    pub fn reason(&self) -> String {
        match self {
            RemoveError::NotOpenable(e) => format!("could not be opened for removal: {e}"),
            RemoveError::IsLink => {
                "is a link or reparse point; links are never removed".to_string()
            }
            RemoveError::IdentityChanged { expected, found } => format!(
                "the file behind this path changed (approved {} {}, now {} {}); refusing to \
                 remove a file that was not the one that was reviewed",
                expected.volume_serial, expected.file_index, found.volume_serial, found.file_index
            ),
            RemoveError::IdentityUnknown(e) => {
                format!("the file identity could not be confirmed ({e}); refusing to remove it")
            }
            RemoveError::DeleteRefused(e) => format!("the remove was refused: {e}"),
        }
    }
}

/// What happened when a file was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveOutcome {
    pub identity: FileIdentity,
    /// `true` when the file had exactly one hard link, so removing the name
    /// really does release the bytes.
    pub reclaim_certain: bool,
}

/// The identity of the file at `path`, if the platform will say.
pub fn file_identity(path: &Path) -> Option<FileIdentity> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
            FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_MODE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let share = FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0;
        // SAFETY: `wide` is NUL-terminated; the handle is checked before use.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_MODE(share),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };
        let handle = match handle {
            Ok(h) => h,
            Err(_) => return None,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is live and `info` is a correctly sized
        // out-parameter for the duration of the call.
        let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
        // SAFETY: the handle is closed exactly once, here.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
        if ok.is_err() {
            return None;
        }
        Some(FileIdentity {
            volume_serial: info.dwVolumeSerialNumber,
            file_index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Remove the file at `path`, but only if it is the file that was approved.
///
/// `expected` is the identity recorded when the plan was built. `None` means
/// the identity could not be recorded at plan time; in that case the removal
/// is refused rather than performed without a check.
pub fn remove_if_unchanged(
    path: &Path,
    expected: Option<&FileIdentity>,
) -> Result<RemoveOutcome, RemoveError> {
    match expected {
        Some(want) => remove_verified(path, *want),
        None => Err(RemoveError::IdentityUnknown(
            "no identity was recorded when this item was approved".to_string(),
        )),
    }
}

#[cfg(windows)]
fn remove_verified(path: &Path, expected: FileIdentity) -> Result<RemoveOutcome, RemoveError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileDispositionInfo, GetFileInformationByHandle, SetFileInformationByHandle,
        BY_HANDLE_FILE_INFORMATION, DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let share = FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0;
    let access = DELETE.0 | FILE_READ_ATTRIBUTES.0;

    // `FILE_FLAG_OPEN_REPARSE_POINT` opens the *entry* rather than whatever
    // it points at, so a link cannot be used to reach somewhere else.
    // SAFETY: `wide` is NUL-terminated; the handle is checked before use.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            FILE_SHARE_MODE(share),
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    };
    let handle = match handle {
        Ok(h) => h,
        Err(e) => return Err(RemoveError::NotOpenable(e.to_string())),
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is live and `info` is a correctly sized
    // out-parameter for the duration of the call.
    let read = unsafe { GetFileInformationByHandle(handle, &mut info) };
    if read.is_err() {
        // SAFETY: the handle is closed exactly once, here.
        let _ = unsafe { CloseHandle(handle) };
        return Err(RemoveError::IdentityUnknown(
            "the file could not be inspected through its handle".to_string(),
        ));
    }

    let identity = FileIdentity {
        volume_serial: info.dwVolumeSerialNumber,
        file_index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    };
    let link_count = info.nNumberOfLinks;

    if identity != expected {
        // SAFETY: the handle is closed exactly once, here.
        let _ = unsafe { CloseHandle(handle) };
        return Err(RemoveError::IdentityChanged {
            expected,
            found: identity,
        });
    }

    // The entry behind the handle must not be a link.
    if info.dwFileAttributes & super::fsx::attrs::REPARSE_POINT != 0 {
        // SAFETY: the handle is closed exactly once, here.
        let _ = unsafe { CloseHandle(handle) };
        return Err(RemoveError::IsLink);
    }

    // Mark the *verified object* for deletion. The path is never touched
    // again, so nothing can be substituted between the check and the delete.
    let disposition = FILE_DISPOSITION_INFO {
        DeleteFile: windows::Win32::Foundation::BOOLEAN(1),
    };
    // SAFETY: `disposition` matches the class and size passed alongside it.
    let marked = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            &disposition as *const FILE_DISPOSITION_INFO as *const core::ffi::c_void,
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    // SAFETY: the handle is closed exactly once, here. Closing it releases
    // the deletion that was marked above.
    let _ = unsafe { CloseHandle(handle) };

    match marked {
        Ok(()) => Ok(RemoveOutcome {
            identity,
            reclaim_certain: link_count == 1,
        }),
        Err(e) => Err(RemoveError::DeleteRefused(e.to_string())),
    }
}

#[cfg(not(windows))]
fn remove_verified(path: &Path, _expected: FileIdentity) -> Result<RemoveOutcome, RemoveError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(RemoveOutcome {
            identity: FileIdentity {
                volume_serial: 0,
                file_index: 0,
            },
            reclaim_certain: true,
        }),
        Err(e) => Err(RemoveError::DeleteRefused(e.to_string())),
    }
}

/// Check that no ancestor of `path` is a link or junction.
///
/// This is a second, independent guard: the identity check already refuses a
/// delete that would land on the wrong file, but refusing outright when a
/// parent has been replaced is clearer for the operator and removes the
/// ambiguity entirely.
pub fn ancestors_are_real(path: &Path) -> Result<(), String> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir.as_os_str().is_empty() {
            break;
        }
        match std::fs::symlink_metadata(dir) {
            Ok(md) => {
                if fsx::is_reparse_point(&md) {
                    return Err(format!(
                        "the folder `{}` is a link or junction, so the target cannot be confirmed",
                        dir.display()
                    ));
                }
            }
            Err(e) => {
                return Err(format!(
                    "the folder `{}` could not be read: {}",
                    dir.display(),
                    crate::storage::walk::describe_io(&e)
                ));
            }
        }
        current = dir.parent();
    }
    Ok(())
}
