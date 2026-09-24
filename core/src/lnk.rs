//! Safe `.lnk` (Shell Link) inspection — MS-SHLLINK subset, no execution.
//!
//! Resolves what a Startup shortcut POINTS at (target, arguments, working
//! directory) without ever launching anything. Only metadata is read:
//! StringData sections, the LinkInfo local path, and the
//! EnvironmentVariableDataBlock target. PIDL (LinkTargetIDList) and
//! distributed-tracking data are deliberately NOT interpreted.
//!
//! The parser is total: truncated, garbage, or hostile input yields `None`,
//! never a panic and never any code execution. Files over 1 MiB are
//! refused (shortcuts are a few KB; anything bigger is not a shortcut).

use std::path::Path;

/// What a shortcut declares. All fields are untrusted display strings.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct LnkInfo {
    /// Resolved target: LinkInfo local path preferred, else the ExtraData
    /// environment-block target, else the relative-path string.
    pub target: Option<String>,
    pub arguments: Option<String>,
    pub working_dir: Option<String>,
    pub name: Option<String>,
}

const MAX_LNK_BYTES: usize = 1024 * 1024;
const SHELL_LINK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

const HAS_NAME: u32 = 0x0000_0004;
const HAS_RELATIVE_PATH: u32 = 0x0000_0008;
const HAS_WORKING_DIR: u32 = 0x0000_0010;
const HAS_ARGUMENTS: u32 = 0x0000_0020;
const HAS_ICON: u32 = 0x0000_0040;
const IS_UNICODE: u32 = 0x0000_0080;
const HAS_LINK_INFO: u32 = 0x0000_0002;
const ENV_BLOCK_SIG: u32 = 0xA000_0001;

/// Read + parse a `.lnk` file. `None` = not parseable (or too big).
///
/// On Windows the shell itself is asked first (`IShellLink::GetPath` with
/// no UI, no update, no search): it resolves the PIDL, environment strings
/// and relative paths exactly as Explorer would, without executing
/// anything. Whatever it returns (absolute or relative) becomes the
/// target; the raw parser below still supplies arguments, working
/// directory and display name, and is the sole source off Windows.
/// If COM resolution fails or yields nothing, the raw parse stands alone —
/// this fallback is load-bearing (COM may be unavailable in some hosted
/// contexts) and is covered by the byte-fixture tests.
pub fn analyze(path: &Path) -> Option<LnkInfo> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > MAX_LNK_BYTES {
        return None;
    }
    let mut info = parse(&bytes)?;
    #[cfg(windows)]
    {
        if let Some(shell_target) = shell_resolve_target(path) {
            if !shell_target.is_empty() {
                info.target = Some(shell_target);
            }
        }
    }
    Some(info)
}

/// Ask the shell for a shortcut's target without UI, update, search, or
/// execution. `None` on any failure — the caller falls back to raw bytes.
///
/// Safety: COM is initialized here (STA); only uninitialized if this call
/// initialized it (`S_OK`). `S_FALSE`/already-initialized apartments are
/// borrowed, never torn down. The output buffer is stack-owned and sized
/// `MAX_PATH`; the shell writes at most that many UTF-16 units plus NUL.
#[cfg(windows)]
fn shell_resolve_target(lnk_path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{
        IShellLinkW, ShellLink, SLR_NOSEARCH, SLR_NOUPDATE, SLR_NO_UI,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if init.is_err() {
            return None;
        }
        let result = (|| -> windows::core::Result<String> {
            let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let persist: IPersistFile = shell.cast()?;
            let l = wide(lnk_path.as_os_str());
            persist.Load(PCWSTR(l.as_ptr()), STGM_READ)?;
            const MAX_PATH: usize = 260;
            let mut buf = vec![0u16; MAX_PATH];
            let mut find_data =
                std::mem::zeroed::<windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW>();
            shell.GetPath(
                &mut buf,
                &mut find_data as *mut _,
                (SLR_NO_UI.0 | SLR_NOUPDATE.0 | SLR_NOSEARCH.0) as u32,
            )?;
            let end = buf.iter().position(|&c| c == 0).unwrap_or(MAX_PATH);
            let short = String::from_utf16_lossy(&buf[..end]);
            if short.is_empty() {
                return Err(windows::core::Error::from_win32());
            }
            // Shell often returns 8.3 short names (CURE_T~1.EXE); expand to
            // long for stable display and for scoring heuristics that need
            // the long name (random-name detection). Best-effort: if the
            // file doesn't exist, keep the short form.
            Ok(to_long_path_best_effort(&short))
        })();
        if init == S_OK {
            // Only our own init gets torn down; a borrowed apartment
            // (S_FALSE / already-initialized) is left exactly as found.
            CoUninitialize();
        }
        result.ok().filter(|s| !s.is_empty())
    }
}

