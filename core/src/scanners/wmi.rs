//! Read-only WMI event-subscription audit.
//!
//! Queries `ROOT\subscription` for `__EventFilter`, `__EventConsumer`, and
//! `__FilterToConsumerBinding` instances — the classic fileless persistence
//! mechanism (ATT&CK T1546.003). A clean machine is normally EMPTY here, so
//! any finding is high-signal (review, never a verdict).
//!
//! Safety design:
//! - Read-only WQL `SELECT`s through a forward-only enumerator; no methods
//!   are invoked on any WMI object, ever.
//! - Properties are read via `GetObjectText` (MOF text) and parsed with
//!   bounded string search — no script, query, or consumer payload is ever
//!   executed, expanded, or passed to a shell.
//! - Any COM/WMI failure (service disabled, locked-down box, timeout)
//!   yields an INCOMPLETE result (see [`scan_report`]), never a panic, never
//!   a hard error, and never a bare "0 entries" that reads as clean.
//! - Output is capped (objects per query, text length, total entries).
//!
//! Non-Windows builds report Unavailable.

use crate::model::PersistenceEntry;

pub const WMI_NAMESPACE: &str = r"ROOT\subscription";

pub fn filter_location(name: &str) -> String {
    format!(r"{WMI_NAMESPACE}:__EventFilter.Name={name:?}")
}

#[cfg(windows)]
pub fn scan() -> Vec<PersistenceEntry> {
    scan_report().entries
}

/// Scan result with access accounting. A COM/WMI failure is CHECK FAILED
/// (or ACCESS DENIED on 0x80070005) — an empty vec from a failed query
/// must never read as "clean".
pub struct WmiScanReport {
    pub entries: Vec<PersistenceEntry>,
    pub status: crate::elevation::SourceStatus,
}

#[cfg(windows)]
pub fn scan_report() -> WmiScanReport {
    let (entries, result) = imp::scan();
    combine(entries, result)
}

/// Pure combine step (unit-testable without COM): entries are always kept;
/// only the status reflects the outcome.
#[cfg(windows)]
fn combine(entries: Vec<PersistenceEntry>, result: windows::core::Result<()>) -> WmiScanReport {
    use crate::elevation::SourceStatus;
    match result {
        Ok(()) => WmiScanReport {
            entries,
            status: SourceStatus::Available,
        },
        Err(e) => WmiScanReport {
            entries,
            status: crate::elevation::classify_win_error(
                e.code().0,
                "WMI event subscription query",
            ),
        },
    }
}

#[cfg(not(windows))]
pub fn scan_report() -> WmiScanReport {
    WmiScanReport {
        entries: Vec::new(),
        status: crate::elevation::SourceStatus::Unavailable {
            reason: "Windows-only source".to_string(),
        },
    }
}

#[cfg(not(windows))]
pub fn scan() -> Vec<PersistenceEntry> {
    Vec::new()
}

// MOF property helper shared with the incident observer (command-line
// fetch reuses the same bounded parser as subscription parsing).
#[cfg(windows)]
pub(crate) use imp::mof_prop;

#[cfg(windows)]
mod imp {
    use windows::core::{BSTR, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED, EOLE_AUTHENTICATION_CAPABILITIES, RPC_C_AUTHN_LEVEL_CALL,
        RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::Wmi::{
        IEnumWbemClassObject, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
        WBEM_FLAG_RETURN_IMMEDIATELY,
    };

    use super::{filter_location, WMI_NAMESPACE};
    use crate::model::{PersistenceEntry, PersistenceSource};

    const MAX_OBJECTS_PER_QUERY: usize = 64;
    const MAX_MOF_CHARS: usize = 16 * 1024;
    const MAX_ENTRIES: usize = 128;
    const NEXT_TIMEOUT_MS: i32 = 5000;

    /// COM setup + full enumeration.
    ///
    /// Returns the entries collected so far plus the outcome. Partial results
    /// are KEPT on failure (a denied third query must not discard two good
    /// ones); the caller maps `Err` to an INCOMPLETE status so a failed check
    /// can never read as "0 entries, clean".
    ///
    /// COM apartment notes: S_OK = we initialized and must uninitialize;
    /// S_FALSE = already initialized (same model), proceed without touching
    /// it; RPC_E_CHANGED_MODE = initialized with a different model — WMI
    /// still works via marshaling, so proceed without touching it. Only a
    /// genuine init failure aborts before trying.
    pub(super) fn scan() -> (Vec<PersistenceEntry>, windows::core::Result<()>) {
        use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_OK};
        let mut entries = Vec::new();
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if init.is_err() && init != RPC_E_CHANGED_MODE {
            return (entries, Err(windows::core::Error::from(init)));
        }
        let result = scan_inner(&mut entries);
        // Only our own init gets uninitialized; borrowed apartments
        // (S_FALSE, CHANGED_MODE) are left exactly as found.
        if init == S_OK {
            unsafe { CoUninitialize() };
        }
        (entries, result)
    }

