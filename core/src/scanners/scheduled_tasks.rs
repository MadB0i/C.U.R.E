use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use walkdir::WalkDir;

use crate::model::{PersistenceEntry, PersistenceSource};

/// Task files over 4 MiB are skipped (real task XML is a few KB; anything
/// bigger is not worth parsing).
const MAX_TASK_BYTES: usize = 4 * 1024 * 1024;

/// One `<Exec>` action inside a task.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ExecAction {
    pub command: String,
    pub arguments: String,
    pub working_dir: String,
}

/// Structured task metadata. Pure data — never executed, never trusted.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TaskDetails {
    pub actions: Vec<ExecAction>,
    pub author: String,
    pub user_id: String,
    pub run_level: String,
    /// Trigger element names (`LogonTrigger`, `BootTrigger`, …).
    pub triggers: Vec<String>,
    /// Task-level `<Settings><Enabled>` when present.
    pub enabled: Option<bool>,
    /// Task-level `<Settings><Hidden>`.
    pub hidden: bool,
}

pub fn default_tasks_root() -> PathBuf {
    PathBuf::from(r"C:\Windows\System32\Tasks")
}

/// Scan result with access accounting. System task files unreadable
/// without elevation are COUNTED as skipped — never silently omitted.
pub struct TaskScanReport {
    pub entries: Vec<PersistenceEntry>,
    pub files_seen: usize,
    pub skipped: usize,
}

impl TaskScanReport {
    pub fn status(&self) -> crate::elevation::SourceStatus {
        use crate::elevation::SourceStatus;
        if self.skipped == 0 {
            SourceStatus::Available
        } else if self.files_seen > self.skipped {
            SourceStatus::Partial {
                skipped: self.skipped,
                reason: "some task files unreadable (elevation may help)".to_string(),
            }
        } else {
            SourceStatus::AccessDenied {
                reason: "task directory unreadable (elevation may help)".to_string(),
            }
        }
    }
}

pub fn scan_report(root: &Path) -> TaskScanReport {
    let mut report = TaskScanReport {
        entries: Vec::new(),
        files_seen: 0,
        skipped: 0,
    };
    if !root.is_dir() {
        return report;
    }
    for entry in WalkDir::new(root).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                report.skipped += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        report.files_seen += 1;
        let path = entry.path();
        let xml = read_text_lossy(path);
        if xml.is_empty() {
            // Unreadable or over the size cap — counted as skipped only if
            // the file could not be read at all.
            if std::fs::metadata(path).is_err() {
                report.skipped += 1;
            }
            continue;
        }
        let Some(command) = extract_command(&xml) else {
            continue;
        };
        let relative = path.strip_prefix(root).unwrap_or(path);
        let name = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\\");
        report.entries.push(PersistenceEntry::new(
            PersistenceSource::ScheduledTask,
            name,
            command,
            path.to_string_lossy().into_owned(),
        ));
    }
    report.entries.sort_by(|a, b| a.name.cmp(&b.name));
    report
}

pub fn scan(root: &Path) -> Vec<PersistenceEntry> {
    scan_report(root).entries
}

/// First action's command (compat path for entry scoring/display).
/// Backed by the structured parser — one parser, not two.
pub fn extract_command(xml: &str) -> Option<String> {
    let details = parse_details(xml);
    let first = details.actions.into_iter().next()?;
    if first.command.trim().is_empty() {
        return None;
    }
    Some(first.command)
}

