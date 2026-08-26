//! Ransom-note and bulk-encryption detection.
//!
//! All logic is pure and cross-platform — no filesystem writes, no OS
//! calls.  The caller is responsible for reading directory entries and
//! passing them in.

use std::path::{Path, PathBuf};

// ── configurable pattern list ─────────────────────────────────────────

/// Filename stems (case-insensitive) commonly used by ransom notes.
/// The list is deliberately broad to catch families without hardcoding
/// assumptions about any single one.
pub const RANSOM_NOTE_STEMS: &[&str] = &[
    "README",
    "DECRYPT",
    "DECRYPT-NOW",
    "DECRYPT-FILES",
    "HOW_TO_RECOVER",
    "HOW_TO_DECRYPT",
    "HOW_TO_RESTORE",
    "RECOVER-YOUR-FILES",
    "RESTORE-YOUR-FILES",
    "YOUR_FILES_ARE_ENCRYPTED",
    "FILES_ENCRYPTED",
    "READ_ME",
    "HELP_DECRYPT",
    "WHAT_HAPPENED",
    "RECOVER",
    "IMPORTANT",
    "INSTRUCTION",
    "RELEVANT",
];

/// Filename extensions commonly paired with ransom notes.
pub const NOTE_EXTENSIONS: &[&str] = &[
    "txt", "html", "htm", "md", "json", "png", "jpg", "bmp",
];

/// Extensions that are too common to signal mass encryption (every OS has
/// thousands of `.txt`, `.docx`, `.exe`, etc.).
const BORING_EXTENSIONS: &[&str] = &[
    "exe", "dll", "sys", "msi", "msu", "msp",
    "txt", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "pdf", "html", "htm", "xml", "json", "csv",
    "jpg", "jpeg", "png", "gif", "bmp", "ico", "svg", "webp",
    "mp3", "mp4", "avi", "mkv", "mov", "wav", "flac",
    "zip", "rar", "7z", "tar", "gz",
    "bat", "cmd", "ps1", "vbs", "js", "wsh",
    "ini", "cfg", "conf", "log",
    "lnk", "url",
];

/// Minimum number of files with the same extension in a single directory
/// to trigger the bulk-extension heuristic.
const MIN_BULK_COUNT: usize = 10;

/// Case-insensitive check for extensions too common to be useful signal.
pub fn is_boring_extension(ext: &str) -> bool {
    let e = ext.to_ascii_lowercase();
    BORING_EXTENSIONS.iter().any(|&b| b == e)
}

/// Maximum ratio of "boring" (common) extensions allowed before the
/// cluster is dismissed.  If more than 30 % of files in a cluster have
/// boring extensions, skip it — likely a normal data directory.
const BORING_RATIO_THRESHOLD: f64 = 0.3;

/// Minimum age (in days) for the oldest file in a cluster to be
/// considered suspicious — a folder full of brand-new `.abc` files is
/// more likely a software artifact than ransomware output.
const MIN_AGE_DAYS: u64 = 1;

// ── types ─────────────────────────────────────────────────────────────

/// A file entry as seen by the caller (pure data, no OS handles).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub age_days: u64,
}

/// A suspected ransom note found on disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RansomNote {
    /// Full path to the note file.
    pub path: PathBuf,
    /// Matched filename stem.
    pub matched_stem: String,
    /// Suspected ransomware family (best-effort guess, may be `None`).
    pub suspected_family: Option<String>,
    /// Short snippet from the note content for display.
    pub content_snippet: String,
}

/// A cluster of files with an unusual, shared extension.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BulkExtensionCluster {
    /// Directory containing the cluster.
    pub folder: PathBuf,
    /// The shared extension (without leading dot).
    pub extension: String,
    /// Number of files sharing this extension.
    pub file_count: usize,
    /// Average age of the files in days.
    pub avg_age_days: u64,
}

/// A single ransom-detection finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RansomFinding {
    Note(RansomNote),
    BulkEncryption(BulkExtensionCluster),
}

// ── helpers ───────────────────────────────────────────────────────────

fn stem_matches(name: &str) -> Option<String> {
    let stem = name.rsplit('.').nth(1).unwrap_or("").to_ascii_uppercase();

    // Sort by length descending so longer patterns match first
    // (avoids "DECRYPT" matching before "DECRYPT-NOW")
    let upper = name.to_ascii_uppercase();
    let mut best: Option<(&str, usize)> = None;
    for &pattern in RANSOM_NOTE_STEMS {
        let matched = if stem == pattern {
            Some(pattern.len())
        } else if upper.contains(pattern) {
            Some(pattern.len())
        } else {
            None
        };
        if let Some(len) = matched {
            if best.map_or(true, |(_, bl)| len > bl) {
                best = Some((pattern, len));
            }
        }
    }
    best.map(|(p, _)| p.to_string())
}

