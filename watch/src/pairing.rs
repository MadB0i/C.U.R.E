//! Token-paired USB launch (P0-1: the watcher must never execute USB code).
//!
//! Threat model:
//! - TRUSTED: the host-installed GUI copy at `%LOCALAPPDATA%\CURE\cure-gui.exe`
//!   (pinned by SHA-256 at install time) and the per-user pairing file next to
//!   it. `%LOCALAPPDATA%` inherits the profile DACL (current user + SYSTEM +
//!   administrators only), so other non-admin users can neither steal the
//!   token nor swap the pinned binary.
//! - UNTRUSTED: every USB stick, including its `.cure-trigger` file and any
//!   `cure-gui.exe` it carries. The trigger only *selects* a drive; it never
//!   supplies code. A copied trigger without the token is worthless.
//! - Pairing is trust-on-first-use: at consented self-install, the media the
//!   operator deliberately launched the watcher from supplies the initial GUI
//!   copy and receives the token. Later media are paired explicitly with
//!   `cure-watch pair E:`.
//! - Residual TOCTOU: the host copy is hashed and *then* spawned, so a writer
//!   with same-user code execution could swap bytes in between. That attacker
//!   already owns the user's data; cross-user swaps are blocked by the profile
//!   DACL and reparse-point planting is refused outright (see below).
//!
//! Launch rule (all must hold, else silent ignore + local log):
//! 1. the drive's trigger parses as `CURE-TRIGGER-V2:<64 hex>` and its token
//!    equals the pinned token (constant-time compare);
//! 2. the host copy exists as a plain file (reparse points refused);
//! 3. the host copy's SHA-256 still matches the pinned hash.
//!
//! The decision itself is the pure [`decide_launch`] below so every branch is
//! unit-testable without touching the filesystem, the registry, or processes.

use std::io;
use std::path::{Path, PathBuf};

/// Name of the trigger file at a drive root.
pub const TRIGGER_FILE_NAME: &str = ".cure-trigger";
/// Trigger format prefix. V1 (`CURE-TRIGGER-V1`, bare constant) is legacy and
/// is always rejected — it carries no token and authenticates nobody.
pub const TRIGGER_V2_PREFIX: &str = "CURE-TRIGGER-V2:";
/// Upper bound on trigger bytes read from a drive. Anything larger is
/// rejected without parsing (a trigger is one short line, not a document).
pub const MAX_TRIGGER_BYTES: usize = 256;
/// Hex length of the 32-byte pairing token.
pub const TOKEN_HEX_LEN: usize = 64;
/// Host directory name under `%LOCALAPPDATA%`.
pub const HOST_DIR_NAME: &str = "CURE";
/// File name of the pinned host GUI copy.
pub const HOST_GUI_EXE_NAME: &str = "cure-gui.exe";
/// File name of the per-user pairing record.
pub const PAIRING_FILE_NAME: &str = "watcher-pairing.json";

/// Pinned per-user pairing: secret token + expected GUI bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    /// Lowercase hex of the 32-byte token (64 chars).
    pub token_hex: String,
    /// Lowercase hex SHA-256 of the pinned host GUI copy (64 chars).
    pub gui_sha256_hex: String,
}

/// Serialize a pairing record (canonical JSON, no whitespace surprises).
pub fn pairing_to_json(pairing: &Pairing) -> String {
    format!(
        "{{\"token_hex\":\"{}\",\"gui_sha256_hex\":\"{}\"}}",
        pairing.token_hex, pairing.gui_sha256_hex
    )
}

/// Strictly parse a pairing record. Shapes are validated: both fields must be
/// present strings of exactly 64 lowercase hex chars. Anything else is `None`
/// (fail closed — a garbled pairing never authorizes a launch).
pub fn pairing_from_json(raw: &str) -> Option<Pairing> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    let token_hex = parsed.get("token_hex")?.as_str()?.to_string();
    let gui_sha256_hex = parsed.get("gui_sha256_hex")?.as_str()?.to_string();
    if !is_lower_hex_64(&token_hex) || !is_lower_hex_64(&gui_sha256_hex) {
        return None;
    }
    Some(Pairing {
        token_hex,
        gui_sha256_hex,
    })
}

