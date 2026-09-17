//! Read-only Windows service audit (Detection Engine: startup persistence).
//!
//! Enumerates Win32 services configured to start without user action
//! (Automatic, Automatic-Delayed, Boot, System) via EnumServicesStatusEx +
//! QueryServiceConfig — both read-only rights, no admin needed. Manual and
//! Disabled services cannot cause startup popups and are skipped by design.
//!
//! Detection only: this module never stops, disables, or deletes anything.
//! Non-Windows builds return an empty vec so callers keep one code path.

use crate::model::PersistenceEntry;

/// How the service is configured to start (evidence, not a verdict).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStart {
    Automatic,
    AutomaticDelayed,
    Boot,
    System,
}

impl ServiceStart {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::AutomaticDelayed => "Automatic (Delayed Start)",
            Self::Boot => "Boot",
            Self::System => "System",
        }
    }
}

/// One auto-start service plus the metadata the scorer needs.
#[derive(Debug, Clone)]
pub struct ServiceRecord {
    /// Unified finding: name = service key name, command = raw ImagePath,
    /// location = service registry key path.
    pub entry: PersistenceEntry,
    pub display_name: String,
    pub start_type: ServiceStart,
    /// Current state at scan time (Running/Stopped/…) — evidence only.
    pub state: String,
    /// Account the service runs as (e.g. LocalSystem) — evidence only.
    pub account: String,
    /// Raw `BinaryPathName` as configured (may include args/env vars).
    pub image_path: String,
    /// Hosting process id at scan time, when the SCM reports one.
    /// Lets incident correlation require pid equality for shared images
    /// (svchost) instead of matching every instance with every service.
    pub pid: Option<u32>,
}

pub fn service_key_path(name: &str) -> String {
    format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}")
}

/// Where a service's configured image resolves to — without executing
/// anything. Env vars (`%SystemRoot%`) are expanded; only the leading
/// program token is considered (arguments are scorer evidence, not paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageStatus {
    /// Absolute path that exists on disk.
    Found(std::path::PathBuf),
    /// Absolute path that does not exist — strong evidence (hollowed,
    /// deleted payload, or typo-squat persistence).
    Missing,
    /// Relative / unparseable / empty — no verdict either way.
    Unresolvable,
}

pub fn image_status(image_path: &str) -> ImageStatus {
    let expanded = expand_env_vars(image_path);
    let token = crate::risk::extract_program_path(&expanded);
    let token = token.trim().trim_matches('"');
    if token.is_empty() {
        return ImageStatus::Unresolvable;
    }
    let path = std::path::Path::new(token);
    if !path.is_absolute() {
        return ImageStatus::Unresolvable;
    }
    if path.is_file() {
        ImageStatus::Found(path.to_path_buf())
    } else {
        ImageStatus::Missing
    }
}

/// Expand `%VAR%` segments on Windows; identity elsewhere (tests stay
/// platform-independent).
pub fn expand_env_vars(text: &str) -> String {
    #[cfg(windows)]
    {
        imp_expand(text)
    }
    #[cfg(not(windows))]
    {
        text.to_string()
    }
}

#[cfg(windows)]
fn imp_expand(text: &str) -> String {
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let needed = ExpandEnvironmentStringsW(windows::core::PCWSTR(wide.as_ptr()), None);
        if needed == 0 || needed > 32 * 1024 {
            return text.to_string();
        }
        let mut buf = vec![0u16; needed as usize];
        let written =
            ExpandEnvironmentStringsW(windows::core::PCWSTR(wide.as_ptr()), Some(&mut buf));
        if written == 0 || written as usize > buf.len() {
            return text.to_string();
        }
        String::from_utf16_lossy(&buf[..written as usize - 1])
    }
}

#[cfg(windows)]
pub fn scan() -> Vec<ServiceRecord> {
    scan_report().records
}