fn ext_is_boring(ext: &str) -> bool {
    BORING_EXTENSIONS.contains(&ext)
}

fn is_hidden_or_system(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || name == "Thumbs.db"
}

/// Read a capped snippet from a ransom note file (read-only, lossy UTF-8).
/// Used by the GUI glue to feed `guess_family` without loading the full file.
/// Returns an empty string on any I/O or decode error.
pub fn load_note_content(path: &Path, max_bytes: usize) -> String {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = vec![0u8; max_bytes];
    let Ok(n) = f.read(&mut buf) else {
        return String::new();
    };
    buf.truncate(n);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Guess the ransomware family from the content of a note.
/// Returns a best-effort label — clearly-labeled as uncertain in the UI.
pub fn guess_family(content: &str) -> Option<&'static str> {
    let lower = content.to_ascii_lowercase();
    let families: &[(&[&str], &str)] = &[
        (&["revil", "sodinokibi", "sodin"], "REvil/Sodinokibi"),
        (&["ryuk"], "Ryuk"),
        (&["maze"], "Maze"),
        (&["conti"], "Conti"),
        (&["lockbit"], "LockBit"),
        (&["phobos"], "Phobos"),
        (&["stop", "djvu", "djvu2", "tro"], "STOP/Djvu"),
        (&["wannacry", "wanna cry", "wanna"], "WannaCry"),
        (&["petya", "notpetya"], "Petya/NotPetya"),
        (&["dharma", "crypto"], "Dharma"),
        (&["medusa"], "Medusa"),
        (&["blackcat", "alphv"], "BlackCat/ALPHV"),
        (&["play"], "Play"),
    ];
    for (keywords, family) in families {
        if keywords.iter().any(|kw| lower.contains(kw)) {
            return Some(family);
        }
    }
    None
}

// ── public API ────────────────────────────────────────────────────────

/// Check if a filename looks like a ransom note.
/// Returns the matched stem if so.
pub fn is_ransom_note(name: &str) -> Option<String> {
    if is_hidden_or_system(name) {
        return None;
    }
    stem_matches(name)
}

/// Scan a list of directory entries for ransom-note filenames.
///
/// `folder` is the directory being scanned (used only for building the
/// returned path).
pub fn scan_for_notes(folder: &Path, entries: &[DirEntry]) -> Vec<RansomNote> {
    let mut results = Vec::new();
    for e in entries {
        if let Some(stem) = is_ransom_note(&e.name) {
            results.push(RansomNote {
                path: folder.join(&e.name),
                matched_stem: stem,
                suspected_family: None,
                content_snippet: String::new(),
            });
        }
    }
    results
}

/// Detect clusters of files with an unusual shared extension.
///
/// Returns only clusters that pass all heuristic gates (sufficient count,
/// uncommon extension, no excessive boring-extension ratio, minimum age).
pub fn detect_bulk_extension(folder: PathBuf, entries: &[DirEntry]) -> Vec<BulkExtensionCluster> {
    use std::collections::HashMap;

    if entries.len() < MIN_BULK_COUNT {
        return Vec::new();
    }

    // Group by lowercase extension
    let mut groups: HashMap<String, Vec<&DirEntry>> = HashMap::new();
    for e in entries {
        if let Some(ext) = e.name.rsplit('.').next() {
            let ext_lower = ext.to_ascii_lowercase();
            if ext_lower.len() >= 2 && ext_lower.len() <= 12 {
                groups.entry(ext_lower).or_default().push(e);
            }
        }
    }

    let mut results = Vec::new();
    for (ext, group) in &groups {
        let count = group.len();
        if count < MIN_BULK_COUNT {
            continue;
        }
        if ext_is_boring(ext) {
            continue;
        }

        // Check boring ratio — if many files have boring extensions, skip
        let boring_count = group
            .iter()
            .filter(|e| {
                e.name
                    .rsplit('.')
                    .next()
                    .map(|e| ext_is_boring(&e.to_ascii_lowercase()))
                    .unwrap_or(false)
            })
            .count();
        if (boring_count as f64 / count as f64) > BORING_RATIO_THRESHOLD {
            continue;
        }

        // Minimum age check
        let avg_age: u64 = group.iter().map(|e| e.age_days).sum::<u64>() / count as u64;
        if avg_age < MIN_AGE_DAYS {
            continue;
        }

        results.push(BulkExtensionCluster {
            folder: folder.clone(),
            extension: ext.clone(),
            file_count: count,
            avg_age_days: avg_age,
        });
    }

    // Sort by count descending — most-affected folders first
    results.sort_by(|a, b| b.file_count.cmp(&a.file_count));
    results
}

