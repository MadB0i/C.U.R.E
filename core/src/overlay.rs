//! Suspicious-fullscreen-overlay matching (pure decision logic).
//!
//! The OS glue (EnumWindows, styles, process paths, rects, closing/
//! terminating) lives in the GUI; this module only decides WHO should be
//! shown for confirmation, given fully-populated [`OverlayCandidate`]s plus
//! the user's allowlist. That split keeps every rule here unit-testable
//! without touching real windows.
//!
//! The matcher is deliberately conservative — ALL conditions must hold at
//! once (AND, not OR) for a window to even be *shown*:
//!
//! 1. the window is topmost (`WS_EX_TOPMOST`),
//! 2. the window is borderless/undecorated (no `WS_CAPTION` — real ransom
//!    overlays are borderless; legit fullscreen apps usually are not),
//! 3. the owning process's binary does NOT have a valid Authenticode
//!    signature (unsigned OR invalid — an invalid signature is worse),
//! 4. it is not C.U.R.E's own window and not a known system window
//!    (taskbar/shell — pre-classified by the glue layer),
//! 5. it covers a large fraction of its monitor (ransom lock screens are
//!    fullscreen; small palettes/splashes are not) — see
//!    [`OVERLAY_COVERAGE_THRESHOLD`],
//! 6. its (path, binary hash) is not on the user's persistent allowlist.
//!
//! Matching is NOT closing: the GUI shows every match with process name,
//! path, PID, signature state and rect, and closes only what the operator
//! confirms per window (graceful `WM_CLOSE` by default, explicit
//! per-window "Force close" for termination).

use std::path::{Path, PathBuf};

use crate::signature::SignatureStatus;

/// Fraction of the containing monitor a window must cover to qualify.
/// Ransom lock screens are fullscreen (≈1.0); small topmost palettes,
/// splash screens and widgets sit far below. Named (not magic) so the
/// tradeoff is visible: lowering it catches more, at more false-positive
/// risk; raising it may miss partial-screen lockers.
pub const OVERLAY_COVERAGE_THRESHOLD: f64 = 0.90;

/// Pixel rectangle (left, top, right, bottom) in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WindowRect {
    /// Area in pixels, clamped at zero (degenerate/inverted rects count
    /// as covering nothing, never as covering everything).
    pub fn area(&self) -> u64 {
        let w = (self.right as i64 - self.left as i64).max(0) as u64;
        let h = (self.bottom as i64 - self.top as i64).max(0) as u64;
        w.saturating_mul(h)
    }
}

/// Everything the decision needs to know about one top-level window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDesc {
    pub title: String,
    /// Full path of the process that owns the window.
    pub process_path: PathBuf,
    /// OS process id of the owner (display + targeted close).
    pub pid: u32,
    pub rect: WindowRect,
    /// Size of the monitor containing (most of) the window, in pixels.
    pub monitor: (u32, u32),
    pub is_topmost: bool,
    /// true = no standard caption/title bar (borderless).
    pub is_borderless: bool,
    /// true = this window belongs to cure-gui itself; never a candidate.
    pub is_own_process: bool,
    /// true = owned by a known system/shell process (taskbar, explorer,
    /// etc.) — excluded regardless of signature verdict.
    pub is_system_window: bool,
}

impl WindowDesc {
    pub fn process_name(&self) -> String {
        self.process_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Window area as a fraction of the containing monitor (0.0 when the
    /// monitor size is degenerate). May exceed 1.0 for multi-monitor
    /// straddlers — the threshold comparison still does the right thing.
    pub fn coverage_ratio(&self) -> f64 {
        let mon = self.monitor.0 as u64 * self.monitor.1 as u64;
        if mon == 0 {
            return 0.0;
        }
        self.rect.area() as f64 / mon as f64
    }
}

/// One user-allowlist entry: "don't ask again for this app". Both halves
/// must match — path alone is re-pointable, hash alone is unreadable, so
/// the pair pins the exact binary the operator approved. Serialized as-is
/// into the GUI's local `overlay-allowlist.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AllowEntry {
    /// Full binary path, compared normalized (lowercase, `/` separators).
    pub path: String,
    /// Lowercase hex SHA-256 of the approved binary bytes.
    pub sha256: String,
}

impl AllowEntry {
    pub fn new(path: &str, sha256: &str) -> Self {
        Self {
            path: normalize_allow_path(path),
            sha256: sha256.to_ascii_lowercase(),
        }
    }

    /// True when this entry approves `path` with binary hash `sha256`
    /// (`None` hash never matches — unhashable binaries stay askable).
    pub fn allows(&self, path: &Path, sha256: Option<&str>) -> bool {
        let Some(sha) = sha256 else {
            return false;
        };
        normalize_allow_path(&path.to_string_lossy()) == self.path
            && sha.to_ascii_lowercase() == self.sha256
    }
}

fn normalize_allow_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// One matchable candidate: the window, its signature verdict, and its
/// binary hash (for allowlist comparison; `None` when unhashable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayCandidate {
    pub desc: WindowDesc,
    pub signature: SignatureStatus,
    pub binary_sha256: Option<String>,
}