fn is_lower_hex_64(s: &str) -> bool {
    s.len() == TOKEN_HEX_LEN
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Render the exact bytes a paired drive's trigger file must contain.
pub fn format_trigger(token_hex: &str) -> String {
    format!("{TRIGGER_V2_PREFIX}{token_hex}\n")
}

/// Strictly parse trigger bytes. Accepts exactly
/// `CURE-TRIGGER-V2:<64 hex>` with at most one trailing LF or CRLF.
/// Returns the lowercased token on success. Rejects: empty input, oversized
/// input, legacy V1 content, wrong prefix, short/long/non-hex tokens, leading
/// whitespace, and any trailing junk after the terminator.
pub fn parse_trigger(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_TRIGGER_BYTES {
        return None;
    }
    let mut text = bytes;
    // Allow a single trailing LF, optionally preceded by one CR — nothing more.
    if text.ends_with(b"\n") {
        text = &text[..text.len() - 1];
    }
    if text.ends_with(b"\r") {
        text = &text[..text.len() - 1];
    }
    let text = std::str::from_utf8(text).ok()?;
    let token = text.strip_prefix(TRIGGER_V2_PREFIX)?;
    if token.len() != TOKEN_HEX_LEN || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(token.to_ascii_lowercase())
}

/// Constant-time byte equality (no early exit on first difference, so the
/// comparison time does not reveal how many leading bytes matched).
/// Lengths are not secret — token length is fixed — so length mismatch is a
/// plain `false`.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Filesystem state of the host-installed GUI copy, gathered by the caller so
/// the launch decision stays pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostState {
    /// The pinned path exists and is a file.
    pub exists: bool,
    /// Its SHA-256 matches the pinned hash.
    pub sha256_matches_pin: bool,
    /// It is a symlink / reparse point (never launch these).
    pub is_reparse_point: bool,
}

/// Outcome of the launch decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchDecision {
    /// Launch this host path (absolute, no shell) with the drive as data dir.
    Launch { host_exe: PathBuf },
    /// Do nothing. Silent to the console; the caller logs `reason` locally.
    Ignore { reason: &'static str },
}

/// Pure launch decision. Inputs: raw trigger bytes (`None` = unreadable or
/// missing file), the pinned token, the host path to launch, and the host
/// state. Never touches IO, never spawns.
pub fn decide_launch(
    trigger_bytes: Option<&[u8]>,
    pinned_token_hex: &str,
    host_exe: &Path,
    host: &HostState,
) -> LaunchDecision {
    let Some(bytes) = trigger_bytes else {
        return LaunchDecision::Ignore {
            reason: "no trigger file on drive; ignoring",
        };
    };
    let Some(token) = parse_trigger(bytes) else {
        return LaunchDecision::Ignore {
            reason: "malformed trigger file; ignoring",
        };
    };
    if !constant_time_eq(token.as_bytes(), pinned_token_hex.as_bytes()) {
        return LaunchDecision::Ignore {
            reason: "trigger token mismatch; ignoring",
        };
    };
    if !host.exists {
        return LaunchDecision::Ignore {
            reason: "host GUI copy missing; pairing incomplete, ignoring",
        };
    }
    if host.is_reparse_point {
        return LaunchDecision::Ignore {
            reason: "host GUI copy is a reparse point; refusing to launch",
        };
    }
    if !host.sha256_matches_pin {
        return LaunchDecision::Ignore {
            reason: "host GUI copy hash mismatch; refusing to launch",
        };
    }
    LaunchDecision::Launch {
        host_exe: host_exe.to_path_buf(),
    }
}

