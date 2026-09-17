//! Elevation + per-scanner availability reporting.
//!
//! Every scanner result should say HOW it was obtained, not just what it
//! found: AVAILABLE (full read), PARTIAL (some locations skipped, counted),
//! UNAVAILABLE (wrong OS), CHECK FAILED (unexpected error), ACCESS DENIED
//! (privilege boundary). Dashboards must render these states instead of
//! showing "0 entries" as if it were a clean bill of health.

use serde::{Deserialize, Serialize};

/// How completely a source could be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceStatus {
    /// Fully enumerated.
    Available,
    /// Enumerated with skips (counted in `skipped`, reason given).
    Partial { skipped: usize, reason: String },
    /// Cannot run here at all (wrong OS).
    Unavailable { reason: String },
    /// Enumeration itself failed (COM/WMI down, SCM locked, …).
    CheckFailed { reason: String },
    /// The OS refused access (elevation or ACL boundary).
    AccessDenied { reason: String },
}

impl SourceStatus {
    /// Short UI/report token: CHECKED, PARTIAL, UNAVAILABLE, CHECK FAILED,
    /// ACCESS DENIED.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Available => "CHECKED",
            Self::Partial { .. } => "PARTIAL",
            Self::Unavailable { .. } => "UNAVAILABLE",
            Self::CheckFailed { .. } => "CHECK FAILED",
            Self::AccessDenied { .. } => "ACCESS DENIED",
        }
    }

    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Available | Self::Partial { .. })
    }
}

/// True when the current process holds elevated (administrator) rights.
/// Used to explain skips (system task files, HKLM keys) — never to
/// escalate or to change behavior silently.
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        imp::is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub(super) fn is_elevated() -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut returned = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            );
            let _ = CloseHandle(token);
            ok.is_ok() && elevation.TokenIsElevated != 0
        }
    }
}

/// Classify a Windows HRESULT-ish error value for status reporting.
/// `0x80070005` (E_ACCESSDENIED) → access-denied, else failed.
pub fn classify_win_error(code: i32, context: &str) -> SourceStatus {
    const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
    if code == E_ACCESSDENIED {
        SourceStatus::AccessDenied {
            reason: format!("{context}: access denied (elevation may help)"),
        }
    } else {
        SourceStatus::CheckFailed {
            reason: format!("{context}: error 0x{code:08x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_stable() {
        assert_eq!(SourceStatus::Available.label(), "CHECKED");
        assert!(SourceStatus::Available.is_usable());
        assert!(SourceStatus::Partial {
            skipped: 8,
            reason: "x".into()
        }
        .is_usable());
        assert!(!SourceStatus::Unavailable { reason: "x".into() }.is_usable());
        assert!(!SourceStatus::CheckFailed { reason: "x".into() }.is_usable());
        assert!(!SourceStatus::AccessDenied { reason: "x".into() }.is_usable());
    }

    #[test]
    fn access_denied_classification() {
        assert!(matches!(
            classify_win_error(0x8007_0005u32 as i32, "WMI"),
            SourceStatus::AccessDenied { .. }
        ));
        assert!(matches!(
            classify_win_error(-1, "WMI"),
            SourceStatus::CheckFailed { .. }
        ));
    }

    #[test]
    fn elevation_query_never_panics() {
        let _ = is_elevated();
    }
}
