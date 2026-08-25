//! Suspicious-fullscreen-overlay matching (pure decision logic).
//!
//! The OS glue (EnumWindows, styles, process paths, closing/terminating)
//! lives in the GUI; this module only decides WHO should be closed, given a
//! fully-populated [`WindowDesc`] plus the owner's signature verdict. That
//! split keeps every rule here unit-testable without touching real windows.
//!
//! The matcher is deliberately conservative — ALL conditions must hold at
//! once (AND, not OR) so legitimate fullscreen apps (games, video calls,
//! presentations) are never touched:
//!
//! 1. the window is topmost (`WS_EX_TOPMOST`),
//! 2. the window is borderless/undecorated (no `WS_CAPTION` — real ransom
//!    overlays are borderless; legit fullscreen apps usually are not),
//! 3. the owning process's binary does NOT have a valid Authenticode
//!    signature (unsigned OR invalid — an invalid signature is worse),
//! 4. it is not C.U.R.E's own window and not a known system window
//!    (taskbar/shell — pre-classified by the glue layer).

use std::path::PathBuf;

use crate::signature::SignatureStatus;

/// Everything the decision needs to know about one top-level window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDesc {
    pub title: String,
    /// Full path of the process that owns the window.
    pub process_path: PathBuf,
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
}

/// Would this window be closed by the overlay-dismissal pass?
/// All four signals must line up (see module docs).
pub fn is_suspicious_overlay(desc: &WindowDesc, signature: &SignatureStatus) -> bool {
    if desc.is_own_process || desc.is_system_window {
        return false;
    }
    desc.is_topmost && desc.is_borderless && *signature != SignatureStatus::ValidSigned
}

/// Filter a pre-assembled list down to the indices of windows that should
/// be closed. Keeping this as (desc, signature) pairs makes the batch
/// decision trivially testable.
pub fn pick_overlays(windows: &[(WindowDesc, SignatureStatus)]) -> Vec<usize> {
    windows
        .iter()
        .enumerate()
        .filter(|(_, (desc, sig))| is_suspicious_overlay(desc, sig))
        .map(|(idx, _)| idx)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_desc() -> WindowDesc {
        WindowDesc {
            title: "LOCKED".to_string(),
            process_path: PathBuf::from(r"C:\Users\bob\AppData\Local\Temp\evillock.exe"),
            is_topmost: true,
            is_borderless: true,
            is_own_process: false,
            is_system_window: false,
        }
    }

    #[test]
    fn classic_ransom_overlay_matches() {
        // topmost + borderless + unsigned: the textbook case
        assert!(is_suspicious_overlay(&base_desc(), &SignatureStatus::Unsigned));
    }

    #[test]
    fn invalid_signature_also_matches() {
        assert!(is_suspicious_overlay(&base_desc(), &SignatureStatus::Invalid));
    }

    #[test]
    fn unverifiable_signature_counts_as_not_valid() {
        assert!(is_suspicious_overlay(&base_desc(), &SignatureStatus::Unknown));
    }

    #[test]
    fn validly_signed_overlay_is_left_alone() {
        // e.g. a legitimate kiosk/lock tool that happens to be topmost+borderless
        assert!(!is_suspicious_overlay(&base_desc(), &SignatureStatus::ValidSigned));
    }

    #[test]
    fn decorated_window_never_matches() {
        let mut d = base_desc();
        d.is_borderless = false; // has a normal title bar (games with borders, etc.)
        assert!(!is_suspicious_overlay(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn non_topmost_window_never_matches() {
        let mut d = base_desc();
        d.is_topmost = false; // a plain borderless window behind everything
        assert!(!is_suspicious_overlay(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn own_process_is_always_excluded() {
        let mut d = base_desc();
        d.is_own_process = true;
        assert!(!is_suspicious_overlay(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn system_windows_are_always_excluded() {
        // taskbar: topmost + borderless but owned by explorer
        let mut d = base_desc();
        d.title = "".to_string();
        d.process_path = PathBuf::from(r"C:\Windows\explorer.exe");
        d.is_system_window = true;
        assert!(!is_suspicious_overlay(&d, &SignatureStatus::Unsigned));
    }

    #[test]
    fn pick_overlays_returns_only_matching_indices() {
        let mut taskbar = base_desc();
        taskbar.process_path = PathBuf::from(r"C:\Windows\explorer.exe");
        taskbar.is_system_window = true;

        let mut game = base_desc();
        game.title = "Full Screen Game".to_string();
        game.is_borderless = false;

        let list = vec![
            (base_desc(), SignatureStatus::Unsigned),            // 0: close
            (taskbar, SignatureStatus::Unsigned),                // 1: system, skip
            (game, SignatureStatus::Unsigned),                   // 2: decorated, skip
            (base_desc(), SignatureStatus::ValidSigned),         // 3: signed, skip
            (base_desc(), SignatureStatus::Invalid),             // 4: close
        ];
        assert_eq!(pick_overlays(&list), vec![0, 4]);
    }

    #[test]
    fn process_name_extracts_file_name() {
        assert_eq!(base_desc().process_name(), "evillock.exe");
    }
}