/// Bounded trigger read: at most `MAX_TRIGGER_BYTES + 1` bytes are pulled so
/// a hostile multi-gigabyte file at the drive root cannot OOM the watcher.
/// Returns `None` when the file is missing, unreadable, or oversized
/// (oversized is malformed by construction — see [`parse_trigger`]).
pub fn read_trigger_bytes(drive_root: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(drive_root.join(TRIGGER_FILE_NAME)).ok()?;
    let mut buf = Vec::new();
    file.take((MAX_TRIGGER_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() > MAX_TRIGGER_BYTES {
        return None;
    }
    Some(buf)
}

/// True when `path` is a symlink / reparse point (checked via
/// `symlink_metadata`, which never follows the link). Missing paths are
/// "not a reparse point" — absence is handled by the `exists` flag instead.
pub fn is_reparse_point(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Host directory for the pinned copy + pairing record. `None` off Windows
/// or when `%LOCALAPPDATA%` is unset (portable mode: no pairing possible).
#[cfg(target_os = "windows")]
pub fn host_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(HOST_DIR_NAME))
}

/// Absolute path of the pinned host GUI copy (`None` when unpaired-able).
#[cfg(target_os = "windows")]
pub fn host_gui_exe() -> Option<PathBuf> {
    host_dir().map(|dir| dir.join(HOST_GUI_EXE_NAME))
}

/// Path of the pairing record (`None` when unpaired-able).
#[cfg(target_os = "windows")]
pub fn pairing_path() -> Option<PathBuf> {
    host_dir().map(|dir| dir.join(PAIRING_FILE_NAME))
}

/// Load and strictly validate the pairing record. `None` = missing,
/// unreadable, or malformed (all treated as "not paired").
#[cfg(target_os = "windows")]
pub fn load_pairing() -> Option<Pairing> {
    let path = pairing_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    pairing_from_json(&raw)
}

/// Persist the pairing record. The file inherits `%LOCALAPPDATA%`'s DACL
/// (current user + SYSTEM + administrators); no extra ACL code is needed or
/// attempted — hand-rolled DACL edits are a bug farm and the profile
/// directory already provides the required user-only boundary.
#[cfg(target_os = "windows")]
pub fn save_pairing(pairing: &Pairing) -> io::Result<()> {
    let path = pairing_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "LOCALAPPDATA not set; cannot store pairing",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, pairing_to_json(pairing))
}

