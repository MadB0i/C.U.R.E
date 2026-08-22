use std::collections::HashSet;
use std::path::Path;

pub fn list_drives() -> HashSet<String> {
    #[cfg(target_os = "windows")]
    {
        windows_drives()
    }
    #[cfg(not(target_os = "windows"))]
    {
        HashSet::new()
    }
}

#[cfg(target_os = "windows")]
fn windows_drives() -> HashSet<String> {
    let mut drives = HashSet::new();
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if Path::new(&root).exists() {
            drives.insert(root);
        }
    }
    drives
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_look_like_drive_roots() {
        for drive in list_drives() {
            let chars: Vec<char> = drive.chars().collect();
            assert_eq!(chars.len(), 3, "unexpected entry: {drive}");
            assert!(chars[0].is_ascii_uppercase(), "unexpected entry: {drive}");
            assert_eq!(chars[1], ':', "unexpected entry: {drive}");
            assert_eq!(chars[2], '\\', "unexpected entry: {drive}");
        }
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn stub_is_empty_off_windows() {
        assert!(list_drives().is_empty());
    }
}
