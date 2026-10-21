//! Platform file facts the storage engine needs but `std::fs` does not
//! expose: allocated size, NTFS attributes, reparse points, hard-link counts
//! and free space on a volume.
//!
//! Every function degrades to `None`/`false` rather than failing when the
//! platform will not answer. A missing fact lowers confidence; it never
//! fabricates one.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Windows file attribute bits used by the engine.
pub mod attrs {
    pub const READONLY: u32 = 0x0000_0001;
    pub const HIDDEN: u32 = 0x0000_0002;
    pub const SYSTEM: u32 = 0x0000_0004;
    pub const DIRECTORY: u32 = 0x0000_0010;
    pub const REPARSE_POINT: u32 = 0x0000_0400;
}

#[cfg(windows)]
fn raw_attributes(md: &std::fs::Metadata) -> Option<u32> {
    use std::os::windows::fs::MetadataExt;
    Some(md.file_attributes())
}

#[cfg(not(windows))]
fn raw_attributes(_md: &std::fs::Metadata) -> Option<u32> {
    None
}

/// Raw Windows attribute word, or `None` off Windows.
pub fn file_attributes(md: &std::fs::Metadata) -> Option<u32> {
    raw_attributes(md)
}

/// True when the entry is a symlink, junction, mount point or any other
/// reparse point.
///
/// `Metadata::is_symlink` alone reports only the symlink flavour; junctions
/// and other reparse tags would slip past it. Both checks are required.
pub fn is_reparse_point(md: &std::fs::Metadata) -> bool {
    if md.is_symlink() {
        return true;
    }
    match raw_attributes(md) {
        Some(a) => a & attrs::REPARSE_POINT != 0,
        None => false,
    }
}

pub fn is_read_only(md: &std::fs::Metadata) -> bool {
    match raw_attributes(md) {
        Some(a) => a & attrs::READONLY != 0,
        None => false,
    }
}

pub fn is_hidden(md: &std::fs::Metadata) -> bool {
    match raw_attributes(md) {
        Some(a) => a & attrs::HIDDEN != 0,
        None => false,
    }
}

pub fn is_system(md: &std::fs::Metadata) -> bool {
    match raw_attributes(md) {
        Some(a) => a & attrs::SYSTEM != 0,
        None => false,
    }
}