/// 32 random bytes from the OS CSPRNG, lowercase hex.
/// Windows-only: the watcher only performs its duty on Windows, and falling
/// back to a weaker source off-platform would silently downgrade security.
#[cfg(target_os = "windows")]
pub fn generate_token_hex() -> io::Result<String> {
    use windows::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let mut bytes = [0u8; 32];
    // SAFETY: `bytes` is a live 32-byte buffer for the duration of the call;
    // NULL handle + SYSTEM_PREFERRED_RNG selects the system CSPRNG with no
    // algorithm handle to manage or close.
    unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .map_err(|e| io::Error::other(format!("CSPRNG failed: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Drive root (`X:\`) containing `path`, when it exists on disk.
/// Used to pair the media the watcher was launched from — never hardcoded.
#[cfg(target_os = "windows")]
pub fn drive_root_of(path: &Path) -> Option<PathBuf> {
    let root = path.ancestors().last()?;
    if root.is_dir() {
        Some(root.to_path_buf())
    } else {
        None
    }
}

/// True when `root` is a removable drive (the only kind auto-paired at
/// install time — see `ensure_pairing` in main.rs).
#[cfg(target_os = "windows")]
pub fn is_removable_drive(root: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    /// `DRIVE_REMOVABLE` from winbase.h (`GetDriveTypeW` returns a plain
    /// `u32`; the constant lives outside the `windows` 0.58 projection used
    /// here, so it is spelled out with its documented value).
    const DRIVE_REMOVABLE: u32 = 2;
    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; GetDriveTypeW
    // only reads it and holds no state.
    unsafe { GetDriveTypeW(windows::core::PCWSTR(wide.as_ptr())) == DRIVE_REMOVABLE }
}

/// Parse `cure-watch pair <drive>` arguments (pure: no IO).
/// Accepts `E:` or `E:\` (any ASCII letter, either separator style) and
/// normalizes to `E:\`. Rejects everything else — UNC paths, subdirectories,
/// bare words — so a typo can never stamp the token somewhere unexpected.
pub fn parse_pair_drive(args: &[String]) -> Option<PathBuf> {
    if args.len() != 2 || args[0] != "pair" {
        return None;
    }
    let raw = args[1].replace('/', "\\");
    let trimmed = raw.strip_suffix('\\').unwrap_or(&raw);
    if trimmed.len() != 2
        || !trimmed.as_bytes()[0].is_ascii_alphabetic()
        || trimmed.as_bytes()[1] != b':'
    {
        return None;
    }
    let mut root = String::from(trimmed);
    root.push('\\');
    Some(PathBuf::from(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(byte: u8) -> String {
        [byte; 32].iter().map(|b| format!("{b:02x}")).collect()
    }

    fn host_ok() -> HostState {
        HostState {
            exists: true,
            sha256_matches_pin: true,
            is_reparse_point: false,
        }
    }

    fn launch_with(trigger: &[u8], pinned: &str) -> LaunchDecision {
        decide_launch(
            Some(trigger),
            pinned,
            Path::new(r"C:\host\CURE\cure-gui.exe"),
            &host_ok(),
        )
    }

    #[test]
    fn exact_v2_trigger_launches() {
        let pinned = token(0xAB);
        let bytes = format_trigger(&pinned);
        assert_eq!(
            launch_with(bytes.as_bytes(), &pinned),
            LaunchDecision::Launch {
                host_exe: PathBuf::from(r"C:\host\CURE\cure-gui.exe"),
            }
        );
    }

    #[test]
    fn token_compare_is_case_insensitive_hex() {
        // Uppercase hex on the drive still pairs (normalized before compare).
        let pinned = token(0xAB);
        let bytes = format!("{TRIGGER_V2_PREFIX}{}\n", pinned.to_ascii_uppercase());
        assert!(matches!(
            launch_with(bytes.as_bytes(), &pinned),
            LaunchDecision::Launch { .. }
        ));
    }

    #[test]
    fn wrong_token_is_ignored() {
        let decision = launch_with(format_trigger(&token(0x01)).as_bytes(), &token(0x02));
        assert_eq!(
            decision,
            LaunchDecision::Ignore {
                reason: "trigger token mismatch; ignoring",
            }
        );
    }

    #[test]
    fn missing_trigger_file_is_ignored() {
        assert_eq!(
            decide_launch(None, &token(0x01), Path::new("x"), &host_ok()),
            LaunchDecision::Ignore {
                reason: "no trigger file on drive; ignoring",
            }
        );
    }

    #[test]
    fn empty_trigger_is_malformed() {
        assert!(parse_trigger(b"").is_none());
        assert_eq!(
            launch_with(b"", &token(0x01)),
            LaunchDecision::Ignore {
                reason: "malformed trigger file; ignoring",
            }
        );
    }

    #[test]
    fn oversized_trigger_is_rejected() {
        let mut big = format_trigger(&token(0x01)).into_bytes();
        big.resize(MAX_TRIGGER_BYTES + 1, b'A');
        assert!(parse_trigger(&big).is_none());
        assert_eq!(
            launch_with(&big, &token(0x01)),
            LaunchDecision::Ignore {
                reason: "malformed trigger file; ignoring",
            }
        );
    }

    #[test]
    fn legacy_v1_trigger_is_rejected() {
        // The old bare-constant format carries no token and must never launch.
        for content in [
            "CURE-TRIGGER-V1",
            "CURE-TRIGGER-V1\n",
            "CURE-TRIGGER-V1\r\n",
        ] {
            assert!(
                parse_trigger(content.as_bytes()).is_none(),
                "accepted legacy: {content:?}"
            );
        }
    }

    #[test]
    fn malformed_triggers_rejected() {
        let good = token(0x01);
        let mut cases = vec![
            "CURE-TRIGGER-V3:".to_string() + &good,   // wrong version
            "CURE-TRIGGER-V2:".to_string(),           // missing token
            format!("{TRIGGER_V2_PREFIX}abc"),        // short token
            format!("{TRIGGER_V2_PREFIX}{}00", good), // long token
            format!("{TRIGGER_V2_PREFIX}{}", "zz".repeat(32)), // non-hex
            format!(" CURE-TRIGGER-V2:{good}"),       // leading space
            format!("\n{TRIGGER_V2_PREFIX}{good}"),   // leading newline
            format!("{TRIGGER_V2_PREFIX}{good}\n\n"), // double terminator
            format!("{TRIGGER_V2_PREFIX}{good} extra"), // trailing junk
            format!("{TRIGGER_V2_PREFIX}{good}\nmore"), // second line
        ];
        // Interior NUL / non-UTF8 must also fail.
        let mut nul = format!("{TRIGGER_V2_PREFIX}{good}").into_bytes();
        nul.push(0);
        assert!(parse_trigger(&nul).is_none());
        assert!(parse_trigger(&[0xFF, 0xFE, 0x41]).is_none());
        for case in cases.drain(..) {
            assert!(
                parse_trigger(case.as_bytes()).is_none(),
                "accepted malformed: {case:?}"
            );
        }
    }

    #[test]
    fn crlf_terminator_accepted() {
        let pinned = token(0x07);
        let bytes = format!("{TRIGGER_V2_PREFIX}{pinned}\r\n");
        assert!(matches!(
            launch_with(bytes.as_bytes(), &pinned),
            LaunchDecision::Launch { .. }
        ));
    }

    #[test]
    fn missing_host_exe_is_ignored() {
        let pinned = token(0x01);
        let decision = decide_launch(
            Some(format_trigger(&pinned).as_bytes()),
            &pinned,
            Path::new("host.exe"),
            &HostState {
                exists: false,
                sha256_matches_pin: false,
                is_reparse_point: false,
            },
        );
        assert_eq!(
            decision,
            LaunchDecision::Ignore {
                reason: "host GUI copy missing; pairing incomplete, ignoring",
            }
        );
    }

    #[test]
    fn spoofed_usb_exe_never_launches() {
        // There is no code path that launches a drive-supplied binary: the
        // only Launch target is the caller-supplied host path. A USB carrying
        // a valid token AND its own cure-gui.exe still yields the host path.
        let pinned = token(0x09);
        let decision = decide_launch(
            Some(format_trigger(&pinned).as_bytes()),
            &pinned,
            Path::new(r"C:\Users\op\AppData\Local\CURE\cure-gui.exe"),
            &host_ok(),
        );
        match decision {
            LaunchDecision::Launch { host_exe } => {
                assert!(!host_exe.to_string_lossy().starts_with("E:"));
                assert!(host_exe.ends_with(HOST_GUI_EXE_NAME));
            }
            LaunchDecision::Ignore { .. } => panic!("valid pairing must launch"),
        }
    }

    #[test]
    fn reparse_point_host_is_refused() {
        let pinned = token(0x01);
        let decision = decide_launch(
            Some(format_trigger(&pinned).as_bytes()),
            &pinned,
            Path::new("host.exe"),
            &HostState {
                exists: true,
                sha256_matches_pin: true,
                is_reparse_point: true,
            },
        );
        assert_eq!(
            decision,
            LaunchDecision::Ignore {
                reason: "host GUI copy is a reparse point; refusing to launch",
            }
        );
    }

    #[test]
    fn hash_mismatch_host_is_refused() {
        let pinned = token(0x01);
        let decision = decide_launch(
            Some(format_trigger(&pinned).as_bytes()),
            &pinned,
            Path::new("host.exe"),
            &HostState {
                exists: true,
                sha256_matches_pin: false,
                is_reparse_point: false,
            },
        );
        assert_eq!(
            decision,
            LaunchDecision::Ignore {
                reason: "host GUI copy hash mismatch; refusing to launch",
            }
        );
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abcd", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abce"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn pairing_json_roundtrips_and_rejects_garbage() {
        let pairing = Pairing {
            token_hex: token(0x11),
            gui_sha256_hex: token(0x22),
        };
        let json = pairing_to_json(&pairing);
        assert_eq!(pairing_from_json(&json), Some(pairing));
        for bad in [
            "",
            "{}",
            "{\"token_hex\":\"abc\"}",
            "{\"token_hex\":\"zz\".repeat(0)}",
            &format!("{{\"token_hex\":\"{}\"}}", "gg".repeat(32)),
            &format!(
                "{{\"token_hex\":\"{}\",\"gui_sha256_hex\":\"short\"}}",
                token(0x11)
            ),
            &format!(
                "{{\"token_hex\":\"{}\",\"gui_sha256_hex\":\"{}\"}}",
                token(0xAB).to_ascii_uppercase(),
                token(0x22)
            ),
        ] {
            assert!(pairing_from_json(bad).is_none(), "accepted: {bad:?}");
        }
    }

    #[test]
    fn format_trigger_shape() {
        let out = format_trigger(&token(0x5A));
        assert!(out.starts_with(TRIGGER_V2_PREFIX));
        assert!(out.ends_with('\n'));
        assert_eq!(out.len(), TRIGGER_V2_PREFIX.len() + TOKEN_HEX_LEN + 1);
        assert_eq!(parse_trigger(out.as_bytes()), Some(token(0x5A)));
    }

    #[test]
    fn pair_drive_args_parse() {
        assert_eq!(
            parse_pair_drive(&["cure-watch".into(), "pair".into(), "E:".into()]),
            None, // program name must not be included
        );
        assert_eq!(
            parse_pair_drive(&["pair".into(), "E:".into()]),
            Some(PathBuf::from("E:\\"))
        );
        assert_eq!(
            parse_pair_drive(&["pair".into(), "e:\\".into()]),
            Some(PathBuf::from("e:\\"))
        );
        assert_eq!(
            parse_pair_drive(&["pair".into(), "E:/".into()]),
            Some(PathBuf::from("E:\\"))
        );
        for bad in [
            vec!["pair".to_string()],
            vec!["pair".to_string(), "".to_string()],
            vec!["pair".to_string(), "E:\\sub".to_string()],
            vec!["pair".to_string(), "\\\\srv\\share".to_string()],
            vec!["pair".to_string(), "CURE-TRIGGER-V1".to_string()],
            vec!["watch".to_string(), "E:".to_string()],
        ] {
            assert!(parse_pair_drive(&bad).is_none(), "accepted: {bad:?}");
        }
    }

    #[test]
    fn read_trigger_rejects_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let big = "A".repeat(MAX_TRIGGER_BYTES + 10);
        std::fs::write(dir.path().join(TRIGGER_FILE_NAME), &big).unwrap();
        assert!(read_trigger_bytes(dir.path()).is_none());
        assert!(read_trigger_bytes(Path::new("Z:/definitely/not/a/drive")).is_none());
    }

    #[test]
    fn read_trigger_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pinned = token(0x3C);
        std::fs::write(dir.path().join(TRIGGER_FILE_NAME), format_trigger(&pinned)).unwrap();
        let bytes = read_trigger_bytes(dir.path()).unwrap();
        assert_eq!(parse_trigger(&bytes), Some(pinned));
    }

    #[test]
    fn reparse_point_probe_on_plain_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain.exe");
        std::fs::write(&file, b"bytes").unwrap();
        assert!(!is_reparse_point(&file));
        assert!(!is_reparse_point(&dir.path().join("missing.exe")));
    }
}
