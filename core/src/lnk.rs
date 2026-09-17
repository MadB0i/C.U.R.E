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
pub fn analyze(path: &Path) -> Option<LnkInfo> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > MAX_LNK_BYTES {
        return None;
    }
    parse(&bytes)
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
    const VOLUME_AND_LOCAL: u32 = 0x0000_0001 | 0x0000_0010;
    if flags & VOLUME_AND_LOCAL != VOLUME_AND_LOCAL {
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
}