/// Full structured parse. Total on untrusted input: malformed XML yields
/// empty/default fields, never a panic and never code execution.
/// (quick-xml is a pull parser: no DTD processing, no external entities.)
pub fn parse_details(xml: &str) -> TaskDetails {
    let mut out = TaskDetails::default();
    let mut reader = Reader::from_str(xml);
    // No trimming: text can split across entity boundaries (`&quot;` arrives
    // as its own event), so chunks accumulate raw and fields are trimmed
    // when their element closes. Indentation whitespace never accumulates:
    // it only appears while no capture target is armed.
    reader.config_mut().trim_text(false);
    // Hard stop so a hostile megabyte of tags cannot spin the scan.
    let mut events = 0usize;
    const MAX_EVENTS: usize = 20_000;

    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut current = ExecAction::default();
    let mut in_exec = false;
    let mut text_target: Option<&str> = None;
    let mut buf = Vec::new();

    loop {
        if events >= MAX_EVENTS {
            break;
        }
        events += 1;
        let event = match reader.read_event_into(&mut buf) {
            Ok(e) => e,
            Err(_) => break,
        };
        match event {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_vec();
                // Open a text capture for fields we care about.
                text_target = capture_target(&stack, &name, in_exec);
                if name == b"Exec" {
                    in_exec = true;
                    current = ExecAction::default();
                }
                // Trigger types are direct children of <Triggers>.
                if stack.last().map(Vec::as_slice) == Some(b"Triggers".as_slice())
                    && name.ends_with(b"Trigger")
                {
                    let t = String::from_utf8_lossy(&name).into_owned();
                    if out.triggers.len() < 16 && !out.triggers.contains(&t) {
                        out.triggers.push(t);
                    }
                }
                stack.push(name);
            }
            Event::Empty(e) => {
                let name = e.local_name().as_ref().to_vec();
                if stack.last().map(Vec::as_slice) == Some(b"Triggers".as_slice())
                    && name.ends_with(b"Trigger")
                {
                    let t = String::from_utf8_lossy(&name).into_owned();
                    if out.triggers.len() < 16 && !out.triggers.contains(&t) {
                        out.triggers.push(t);
                    }
                }
                // Self-closed <Exec/> carries no command — ignore.
                // Self-closed fields (<Enabled/> etc.) carry no text — ignore.
            }
            Event::Text(e) => {
                if let Some(target) = text_target {
                    let raw = e.decode().unwrap_or_default();
                    let text = unescape(&raw)
                        .map(|s| s.into_owned())
                        .unwrap_or_else(|_| raw.into_owned());
                    if !text.is_empty() {
                        assign_text(&mut out, &mut current, in_exec, target, &text);
                    }
                }
            }
            Event::GeneralRef(e) => {
                // `&quot;`, `&amp;`, numeric refs… — predefined entities are
                // structural text, not markup. Unknown entities are dropped:
                // task commands have no business with custom entities and
                // DTDs are never processed.
                if let Some(target) = text_target {
                    if let Some(ch) = resolve_entity(&e) {
                        let mut s = String::new();
                        s.push(ch);
                        assign_text(&mut out, &mut current, in_exec, target, &s);
                    }
                }
            }
            Event::End(e) => {
                let name = e.local_name();
                if name.as_ref() == b"Exec" && in_exec {
                    in_exec = false;
                    current.command = current.command.trim().to_string();
                    current.arguments = current.arguments.trim().to_string();
                    current.working_dir = current.working_dir.trim().to_string();
                    if !current.command.is_empty() && out.actions.len() < 32 {
                        out.actions.push(std::mem::take(&mut current));
                    }
                }
                if name.as_ref() == b"RegistrationInfo" {
                    out.author = out.author.trim().to_string();
                }
                if name.as_ref() == b"Principal" {
                    out.user_id = out.user_id.trim().to_string();
                    out.run_level = out.run_level.trim().to_string();
                }
                stack.pop();
                text_target = None;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Which struct field the next text node belongs to, if any.
fn capture_target(stack: &[Vec<u8>], name: &[u8], in_exec: bool) -> Option<&'static str> {
    if in_exec {
        return match name {
            b"Command" => Some("command"),
            b"Arguments" => Some("arguments"),
            b"WorkingDirectory" => Some("working_dir"),
            _ => None,
        };
    }
    let parent = stack.last().map(Vec::as_slice);
    match (parent, name) {
        (Some(b"RegistrationInfo"), b"Author") => Some("author"),
        (Some(b"Principal"), b"UserId") => Some("user_id"),
        (Some(b"Principal"), b"RunLevel") => Some("run_level"),
        (Some(b"Settings"), b"Enabled") => Some("enabled"),
        (Some(b"Settings"), b"Hidden") => Some("hidden"),
        _ => None,
    }
}

/// Resolve one entity reference. Predefined XML entities plus numeric
/// character references; anything else (custom entities need a DTD, which
/// is never processed) resolves to nothing and is dropped by the caller.
fn resolve_entity(e: &quick_xml::events::BytesRef) -> Option<char> {
    let bytes: &[u8] = e;
    match bytes {
        b"quot" => Some('"'),
        b"amp" => Some('&'),
        b"apos" => Some('\''),
        b"lt" => Some('<'),
        b"gt" => Some('>'),
        _ => e.resolve_char_ref().ok()?,
    }
}

fn assign_text(
    out: &mut TaskDetails,
    current: &mut ExecAction,
    in_exec: bool,
    target: &str,
    text: &str,
) {
    if in_exec {
        let slot = match target {
            "command" => &mut current.command,
            "arguments" => &mut current.arguments,
            "working_dir" => &mut current.working_dir,
            _ => return,
        };
        // Accumulate: text splits at entity boundaries. Trimmed on </Exec>.
        slot.push_str(text);
        return;
    }
    match target {
        "author" => out.author.push_str(text),
        "user_id" => out.user_id.push_str(text),
        "run_level" => out.run_level.push_str(text),
        "enabled" if out.enabled.is_none() => {
            let t = text.trim();
            if !t.is_empty() {
                out.enabled = Some(!matches!(
                    t.to_ascii_lowercase().as_str(),
                    "false" | "0" | "no"
                ));
            }
        }
        "hidden" => {
            let t = text.trim();
            if !t.is_empty() {
                out.hidden = !matches!(t.to_ascii_lowercase().as_str(), "false" | "0" | "no");
            }
        }
        _ => {}
    }
}

fn read_text_lossy(path: &Path) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    if bytes.len() > MAX_TASK_BYTES {
        return String::new();
    }
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => String::from_utf16_lossy(&u16_slice(rest, true)),
        [0xFE, 0xFF, rest @ ..] => String::from_utf16_lossy(&u16_slice(rest, false)),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn u16_slice(bytes: &[u8], little_endian: bool) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes(*pair)
            } else {
                u16::from_be_bytes(*pair)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::make_id;
    use std::fs;
    use tempfile::tempdir;

    fn utf16le(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn extracts_command_from_utf16_task_xml() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("Microsoft").join("Windows").join("Evil");
        fs::create_dir_all(&nested).unwrap();
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\
                   <Task><Actions><Exec><Command>C:\\Users\\Public\\evil.exe -q</Command>\
                   <Arguments>-q</Arguments></Exec></Actions></Task>";
        fs::write(nested.join("Persist.xml"), utf16le(xml)).unwrap();

        let entries = scan(dir.path());

        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.source, PersistenceSource::ScheduledTask);
        assert_eq!(e.command, r"C:\Users\Public\evil.exe -q");
        assert_eq!(e.name, r"Microsoft\Windows\Evil\Persist.xml");
        assert_eq!(
            e.id,
            make_id(&PersistenceSource::ScheduledTask, &e.name, &e.command)
        );
    }

    #[test]
    fn decodes_entities_and_skips_files_without_commands() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("quoted.xml"),
            r"<Task><Actions><Exec><Command>&quot;C:\Program Files\OK\ok.exe&quot; /run</Command></Exec></Actions></Task>",
        )
        .unwrap();
        fs::write(
            dir.path().join("system-task.xml"),
            r"<Task><Actions><Exec><Command>C:\Windows\System32\cmd.exe</Command></Exec></Actions></Task>",
        )
        .unwrap();
        fs::write(
            dir.path().join("empty-actions.xml"),
            "<Task><Actions /></Task>",
        )
        .unwrap();

        let entries = scan(dir.path());

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "quoted.xml");
        assert_eq!(entries[0].command, r#""C:\Program Files\OK\ok.exe" /run"#);
        assert_eq!(entries[1].name, "system-task.xml");
    }

    #[test]
    fn missing_root_is_empty() {
        assert!(scan(Path::new("Z:/nope")).is_empty());
    }

    #[test]
    fn details_parse_normal_task() {
        let d = parse_details(&crate::fixtures::normal_task_xml());
        assert_eq!(d.actions.len(), 1);
        assert_eq!(
            d.actions[0].command,
            r"C:\Program Files\CURE-SYNTH-Vendor\updater.exe"
        );
        assert_eq!(d.actions[0].arguments, "/checknow");
        assert_eq!(
            d.actions[0].working_dir,
            r"C:\Program Files\CURE-SYNTH-Vendor"
        );
        assert_eq!(d.author, "CURE-SYNTH Vendor");
        assert_eq!(d.run_level, "LeastPrivilege");
        assert_eq!(d.triggers, vec!["LogonTrigger".to_string()]);
        assert_eq!(d.enabled, Some(true));
        assert!(!d.hidden);
    }

    #[test]
    fn details_parse_multi_action_task() {
        let d = parse_details(&crate::fixtures::multi_action_task_xml());
        assert_eq!(d.actions.len(), 2);
        assert_eq!(d.actions[0].command, "powershell.exe");
        assert!(d.actions[0].arguments.contains("-WindowStyle Hidden"));
        assert_eq!(
            d.actions[1].command,
            r"C:\cure-synth\Temp\CURE-SYNTH-stage2.exe"
        );
        assert_eq!(d.actions[1].working_dir, r"C:\cure-synth\Temp");
        assert_eq!(d.author, "CURE-SYNTH Unknown");
        assert_eq!(d.user_id, "S-1-5-18");
        assert_eq!(d.run_level, "HighestAvailable");
        assert!(d.triggers.contains(&"BootTrigger".to_string()));
        assert!(d.triggers.contains(&"CalendarTrigger".to_string()));
        assert_eq!(d.enabled, Some(true));
        assert!(d.hidden);
        // First action still drives entry command/scoring (compat).
        assert_eq!(
            extract_command(&crate::fixtures::multi_action_task_xml()).as_deref(),
            Some("powershell.exe")
        );
    }

    #[test]
    fn details_survive_malformed_input() {
        for (label, xml) in crate::fixtures::malformed_task_xmls() {
            let d = parse_details(&xml);
            assert!(
                d.actions.len() <= 1,
                "{label}: hostile input must not multiply actions"
            );
            if label == "entities" {
                assert_eq!(
                    extract_command(&xml).as_deref(),
                    Some(r"C:\cure-synth\a & b\x.exe"),
                    "{label}: entities must decode"
                );
            } else if label != "entities" {
                assert_eq!(extract_command(&xml), None, "{label}: no command expected");
            }
        }
    }
}
