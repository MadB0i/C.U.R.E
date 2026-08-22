use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::model::{PersistenceEntry, PersistenceSource};

const COMMAND_OPEN: &str = "<Command>";
const COMMAND_CLOSE: &str = "</Command>";

pub fn default_tasks_root() -> PathBuf {
    PathBuf::from(r"C:\Windows\System32\Tasks")
}

pub fn scan(root: &Path) -> Vec<PersistenceEntry> {
    let mut entries = Vec::new();
    if !root.is_dir() {
        return entries;
    }
    for file in WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = file.path();
        let xml = read_text_lossy(path);
        let Some(command) = extract_command(&xml) else {
            continue;
        };
        let relative = path.strip_prefix(root).unwrap_or(path);
        let name = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\\");
        entries.push(PersistenceEntry::new(
            PersistenceSource::ScheduledTask,
            name,
            command,
            path.to_string_lossy().into_owned(),
        ));
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

pub fn extract_command(xml: &str) -> Option<String> {
    let start = xml.find(COMMAND_OPEN)? + COMMAND_OPEN.len();
    let end = start + xml[start..].find(COMMAND_CLOSE)?;
    let raw = xml[start..end].trim();
    if raw.is_empty() {
        return None;
    }
    Some(decode_entities(raw))
}

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn read_text_lossy(path: &Path) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => String::from_utf16_lossy(&u16_slice(rest, true)),
        [0xFE, 0xFF, rest @ ..] => String::from_utf16_lossy(&u16_slice(rest, false)),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn u16_slice(bytes: &[u8], little_endian: bool) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
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
                   <Task><Actions><Action><Command>C:\\Users\\Public\\evil.exe -q</Command>\
                   <Arguments>-q</Arguments></Action></Actions></Task>";
        fs::write(nested.join("Persist.xml"), utf16le(xml)).unwrap();

        let entries = scan(dir.path());

        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.source, PersistenceSource::ScheduledTask);
        assert_eq!(e.command, r"C:\Users\Public\evil.exe -q");
        assert_eq!(e.name, r"Microsoft\Windows\Evil\Persist.xml");
        assert_eq!(
            e.id,
            make_id(
                &PersistenceSource::ScheduledTask,
                &e.name,
                &e.command
            )
        );
    }

    #[test]
    fn decodes_entities_and_skips_files_without_commands() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("quoted.xml"),
            r"<Task><Actions><Action><Command>&quot;C:\Program Files\OK\ok.exe&quot; /run</Command></Action></Actions></Task>",
        )
        .unwrap();
        fs::write(
            dir.path().join("system-task.xml"),
            r"<Task><Actions><Action><Command>C:\Windows\System32\cmd.exe</Command></Action></Actions></Task>",
        )
        .unwrap();
        fs::write(dir.path().join("empty-actions.xml"), "<Task><Actions /></Task>").unwrap();

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
}