#[cfg(windows)]
fn to_long_path_best_effort(path: &str) -> String {
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

/// Parse shortcut bytes. Pure function — unit-testable without files.
pub fn parse(bytes: &[u8]) -> Option<LnkInfo> {
    if bytes.len() < 76 {
        return None;
    }
    let header_size = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    if header_size != 76 {
        return None;
    }
    if bytes[4..20] != SHELL_LINK_CLSID {
        return None;
    }
    let flags = u32::from_le_bytes(bytes[20..24].try_into().ok()?);
    let unicode = flags & IS_UNICODE != 0;

    let mut info = LnkInfo::default();
    let mut off = 76usize;

    // LinkTargetIDList (u16 size + blob) — skipped, never interpreted.
    let id_size = u16_at(bytes, off)? as usize;
    off = off.checked_add(2)?.checked_add(id_size)?;
    if off > bytes.len() {
        return None;
    }

    // LinkInfo — parse the local base path when present.
    if flags & HAS_LINK_INFO != 0 {
        if let Some(local) = link_info_local_path(bytes, off) {
            info.target = Some(local);
        }
        let info_size = u32_at(bytes, off)? as usize;
        if info_size < 4 {
            return None;
        }
        off = off.checked_add(info_size)?;
        if off > bytes.len() {
            return None;
        }
    }

    // StringData in fixed order: NAME, RELATIVE_PATH, WORKING_DIR, ARGS, ICON.
    let sections = [
        (HAS_NAME, "name"),
        (HAS_RELATIVE_PATH, "relative"),
        (HAS_WORKING_DIR, "workdir"),
        (HAS_ARGUMENTS, "args"),
        (HAS_ICON, "icon"),
    ];
    let mut strings: std::collections::HashMap<&str, String> = Default::default();
    for (flag, key) in sections {
        if flags & flag == 0 {
            continue;
        }
        let (text, next) = counted_string(bytes, off, unicode)?;
        strings.insert(key, text);
        off = next;
    }
    info.name = strings.remove("name").filter(|s| !s.is_empty());
    info.working_dir = strings.remove("workdir").filter(|s| !s.is_empty());
    info.arguments = strings.remove("args").filter(|s| !s.is_empty());
    let relative = strings.remove("relative").filter(|s| !s.is_empty());

    // ExtraData blocks: (u32 size, u32 signature, payload...), terminal 0.
    while off + 8 <= bytes.len() {
        let size = u32_at(bytes, off)? as usize;
        if size < 4 {
            break; // terminal block (size 0) or corrupt — stop, keep parsed data
        }
        if size == 4 {
            break;
        }
        let sig = u32_at(bytes, off + 4)?;
        if sig == ENV_BLOCK_SIG && size >= 0x314 && info.target.is_none() {
            if let Some(target) = env_block_target(bytes, off) {
                info.target = Some(target);
            }
        }
        off = off.checked_add(size)?;
        if off > bytes.len() {
            break;
        }
    }

    if info.target.is_none() {
        info.target = relative;
    }
    if info.target.is_none()
        && info.arguments.is_none()
        && info.working_dir.is_none()
        && info.name.is_none()
    {
        return None; // structurally valid but content-free — not useful
    }
    Some(info)
}

/// LocalBasePath from a LinkInfo blob (ANSI, NUL-terminated).
///
/// MS-SHLLINK §2.1.2 defines only two LinkInfoFlags bits:
/// VolumeIDAndLocalBasePath (0x1) and CommonNetworkRelativeLink (0x2).
/// A set 0x1 bit with a sane LocalBasePathOffset is an authoritative
/// absolute path — notably, shell-written links (Explorer, WScript,
/// IShellLink::Save) carry flags == 0x1 with NO VolumeID payload quirk
/// beyond that, and must be accepted. (An earlier revision demanded a
/// nonexistent 0x10 bit and silently discarded every such absolute path,
/// falling back to the relative string — F-LNK-1.)
fn link_info_local_path(bytes: &[u8], off: usize) -> Option<String> {
    let size = u32_at(bytes, off)? as usize;
    if size < 28 || off + size > bytes.len() {
        return None;
    }
    let header_size = u32_at(bytes, off + 4)? as usize;
    if header_size < 28 {
        return None;
    }
    let flags = u32_at(bytes, off + 8)?;
    const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x0000_0001;
    if flags & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None;
    }
    let path_off = u32_at(bytes, off + 16)? as usize;
    let abs = off.checked_add(path_off)?;
    if abs >= bytes.len() {
        return None;
    }
    // NUL-terminated ANSI within the LinkInfo blob.
    let end = bytes[abs..off + size]
        .iter()
        .position(|&c| c == 0)
        .map(|i| abs + i)
        .unwrap_or(off + size);
    let text = String::from_utf8_lossy(&bytes[abs..end]).into_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn env_block_target(bytes: &[u8], off: usize) -> Option<String> {
    // Layout: size(4) sig(4) TargetAnsi[260] TargetUnicode[260].
    let base = off.checked_add(8)?;
    let uni_off = base.checked_add(260)?;
    if uni_off + 520 > bytes.len() {
        return None;
    }
    let uni = &bytes[uni_off..uni_off + 520];
    let units: Vec<u16> = uni
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&c| c != 0)
        .collect();
    if units.is_empty() {
        // Fall back to the ANSI copy.
        let ansi = &bytes[base..base + 260];
        let end = ansi.iter().position(|&c| c == 0).unwrap_or(260);
        let text = String::from_utf8_lossy(&ansi[..end]).into_owned();
        return if text.is_empty() { None } else { Some(text) };
    }
    Some(String::from_utf16_lossy(&units))
}