/// Scan result with access accounting. An SCM open failure (locked-down
/// box) is CHECK FAILED / ACCESS DENIED — never an empty "clean".
pub struct ServiceScanReport {
    pub records: Vec<ServiceRecord>,
    /// Services whose configuration could not be read (query failures).
    /// Manual/Disabled services are excluded by design and NOT counted.
    pub skipped_config: usize,
    pub status: crate::elevation::SourceStatus,
}

#[cfg(windows)]
pub fn scan_report() -> ServiceScanReport {
    use crate::elevation::SourceStatus;
    match imp::scan_inner() {
        Ok((records, skipped_config)) => {
            let status = if skipped_config == 0 {
                SourceStatus::Available
            } else {
                SourceStatus::Partial {
                    skipped: skipped_config,
                    reason: "some service configurations unreadable".to_string(),
                }
            };
            ServiceScanReport {
                records,
                skipped_config,
                status,
            }
        }
        Err(code) => ServiceScanReport {
            records: Vec::new(),
            skipped_config: 0,
            status: crate::elevation::classify_win_error(code, "service control manager"),
        },
    }
}

#[cfg(not(windows))]
pub fn scan_report() -> ServiceScanReport {
    ServiceScanReport {
        records: Vec::new(),
        skipped_config: 0,
        status: crate::elevation::SourceStatus::Unavailable {
            reason: "Windows-only source".to_string(),
        },
    }
}

#[cfg(not(windows))]
pub fn scan() -> Vec<ServiceRecord> {
    Vec::new()
}

#[cfg(windows)]
mod imp {
    use windows::core::PCWSTR;
    use windows::Win32::System::Services::{
        CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW,
        QueryServiceConfig2W, QueryServiceConfigW, ENUM_SERVICE_STATUS_PROCESSW,
        QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_ENUMERATE_SERVICE,
        SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_DELAYED_AUTO_START_INFO,
        SERVICE_QUERY_CONFIG, SERVICE_STATE_ALL, SERVICE_WIN32,
    };