/// Would this window be shown for confirmation by the overlay pass?
/// All signals must line up (see module docs). Unattributable windows
/// (no PID / no owner path — the glue normally skips these before matching)
/// never match, as defense in depth: without an owner there is nothing
/// meaningful to show or confirm.
pub fn is_suspicious_overlay(
    desc: &WindowDesc,
    signature: &SignatureStatus,
    binary_sha256: Option<&str>,
    allowlist: &[AllowEntry],
) -> bool {
    if desc.is_own_process || desc.is_system_window {
        return false;
    }
    if desc.pid == 0 || desc.process_path.as_os_str().is_empty() {
        return false;
    }
    if !(desc.is_topmost && desc.is_borderless && *signature != SignatureStatus::ValidSigned) {
        return false;
    }
    if desc.coverage_ratio() < OVERLAY_COVERAGE_THRESHOLD {
        return false;
    }
    if allowlist
        .iter()
        .any(|entry| entry.allows(&desc.process_path, binary_sha256))
    {
        return false;
    }
    true
}

/// Filter pre-assembled candidates down to the indices the GUI should show
/// for per-window confirmation. Keeping this as slices makes the batch
/// decision trivially testable.
pub fn pick_overlays(candidates: &[OverlayCandidate], allowlist: &[AllowEntry]) -> Vec<usize> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            is_suspicious_overlay(&c.desc, &c.signature, c.binary_sha256.as_deref(), allowlist)
        })
        .map(|(idx, _)| idx)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fullscreen 1080p window on a 1080p monitor: coverage 1.0.
    fn full_rect() -> (WindowRect, (u32, u32)) {
        (
            WindowRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            (1920, 1080),
        )
    }

    fn base_desc() -> WindowDesc {
        let (rect, monitor) = full_rect();
        WindowDesc {
            title: "LOCKED".to_string(),
            process_path: PathBuf::from(r"C:\Users\bob\AppData\Local\Temp\evillock.exe"),
            pid: 4242,
            rect,
            monitor,
            is_topmost: true,
            is_borderless: true,
            is_own_process: false,
            is_system_window: false,
        }
    }

    fn candidate(desc: WindowDesc, sig: SignatureStatus) -> OverlayCandidate {
        OverlayCandidate {
            desc,
            signature: sig,
            binary_sha256: Some("aa".repeat(32)),
        }
    }

    fn is_match(desc: &WindowDesc, sig: &SignatureStatus) -> bool {
        is_suspicious_overlay(desc, sig, Some(&"aa".repeat(32)), &[])
    }

    #[test]
    fn classic_ransom_overlay_matches() {
        // topmost + borderless + unsigned + fullscreen: the textbook case
        assert!(is_match(&base_desc(), &SignatureStatus::Unsigned));
    }

    #[test]
    fn invalid_signature_also_matches() {
        assert!(is_match(&base_desc(), &SignatureStatus::Invalid));
    }

    #[test]
    fn unverifiable_signature_counts_as_not_valid() {
        assert!(is_match(&base_desc(), &SignatureStatus::Unknown));
    }

    #[test]
    fn revocation_unverified_counts_as_not_valid() {
        assert!(is_match(
            &base_desc(),
            &SignatureStatus::ValidRevocationUnknown
        ));
    }

    #[test]
    fn validly_signed_overlay_is_left_alone() {
        // e.g. a legitimate kiosk/lock tool that happens to be fullscreen
        assert!(!is_match(&base_desc(), &SignatureStatus::ValidSigned));
    }

    #[test]
    fn decorated_window_never_matches() {
        let mut d = base_desc();
        d.is_borderless = false; // has a normal title bar (games with borders, etc.)
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn non_topmost_window_never_matches() {
        let mut d = base_desc();
        d.is_topmost = false; // a plain borderless window behind everything
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn own_process_is_always_excluded() {
        let mut d = base_desc();
        d.is_own_process = true;
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn system_windows_are_always_excluded() {
        // taskbar: topmost + borderless but owned by explorer
        let mut d = base_desc();
        d.title = "".to_string();
        d.process_path = PathBuf::from(r"C:\Windows\explorer.exe");
        d.is_system_window = true;
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn small_unsigned_topmost_splash_does_not_match() {
        // Legit splash/palette: topmost + borderless + unsigned, but tiny.
        // This is the F-01-era false positive the coverage gate removes.
        let mut d = base_desc();
        d.title = "CURE_TEST overlay".to_string();
        d.rect = WindowRect {
            left: 200,
            top: 200,
            right: 680,
            bottom: 400,
        };
        assert!(d.coverage_ratio() < 0.25);
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn fullscreen_game_without_allowlist_still_shown() {
        // A fullscreen unsigned game meets every signal: it IS shown — but
        // only shown, for confirmation, never silently closed.
        let mut d = base_desc();
        d.title = "Full Screen Game".to_string();
        d.process_path = PathBuf::from(r"D:\Games\indie.exe");
        assert!(is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn allowlisted_game_is_excluded() {
        let d = {
            let mut d = base_desc();
            d.title = "Full Screen Game".to_string();
            d.process_path = PathBuf::from(r"D:\Games\indie.exe");
            d
        };
        let allow = [AllowEntry::new(r"D:\Games\indie.exe", &"aa".repeat(32))];
        assert!(!is_suspicious_overlay(
            &d,
            &SignatureStatus::Unsigned,
            Some(&"aa".repeat(32)),
            &allow
        ));
    }

    #[test]
    fn allowlist_requires_path_and_hash_together() {
        let d = base_desc();
        // Same path, different binary hash: still shown (binary changed
        // under a whitelisted path — exactly what must NOT auto-pass).
        let allow = [AllowEntry::new(
            r"C:\Users\bob\AppData\Local\Temp\evillock.exe",
            &"bb".repeat(32),
        )];
        assert!(is_suspicious_overlay(
            &d,
            &SignatureStatus::Unsigned,
            Some(&"aa".repeat(32)),
            &allow
        ));
        // Unhashable binary: never allowlisted, always shown.
        let allow_exact = [AllowEntry::new(
            r"C:\Users\bob\AppData\Local\Temp\evillock.exe",
            &"aa".repeat(32),
        )];
        assert!(is_suspicious_overlay(
            &d,
            &SignatureStatus::Unsigned,
            None,
            &allow_exact
        ));
        // Case/separator-insensitive path compare.
        let allow_mixed = [AllowEntry::new(
            r"c:/users/bob/appdata/local/temp/EVILLOCK.exe",
            &"AA".repeat(32),
        )];
        assert!(!is_suspicious_overlay(
            &d,
            &SignatureStatus::Unsigned,
            Some(&"aa".repeat(32)),
            &allow_mixed
        ));
    }

    #[test]
    fn unattributable_windows_never_match() {
        // pid 0 / empty owner path: nothing meaningful to show or confirm.
        // The glue skips these before matching; the guard is defense in depth.
        let mut d = base_desc();
        d.pid = 0;
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
        let mut d = base_desc();
        d.pid = 4242;
        d.process_path = PathBuf::new();
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn coverage_threshold_boundary() {
        assert!((OVERLAY_COVERAGE_THRESHOLD - 0.90).abs() < f64::EPSILON);
        // Exactly 90% of 1000x1000: qualifies (>=).
        let d = WindowDesc {
            rect: WindowRect {
                left: 0,
                top: 0,
                right: 900,
                bottom: 1000,
            },
            monitor: (1000, 1000),
            ..base_desc()
        };
        assert!(is_match(&d, &SignatureStatus::Unsigned));
        // One pixel short: does not qualify.
        let d = WindowDesc {
            rect: WindowRect {
                left: 0,
                top: 0,
                right: 899,
                bottom: 1000,
            },
            monitor: (1000, 1000),
            ..base_desc()
        };
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
        // Degenerate monitor or rect: ratio 0, never matches.
        let d = WindowDesc {
            monitor: (0, 0),
            ..base_desc()
        };
        assert_eq!(d.coverage_ratio(), 0.0);
        assert!(!is_match(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn pick_overlays_returns_only_matching_indices() {
        let mut taskbar = base_desc();
        taskbar.process_path = PathBuf::from(r"C:\Windows\explorer.exe");
        taskbar.is_system_window = true;

        let mut game = base_desc();
        game.title = "Full Screen Game".to_string();
        game.is_borderless = false;

        let mut splash = base_desc();
        splash.title = "tiny splash".to_string();
        splash.rect = WindowRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };

        let list = vec![
            candidate(base_desc(), SignatureStatus::Unsigned), // 0: show
            candidate(taskbar, SignatureStatus::Unsigned),     // 1: system, skip
            candidate(game, SignatureStatus::Unsigned),        // 2: decorated, skip
            candidate(base_desc(), SignatureStatus::ValidSigned), // 3: signed, skip
            candidate(base_desc(), SignatureStatus::Invalid),  // 4: show
            candidate(splash, SignatureStatus::Unsigned),      // 5: small, skip
        ];
        assert_eq!(pick_overlays(&list, &[]), vec![0, 4]);
    }

    #[test]
    fn process_name_extracts_file_name() {
        assert_eq!(base_desc().process_name(), "evillock.exe");
    }
}