    fn scan_inner(entries: &mut Vec<PersistenceEntry>) -> windows::core::Result<()> {
        let locator: windows::Win32::System::Wmi::IWbemLocator =
            unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)? };
        let server: IWbemServices = unsafe {
            locator.ConnectServer(
                &BSTR::from(WMI_NAMESPACE),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )?
        };
        unsafe {
            CoSetProxyBlanket(
                &server,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOLE_AUTHENTICATION_CAPABILITIES(0),
            )?;
        }

        // Event filters: name + WQL query (the query text itself is evidence;
        // it is never executed by us).
        for mof in exec_texts(
            &server,
            "SELECT Name, Query, EventNamespace FROM __EventFilter",
        )? {
            let Some(name) = mof_prop(&mof, "Name") else {
                continue;
            };
            let query = mof_prop(&mof, "Query").unwrap_or_default();
            push_capped(
                entries,
                PersistenceEntry::new(
                    PersistenceSource::WmiSubscription,
                    format!("WMI filter {name}"),
                    if query.is_empty() {
                        format!("WMI event filter {name}")
                    } else {
                        query
                    },
                    filter_location(&name),
                ),
            );
        }

        // Consumers of any class: identify by instance class + payload.
        // Property priority follows the most-abused consumer types
        // (command-line, script, log) — anything else keeps its class label.
        for mof in exec_texts(&server, "SELECT * FROM __EventConsumer")? {
            let class = mof_class(&mof);
            let payload = mof_prop(&mof, "CommandLineTemplate")
                .or_else(|| mof_prop(&mof, "ExecutablePath"))
                .or_else(|| mof_prop(&mof, "ScriptText").map(|s| truncate(&s, 300)))
                .or_else(|| mof_prop(&mof, "LogName").map(|l| format!("event log: {l}")))
                .unwrap_or_default();
            let label = mof_prop(&mof, "Name").unwrap_or_else(|| class.clone());
            push_capped(
                entries,
                PersistenceEntry::new(
                    PersistenceSource::WmiSubscription,
                    format!("WMI consumer {label}"),
                    if payload.is_empty() {
                        format!("WMI event consumer ({class})")
                    } else {
                        payload
                    },
                    format!(r"{WMI_NAMESPACE}:{class}"),
                ),
            );
        }

        // Bindings tie filters to consumers.
        for mof in exec_texts(
            &server,
            "SELECT Filter, Consumer FROM __FilterToConsumerBinding",
        )? {
            let filter = mof_prop(&mof, "Filter").unwrap_or_default();
            let consumer = mof_prop(&mof, "Consumer").unwrap_or_default();
            if filter.is_empty() && consumer.is_empty() {
                continue;
            }
            push_capped(
                entries,
                PersistenceEntry::new(
                    PersistenceSource::WmiSubscription,
                    "WMI filter→consumer binding",
                    format!("filter {filter} → consumer {consumer}"),
                    format!(r"{WMI_NAMESPACE}:__FilterToConsumerBinding"),
                ),
            );
        }
        Ok(())
    }

    fn push_capped(entries: &mut Vec<PersistenceEntry>, entry: PersistenceEntry) {
        if entries.len() < MAX_ENTRIES {
            entries.push(entry);
        }
    }

    /// Run a WQL SELECT and return each instance as (truncated) MOF text.
    fn exec_texts(server: &IWbemServices, query: &str) -> windows::core::Result<Vec<String>> {
        let mut out = Vec::new();
        let enumerator: IEnumWbemClassObject = unsafe {
            server.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )?
        };
        loop {
            if out.len() >= MAX_OBJECTS_PER_QUERY {
                break;
            }
            let mut objects = [None];
            let mut returned = 0u32;
            let hr = unsafe { enumerator.Next(NEXT_TIMEOUT_MS, &mut objects, &mut returned) };
            if hr.is_err() || returned == 0 {
                break; // end of enumeration or timeout — both are clean stops
            }
            let Some(obj) = objects[0].take() else {
                break;
            };
            let text = match unsafe { obj.GetObjectText(0) } {
                Ok(t) => t,
                Err(_) => continue,
            };
            let mut s = text.to_string();
            if s.len() > MAX_MOF_CHARS {
                s.truncate(MAX_MOF_CHARS);
            }
            out.push(s);
        }
        Ok(out)
    }

    /// `instance of __EventFilter` → `__EventFilter`. Scans all lines:
    /// some providers prefix the text with blank lines.
    pub(super) fn mof_class(mof: &str) -> String {
        for line in mof.lines() {
            if let Some(class) = line.trim().strip_prefix("instance of ") {
                let class = class.trim();
                if !class.is_empty() {
                    return class.to_string();
                }
            }
        }
        "unknown".to_string()
    }

    /// Extract `Prop = "value";` from MOF text with MOF unescaping
    /// (`\"` → `"`, `\\` → `\`). Bounded, allocation-light, never executes.
    /// Crate-visible for the incident observer (event TargetInstance parsing
    /// uses its own tiny parser; command-line fetch reuses this).
    pub(crate) fn mof_prop(mof: &str, prop: &str) -> Option<String> {
        let key = format!("{prop} = \"");
        let start = mof.find(&key)? + key.len();
        let rest = &mof[start..];
        let mut value = String::new();
        let mut chars = rest.chars();
        // Hard cap so a hostile multi-MB string cannot blow memory.
        for _ in 0..MAX_MOF_CHARS {
            let c = chars.next()?;
            if c == '\\' {
                match chars.next()? {
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    other => {
                        value.push('\\');
                        value.push(other);
                    }
                }
            } else if c == '"' {
                return Some(value);
            } else {
                value.push(c);
            }
        }
        None
    }

    fn truncate(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            return s.to_string();
        }
        s.chars().take(max_chars).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_location_shape() {
        assert_eq!(
            filter_location("CURE-TEST-FILTER"),
            r#"ROOT\subscription:__EventFilter.Name="CURE-TEST-FILTER""#
        );
    }

    #[cfg(windows)]
    mod mof_tests {
        use super::super::imp::{mof_class, mof_prop};

        const FILTER_MOF: &str = "instance of __EventFilter\n{\n    EventNamespace = \"root\\\\cimv2\";\n    Name = \"CURE-SYNTH-FILTER-DO-NOT-USE\";\n    Query = \"SELECT * FROM __InstanceModificationEvent WITHIN 60 WHERE TargetInstance ISA 'Win32_PerfFormattedData_PerfOS_System'\";\n    QueryLanguage = \"WQL\";\n};";

        const CONSUMER_MOF: &str = "instance of CommandLineEventConsumer\n{\n    CommandLineTemplate = \"C:\\\\Windows\\\\System32\\\\WindowsPowerShell\\\\v1.0\\\\powershell.exe -enc SQBuAHYAbwBrAGUALQBhAHAAcAA=\";\n    WorkingDirectory = \"C:\\\\Temp\";\n};";

        #[test]
        fn class_line_parses() {
            assert_eq!(mof_class(FILTER_MOF), "__EventFilter");
            assert_eq!(mof_class(CONSUMER_MOF), "CommandLineEventConsumer");
            assert_eq!(mof_class("garbage"), "unknown");
        }

        #[test]
        fn string_props_parse_with_unescape() {
            assert_eq!(
                mof_prop(FILTER_MOF, "Name").as_deref(),
                Some("CURE-SYNTH-FILTER-DO-NOT-USE")
            );
            assert_eq!(
                mof_prop(CONSUMER_MOF, "CommandLineTemplate").as_deref(),
                Some("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe -enc SQBuAHYAbwBrAGUALQBhAHAAcAA=")
            );
            assert_eq!(mof_prop(FILTER_MOF, "Missing"), None);
        }

        #[test]
        fn unterminated_quote_is_none_not_panic() {
            assert_eq!(mof_prop("Name = \"oops", "Name"), None);
            assert_eq!(mof_prop("", "Name"), None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn live_scan_never_panics_and_reports_wmi_source() {
        use crate::elevation::SourceStatus;
        use crate::model::PersistenceSource;
        // Live WMI read; asserts shape only. A clean box yields zero rows.
        // On this dev box COM works, so the status must be Available — a
        // regression to silent-empty would show up here as well as in prod.
        let (entries, result) = imp::scan();
        assert!(result.is_ok());
        for e in &entries {
            assert_eq!(e.source, PersistenceSource::WmiSubscription);
            assert!(e.location.starts_with(WMI_NAMESPACE));
        }
        let report = combine(entries, result);
        assert!(matches!(report.status, SourceStatus::Available));
    }

    #[cfg(windows)]
    mod combine_tests {
        use super::super::{combine, filter_location};
        use crate::elevation::SourceStatus;
        use crate::model::{PersistenceEntry, PersistenceSource};
        use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL};

        fn partial() -> Vec<PersistenceEntry> {
            vec![PersistenceEntry::new(
                PersistenceSource::WmiSubscription,
                "WMI filter KEEP-ME",
                "SELECT * FROM __InstanceModificationEvent",
                filter_location("KEEP-ME"),
            )]
        }

        #[test]
        fn ok_keeps_entries_and_reports_available() {
            let report = combine(partial(), Ok(()));
            assert!(matches!(report.status, SourceStatus::Available));
            assert_eq!(report.entries.len(), 1);
        }

        #[test]
        fn access_denied_keeps_partial_entries() {
            // The F-05 regression: a denied query must surface INCOMPLETE
            // with its partial rows, never bare "0 entries".
            let report = combine(partial(), Err(windows::core::Error::from(E_ACCESSDENIED)));
            assert!(matches!(report.status, SourceStatus::AccessDenied { .. }));
            assert_eq!(report.entries.len(), 1);
            assert!(!report.status.is_usable());
        }

        #[test]
        fn generic_failure_is_check_failed_with_partials_kept() {
            let report = combine(partial(), Err(windows::core::Error::from(E_FAIL)));
            assert!(matches!(report.status, SourceStatus::CheckFailed { .. }));
            assert_eq!(report.entries.len(), 1);
            assert!(!report.status.is_usable());
        }
    }
}