    use super::{service_key_path, ServiceRecord, ServiceStart};
    use crate::model::{PersistenceEntry, PersistenceSource};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_str(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        }
    }

    fn service_state(code: u32) -> &'static str {
        match code {
            1 => "Stopped",
            2 => "Start Pending",
            3 => "Stop Pending",
            4 => "Running",
            5 => "Continue Pending",
            6 => "Pause Pending",
            7 => "Paused",
            _ => "Unknown",
        }
    }

    pub(super) fn scan_inner() -> Result<(Vec<ServiceRecord>, usize), i32> {
        let mut out = Vec::new();
        let mut skipped_config = 0usize;
        unsafe {
            let scm: SC_HANDLE = match OpenSCManagerW(None, None, SC_MANAGER_ENUMERATE_SERVICE) {
                Ok(h) => h,
                // Locked-down box: no service data — report WHY, not empty.
                Err(e) => return Err(e.code().0),
            };

            // Two-call buffer pattern for the enumeration.
            let mut needed = 0u32;
            let mut returned = 0u32;
            let mut resume: u32 = 0;
            let _ = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                None,
                &mut needed,
                &mut returned,
                Some(&mut resume),
                None,
            );
            if needed == 0 {
                let _ = CloseServiceHandle(scm);
                return Ok((out, skipped_config));
            }
            let mut buf = vec![0u8; needed as usize];
            let ok = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(buf.as_mut_slice()),
                &mut needed,
                &mut returned,
                Some(&mut resume),
                None,
            );
            if let Err(e) = ok {
                let _ = CloseServiceHandle(scm);
                return Err(e.code().0);
            }
            let statuses: &[ENUM_SERVICE_STATUS_PROCESSW] = std::slice::from_raw_parts(
                buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
                returned as usize,
            );

            for st in statuses {
                match describe_service(scm, st) {
                    Ok(Some(rec)) => out.push(rec),
                    Err(()) => skipped_config += 1,
                    Ok(None) => {}
                }
            }
            let _ = CloseServiceHandle(scm);
        }
        out.sort_by(|a, b| a.entry.name.cmp(&b.entry.name));
        Ok((out, skipped_config))
    }

    /// Open one service for config query.
    /// - `Ok(Some)` = auto-start record (in scope).
    /// - `Ok(None)` = Manual/Disabled/empty — out of scope by design,
    ///   NOT a failure and NOT counted as skipped.
    /// - `Err(())` = query failed with read-only rights — counted as
    ///   skipped so the coverage row stays truthful.
    unsafe fn describe_service(
        scm: SC_HANDLE,
        st: &ENUM_SERVICE_STATUS_PROCESSW,
    ) -> Result<Option<ServiceRecord>, ()> {
        use windows::Win32::System::Services::{
            SERVICE_AUTO_START, SERVICE_BOOT_START, SERVICE_SYSTEM_START,
        };

        let name = wide_str(st.lpServiceName.0 as *const u16);
        if name.is_empty() {
            return Ok(None);
        }
        let display = wide_str(st.lpDisplayName.0 as *const u16);
        let state = service_state(st.ServiceStatusProcess.dwCurrentState.0).to_string();
        let wname = wide(&name);
        let Ok(svc) = OpenServiceW(scm, PCWSTR(wname.as_ptr()), SERVICE_QUERY_CONFIG) else {
            return Err(());
        };

        // Two-call QueryServiceConfig.
        let mut needed = 0u32;
        let _ = QueryServiceConfigW(svc, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseServiceHandle(svc);
            return Err(());
        }
        let mut cfg_buf = vec![0u8; needed as usize];
        #[allow(clippy::cast_ptr_alignment)]
        let cfg = &mut *(cfg_buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW);
        if QueryServiceConfigW(svc, Some(cfg), needed, &mut needed).is_err() {
            let _ = CloseServiceHandle(svc);
            return Err(());
        }
        let start_raw = cfg.dwStartType;
        let image_path = wide_str(cfg.lpBinaryPathName.0 as *const u16);
        let account = wide_str(cfg.lpServiceStartName.0 as *const u16);

        // Delayed-auto is a separate flag, best-effort.
        let mut delayed = SERVICE_DELAYED_AUTO_START_INFO {
            fDelayedAutostart: Default::default(),
        };
        let delayed_bytes = std::slice::from_raw_parts_mut(
            &mut delayed as *mut SERVICE_DELAYED_AUTO_START_INFO as *mut u8,
            std::mem::size_of_val(&delayed),
        );
        let mut delayed_len = 0u32;
        let _ = QueryServiceConfig2W(
            svc,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            Some(delayed_bytes),
            &mut delayed_len,
        );
        let _ = CloseServiceHandle(svc);

        let start_type = match start_raw {
            SERVICE_AUTO_START => {
                if delayed.fDelayedAutostart.as_bool() {
                    ServiceStart::AutomaticDelayed
                } else {
                    ServiceStart::Automatic
                }
            }
            SERVICE_BOOT_START => ServiceStart::Boot,
            SERVICE_SYSTEM_START => ServiceStart::System,
            // Demand-start, disabled, and anything unrecognized cannot cause
            // startup popups — out of scope by design.
            _ => return Ok(None),
        };
        if image_path.trim().is_empty() {
            return Ok(None);
        }

        let entry = PersistenceEntry::new(
            PersistenceSource::WindowsService,
            &name,
            &image_path,
            service_key_path(&name),
        );
        Ok(Some(ServiceRecord {
            entry,
            display_name: if display.is_empty() {
                name.clone()
            } else {
                display
            },
            start_type,
            state,
            account,
            image_path,
            pid: if st.ServiceStatusProcess.dwProcessId != 0 {
                Some(st.ServiceStatusProcess.dwProcessId)
            } else {
                None
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_path_shape() {
        assert_eq!(
            service_key_path("wuauserv"),
            r"HKLM\SYSTEM\CurrentControlSet\Services\wuauserv"
        );
    }

    #[test]
    fn start_labels() {
        assert_eq!(ServiceStart::Automatic.label(), "Automatic");
        assert_eq!(
            ServiceStart::AutomaticDelayed.label(),
            "Automatic (Delayed Start)"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_returns_empty() {
        assert!(scan().is_empty());
    }
}