/// Length-prefixed string: u16 char count (incl. NUL) + UTF-16LE or ANSI.
fn counted_string(bytes: &[u8], off: usize, unicode: bool) -> Option<(String, usize)> {
    let count = u16_at(bytes, off)? as usize;
    if count == 0 {
        return Some((String::new(), off + 2));
    }
    if unicode {
        let need = count.checked_mul(2)?;
        let end = off.checked_add(2)?.checked_add(need)?;
        if end > bytes.len() {
            return None;
        }
        let units: Vec<u16> = bytes[off + 2..end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let mut text = String::from_utf16_lossy(&units);
        if text.ends_with('\0') {
            text.pop();
        }
        Some((text, end))
    } else {
        let end = off.checked_add(2)?.checked_add(count)?;
        if end > bytes.len() {
            return None;
        }
        let mut text = String::from_utf8_lossy(&bytes[off + 2..end]).into_owned();
        if text.ends_with('\0') {
            text.pop();
        }
        Some((text, end))
    }
}

fn u16_at(bytes: &[u8], off: usize) -> Option<u16> {
    bytes
        .get(off..off + 2)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], off: usize) -> Option<u32> {
    bytes
        .get(off..off + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn fixture_round_trips_target_args_workdir() {
        let target = r"C:\Program Files\CURE-SYNTH-Vendor\updater.exe";
        let bytes = fixtures::minimal_lnk_unicode(
            target,
            "/checknow",
            r"C:\Program Files\CURE-SYNTH-Vendor",
        );
        let info = parse(&bytes).expect("fixture must parse");
        assert_eq!(info.target.as_deref(), Some(target));
        assert_eq!(info.arguments.as_deref(), Some("/checknow"));
        assert_eq!(
            info.working_dir.as_deref(),
            Some(r"C:\Program Files\CURE-SYNTH-Vendor")
        );
        assert_eq!(info.name.as_deref(), Some("CURE-SYNTH-Updater"));
    }

    #[test]
    fn unicode_target_survives() {
        let target = "C:\\cure-synth\\caf\u{00e9}\\caf\u{00e9}.exe";
        let bytes = fixtures::minimal_lnk_unicode(target, "", "");
        let info = parse(&bytes).expect("unicode fixture must parse");
        assert_eq!(info.target.as_deref(), Some(target));
    }

    #[test]
    fn malformed_inputs_are_none() {
        for (label, bytes) in fixtures::malformed_lnks() {
            assert_eq!(
                parse(&bytes),
                None,
                "malformed fixture must not parse: {label}"
            );
        }
    }

    #[test]
    fn truncated_string_data_is_none() {
        let mut bytes = fixtures::minimal_lnk_unicode(r"C:\x.exe", "args", "wd");
        bytes.truncate(90); // mid-StringData
        assert_eq!(parse(&bytes), None);
    }

    #[test]
    fn oversized_file_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.lnk");
        std::fs::write(&path, vec![0x4Cu8; MAX_LNK_BYTES + 1]).unwrap();
        assert_eq!(analyze(&path), None);
    }

    #[test]
    fn analyze_reads_fixture_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updater.lnk");
        let target = r"C:\cure-synth\updater.exe";
        std::fs::write(&path, fixtures::minimal_lnk_unicode(target, "--silent", "")).unwrap();
        let info = analyze(&path).expect("file fixture must parse");
        assert_eq!(info.target.as_deref(), Some(target));
        assert_eq!(info.arguments.as_deref(), Some("--silent"));
    }

    /// Create a real shell shortcut (IShellLink + IPersistFile), the same
    /// code path Explorer/WScript.Shell use. Temp dirs only; the file never
    /// leaves the test dir and is never executed.
    #[cfg(windows)]
    fn create_shell_link(link: &Path, target: &Path, args: &str, workdir: &Path) {
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
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("COM init for shell link fixture");
            let result = (|| -> windows::core::Result<()> {
                let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                let t = wide(target.as_os_str());
                shell.SetPath(PCWSTR(t.as_ptr()))?;
                let a = wide(std::ffi::OsStr::new(args));
                shell.SetArguments(PCWSTR(a.as_ptr()))?;
                let w = wide(workdir.as_os_str());
                shell.SetWorkingDirectory(PCWSTR(w.as_ptr()))?;
                let persist: IPersistFile = shell.cast()?;
                let l = wide(link.as_os_str());
                persist.Save(PCWSTR(l.as_ptr()), BOOL::from(false))?;
                Ok(())
            })();
            CoUninitialize();
            result.expect("IShellLink fixture setup");
        }
    }

    #[cfg(windows)]
    #[test]
    fn shell_created_link_resolves_absolute_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dummy-target.exe");
        std::fs::write(&target, b"MZ dummy").unwrap();
        let link = dir.path().join("probe.lnk");
        create_shell_link(&link, &target, "--test-flag", dir.path());
        let raw = std::fs::read(&link).unwrap();
        let info = parse(&raw).expect("shell link must parse");
        // IShellLink persists the canonical long form while `tempdir()` may
        // hand back an 8.3 short form (e.g. RUNNER~1 on CI). Compare
        // long-canonicalized on both sides so the test is stable.
        let expected_long = to_long_path_best_effort(&target.to_string_lossy());
        assert_eq!(
            info.target.as_deref(),
            Some(expected_long.as_str()),
            "raw parser must prefer the absolute LinkInfo path over the relative string"
        );
    }
}
