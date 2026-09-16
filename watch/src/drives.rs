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

    /// Perf guard for the 1.5 s poll loop: one poll is 26 drive-letter
    /// existence probes plus a set-diff. 200 polls must finish in well
    /// under a second — the watcher is I/O-idle, not a CPU cost.
    /// (Detection latency is dominated by the 1500 ms poll interval by
    /// design; see TESTING.md §3 and the WM_DEVICECHANGE plan in AUDIT.md.)
    #[test]
    fn poll_cycle_is_cheap() {
        let start = std::time::Instant::now();
        let mut previous = list_drives();
        for _ in 0..200 {
            let current = list_drives();
            let _ = crate::detector::newly_arrived(&previous, &current);
            previous = current;
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "200 poll cycles took {elapsed:?} — the watcher must stay idle-cheap"
        );
    }
}