/// Number of hard links to this file.
///
/// A file with `n > 1` does not free its bytes when one link is removed, so
/// duplicate reclaim estimates must exclude it rather than double-count.
///
/// `MetadataExt::number_of_links` is still unstable on stable Rust, so this
/// opens the file and asks the kernel. Failure returns `None`, which callers
/// treat as "cannot rule out hard links" — never as "there are none".
pub fn hard_link_count(path: &Path) -> Option<u32> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };

        let share = FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0;
        let Ok(file) = std::fs::OpenOptions::new()
            .access_mode(0) // metadata only; no read or write access requested
            .share_mode(share)
            .custom_flags(FILE_READ_ATTRIBUTES.0)
            .open(path)
        else {
            return None;
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` owns a live handle; `info` is a correctly sized
        // out-parameter for the duration of the call.
        let ok = unsafe {
            GetFileInformationByHandle(
                windows::Win32::Foundation::HANDLE(file.as_raw_handle()),
                &mut info,
            )
        };
        if ok.is_err() {
            return None;
        }
        Some(info.nNumberOfLinks)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path)
            .ok()
            .map(|md| md.nlink().max(1) as u32)
    }
}

/// True when another program is holding the file open right now.
///
/// The file is opened with no sharing at all. If the kernel refuses, some
/// other handle is open on it and the bytes are in use. A file that cannot
/// be opened for an unrelated reason — no permission, say — is *not*
/// reported as in use: only a sharing or lock violation counts, because
/// anything else would be a guess.
pub fn is_in_use(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        match std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
        {
            Ok(_) => false,
            Err(err) => {
                let code = err.raw_os_error().unwrap_or(0);
                code == ERROR_SHARING_VIOLATION || code == ERROR_LOCK_VIOLATION
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Bytes the file actually occupies on the volume.
///
/// Falls back to `logical` when the platform will not say, so callers always
/// have a number; the boolean in [`AllocatedSize`] records whether it is real.
pub fn allocated_size(path: &Path, logical: u64) -> AllocatedSize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetCompressedFileSizeW;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut high: u32 = 0;
        // SAFETY: `wide` is NUL-terminated and `high` is a valid out-pointer.
        let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };
        if low == 0 || high == 0xFFFF_FFFF {
            return AllocatedSize {
                bytes: logical,
                exact: false,
            };
        }
        let on_disk = ((high as u64) << 32) | low as u64;
        AllocatedSize {
            bytes: on_disk.max(logical.min(on_disk)),
            exact: true,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        AllocatedSize {
            bytes: logical,
            exact: false,
        }
    }
}

/// A size figure plus whether it was measured or inferred.
#[derive(Debug, Clone, Copy)]
pub struct AllocatedSize {
    pub bytes: u64,
    pub exact: bool,
}

/// Free/total bytes on the volume holding `path`.
///
/// Uses the OS, so the number the UI shows next to "reclaimable" is a real
/// measurement of the same volume rather than a guess.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeSpace {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub label: Option<String>,
}

/// The volume root that contains `path`.
///
/// `GetDiskFreeSpaceExW` takes a *directory root*, not a file path. Handing it
/// `C:\dir\file.txt\` makes the call fail with `ERROR_DIRECTORY`, which is
/// exactly the bug that made every cleanup receipt report an unreadable
/// volume. This resolves the root first.
pub fn volume_root(path: &Path) -> Option<std::path::PathBuf> {
    let raw = path.as_os_str().to_string_lossy().to_string();
    let raw = raw.strip_prefix("\\\\?\\").unwrap_or(&raw);
    let raw = raw.replace('/', "\\");
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("\\\\") {
        // UNC path: \\server\share\... — the root is \\server\share.
        let mut parts = rest.splitn(3, '\\');
        let server = parts.next()?;
        let share = parts.next()?;
        if server.is_empty() || share.is_empty() {
            return None;
        }
        return Some(std::path::PathBuf::from(format!("\\\\{server}\\{share}")));
    }
    let mut chars = raw.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    let colon = chars.next();
    if colon != Some(':') {
        return None;
    }
    Some(std::path::PathBuf::from(format!(
        "{}:\\",
        drive.to_ascii_uppercase()
    )))
}

/// Query free space for the volume containing `path`.
///
/// The path may be a file; the volume root is derived from it.
pub fn volume_space(path: &Path) -> Option<VolumeSpace> {
    let root = volume_root(path)?;
    volume_space_at(&root)
}

/// Query free space for an explicit volume root.
pub fn volume_space_at(root: &Path) -> Option<VolumeSpace> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let mut wide: Vec<u16> = root.as_os_str().encode_wide().collect();
        while wide.last() == Some(&0) {
            wide.pop();
        }
        if wide.is_empty() {
            return None;
        }
        // A bare drive letter is not a directory as far as this API is
        // concerned; it needs the trailing separator.
        if !wide.last().is_some_and(|c| *c == b'\\' as u16) {
            wide.push(b'\\' as u16);
        }
        wide.push(0);

        let mut free_to_caller: u64 = 0;
        let mut total: u64 = 0;
        let mut total_free: u64 = 0;
        // SAFETY: `wide` is NUL-terminated; all out-pointers are valid.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut free_to_caller),
                Some(&mut total),
                Some(&mut total_free),
            )
        }
        .is_ok();
        if !ok {
            return None;
        }
        let label_root: Vec<u16> = wide[..wide.len() - 1].to_vec();
        Some(VolumeSpace {
            total_bytes: total,
            free_bytes: free_to_caller,
            label: volume_label(&label_root),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        None
    }
}

#[cfg(windows)]
fn volume_label(root: &[u16]) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let mut name_buf = [0u16; 261];
    let mut fs_buf = [0u16; 261];
    let mut serial: u32 = 0;
    let mut max_comp: u32 = 0;
    let mut flags: u32 = 0;
    // SAFETY: `root` is NUL-terminated and both buffers are sized for the
    // lengths passed alongside them.
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            Some(&mut name_buf),
            Some(&mut serial),
            Some(&mut max_comp),
            Some(&mut flags),
            Some(&mut fs_buf),
        )
    }
    .is_ok();
    if !ok {
        return None;
    }
    let len = name_buf.iter().position(|c| *c == 0).unwrap_or(0);
    if len == 0 {
        None
    } else {
        Some(String::from_utf16_lossy(&name_buf[..len]))
    }
}

#[cfg(not(windows))]
fn volume_label(_root: &[u16]) -> Option<String> {
    None
}

/// Wall-clock seconds for a `SystemTime`, or `None` if it predates the epoch.
pub fn unix_seconds(t: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    let t = t.ok()?;
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(secs)
}