/// Convenience: run both note detection and bulk-extension detection on
/// a set of folder entries.  Returns all findings sorted by severity
/// (notes first, then bulk clusters by count descending).
pub fn scan_folders(
    folders: &[(PathBuf, Vec<DirEntry>)],
) -> Vec<RansomFinding> {
    let mut findings: Vec<RansomFinding> = Vec::new();

    for (folder, entries) in folders {
        let notes = scan_for_notes(folder, entries);
        findings.extend(notes.into_iter().map(RansomFinding::Note));

        let clusters = detect_bulk_extension(folder.clone(), entries);
        findings
            .extend(clusters.into_iter().map(RansomFinding::BulkEncryption));
    }

    // Notes first, then clusters by file count descending
    findings.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (a, b) {
            (RansomFinding::Note(_), RansomFinding::BulkEncryption(_)) => Ordering::Less,
            (RansomFinding::BulkEncryption(_), RansomFinding::Note(_)) => Ordering::Greater,
            (
                RansomFinding::BulkEncryption(a),
                RansomFinding::BulkEncryption(b),
            ) => b.file_count.cmp(&a.file_count),
            _ => Ordering::Equal,
        }
    });

    findings
}

// ── tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn readme_txt_is_ransom_note() {
        assert_eq!(is_ransom_note("README.txt"), Some("README".to_string()));
    }

    #[test]
    fn how_to_recover_html_is_ransom_note() {
        assert_eq!(
            is_ransom_note("HOW_TO_RECOVER.html"),
            Some("HOW_TO_RECOVER".to_string())
        );
    }

    #[test]
    fn normal_file_is_not_ransom_note() {
        assert_eq!(is_ransom_note("report.docx"), None);
    }

    #[test]
    fn hidden_file_skipped() {
        assert_eq!(is_ransom_note(".README.txt"), None);
    }

    #[test]
    fn scan_for_notes_finds_matches() {
        let entries = vec![
            DirEntry { name: "photo.jpg".into(), age_days: 5 },
            DirEntry { name: "DECRYPT-NOW.txt".into(), age_days: 1 },
            DirEntry { name: "budget.xlsx".into(), age_days: 30 },
        ];
        let notes = scan_for_notes(Path::new(r"C:\Users\Bob\Desktop"), &entries);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].matched_stem, "DECRYPT-NOW");
        assert!(notes[0].path.to_string_lossy().contains("DECRYPT-NOW.txt"));
    }

    #[test]
    fn bulk_extension_detects_uncommon_cluster() {
        let entries: Vec<DirEntry> = (0..15)
            .map(|i| DirEntry {
                name: format!("file_{i}.locked"),
                age_days: 5,
            })
            .collect();
        let clusters = detect_bulk_extension(PathBuf::from(r"C:\Docs"), &entries);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].extension, "locked");
        assert_eq!(clusters[0].file_count, 15);
    }

    #[test]
    fn bulk_extension_ignores_boring_ext() {
        let entries: Vec<DirEntry> = (0..20)
            .map(|i| DirEntry {
                name: format!("doc_{i}.txt"),
                age_days: 10,
            })
            .collect();
        let clusters = detect_bulk_extension(PathBuf::from(r"C:\Docs"), &entries);
        assert!(clusters.is_empty());
    }

    #[test]
    fn bulk_extension_ignores_too_few() {
        let entries: Vec<DirEntry> = (0..5)
            .map(|i| DirEntry {
                name: format!("f{i}.xyz"),
                age_days: 5,
            })
            .collect();
        let clusters = detect_bulk_extension(PathBuf::from(r"C:\Docs"), &entries);
        assert!(clusters.is_empty());
    }

    #[test]
    fn guess_family_finds_ryuk() {
        let content = "Your files are encrypted by Ryuk. Send bitcoin to ...";
        assert_eq!(guess_family(content), Some("Ryuk"));
    }

    #[test]
    fn guess_family_returns_none_for_unknown() {
        let content = "Please buy our premium software for the best results.";
        assert_eq!(guess_family(content), None);
    }

    #[test]
    fn guess_family_finds_lockbit() {
        let content = "LockBit 3.0 ransomware. All your files have been encrypted.";
        assert_eq!(guess_family(content), Some("LockBit"));
    }

    #[test]
    fn scan_folders_combines_notes_and_clusters() {
        let entries = vec![
            DirEntry { name: "README.txt".into(), age_days: 1 },
        ];
        let mut bulk: Vec<DirEntry> = (0..20)
            .map(|i| DirEntry { name: format!("data_{i}.enc"), age_days: 3 })
            .collect();
        let mut all = entries.clone();
        all.append(&mut bulk);

        let findings = scan_folders(&[(PathBuf::from(r"C:\Users"), all)]);
        assert_eq!(findings.len(), 2); // 1 note + 1 cluster
        assert!(matches!(&findings[0], RansomFinding::Note(_)));
        assert!(matches!(&findings[1], RansomFinding::BulkEncryption(_)));
    }

    #[test]
    fn load_note_content_reads_file() {
        let dir = std::env::temp_dir().join("cure_ransom_load_note_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let note_path = dir.join("DECRYPT_MY_FILES.txt");
        std::fs::write(&note_path, "Your files are encrypted by LockBit 3.0. Pay 0.5 BTC.").unwrap();
        let snippet = load_note_content(&note_path, 128);
        assert!(snippet.contains("LockBit"));
        assert!(snippet.contains("0.5 BTC"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End-to-end test against a real (throwaway) directory.
    /// Uses `std::fs::read_dir` → `DirEntry` exactly like the GUI glue,
    /// then feeds real entries into `scan_folders` to verify both detection
    /// paths fire against actual filesystem reads, not hand-built fixtures.
    #[test]
    fn real_fs_ransom_scan_finds_note_and_bulk_cluster() {
        use std::time::{Duration, SystemTime};
        use std::fs::FileTimes;

        let dir = std::env::temp_dir().join("cure_ransom_realfs_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Fake ransom note mentioning a known family
        let note_path = dir.join("DECRYPT_MY_FILES.txt");
        std::fs::write(
            &note_path,
            "All your files have been encrypted by LockBit 3.0.\n\
             Send 0.5 BTC to the wallet address below to recover them.",
        )
        .unwrap();

        // 15 empty .locked files — uncommon extension, all >1 day old
        let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 86400);
        for i in 0..15 {
            let p = dir.join(format!("photo_{i:02}.locked"));
            let f = File::create(&p).unwrap();
            let times = FileTimes::new().set_modified(two_days_ago);
            f.set_times(times).unwrap();
        }

        // Backdate the note as well so age_days ≥ 1
        {
            let f = File::options().write(true).open(&note_path).unwrap();
            let times = FileTimes::new().set_modified(two_days_ago);
            f.set_times(times).unwrap();
        }

        // Enumerate using real std::fs::read_dir, converting to the same
        // DirEntry shape the GUI glue produces.
        let entries: Vec<DirEntry> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let meta = e.metadata().ok()?;
                let modified = meta.modified().ok()?;
                let age = SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or(Duration::ZERO)
                    .as_secs() / 86400;
                Some(DirEntry { name, age_days: age })
            })
            .collect();

        // Should have 16 entries total (1 note + 15 locked files)
        assert_eq!(entries.len(), 16);

        let findings = scan_folders(&[(dir.clone(), entries)]);
        assert_eq!(findings.len(), 2, "expected note + bulk cluster");

        let note = findings.iter().find_map(|f| match f {
            RansomFinding::Note(n) => Some(n),
            _ => None,
        });
        let cluster = findings.iter().find_map(|f| match f {
            RansomFinding::BulkEncryption(c) => Some(c),
            _ => None,
        });

        let note = note.expect("should find ransom note");
        assert_eq!(note.matched_stem, "DECRYPT");

        // Content snippet + family guess against the REAL file on disk
        let snippet = load_note_content(&note.path, 4096);
        assert!(
            snippet.contains("LockBit"),
            "content snippet should mention LockBit, got: {snippet:?}"
        );
        assert_eq!(guess_family(&snippet), Some("LockBit"));

        let cluster = cluster.expect("should find bulk extension cluster");
        assert_eq!(cluster.file_count, 15);
        assert_eq!(cluster.extension, "locked");
        assert!(cluster.avg_age_days >= 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
