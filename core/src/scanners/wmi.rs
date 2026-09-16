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
//!   yields an empty vec, never a panic or a hard error.
//! - Output is capped (objects per query, text length, total entries).
//!
//! Non-Windows builds return an empty vec.

use crate::model::PersistenceEntry;

pub const WMI_NAMESPACE: &str = r"ROOT\subscription";

pub fn filter_location(name: &str) -> String {
    format!(r"{WMI_NAMESPACE}:__EventFilter.Name={name:?}")
}

#[cfg(windows)]
pub fn scan() -> Vec<PersistenceEntry> {
    imp::scan().unwrap_or_default()
}

#[cfg(not(windows))]
pub fn scan() -> Vec<PersistenceEntry> {
    Vec::new()
}

#[cfg(windows)]
mod imp {
    use windows::core::{BSTR, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize,
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOLE_AUTHENTICATION_CAPABILITIES,
        RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
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

    pub(super) fn scan() -> windows::core::Result<Vec<PersistenceEntry>> {
        let mut entries = Vec::new();
        // S_OK = we initialized COM and must uninitialize; S_FALSE = the
        // thread was already initialized (leave it alone).
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if init.is_err() {
            return Ok(entries);
        }
        let result = scan_inner(&mut entries);
        if init == windows::Win32::Foundation::S_OK {
            unsafe { CoUninitialize() };
        }
        let _ = result;
        Ok(entries)
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
                .or_else(|| {
                    mof_prop(&mof, "ScriptText").map(|s| truncate(&s, 300))
                })
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
        for mof in exec_texts(&server, "SELECT Filter, Consumer FROM __FilterToConsumerBinding")? {
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
    pub(super) fn mof_prop(mof: &str, prop: &str) -> Option<String> {
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
        use crate::model::PersistenceSource;
        // Live WMI read; asserts shape only. A clean box yields zero rows.
        for e in scan() {
            assert_eq!(e.source, PersistenceSource::WmiSubscription);
            assert!(e.location.starts_with(WMI_NAMESPACE));
        }
    }
}
