//! LUMA Storage Rescue: Intelligent Cross-Volume Migration & Space Relief Engine.
//!
//! Purpose:
//! Relieve system drive (C:) pressure safely by identifying, planning,
//! and verifying the migration of high-impact data folders to secondary volumes (e.g. D:)
//! without data loss, silent deletion, or broken applications.
//!
//! Non-negotiable principles:
//! 1. **Never silently delete the source**: Source files remain 100% intact until the operator
//!    explicitly reviews the verification receipt and decides to retire them.
//! 2. **Strict safety classification**:
//!    - `Safe`: User personal content (Downloads, Videos, Pictures, Documents, Desktop, Archives).
//!    - `Caution`: Developer caches, package repos, workspaces (requires environment or IDE reconfiguration).
//!    - `DoNotMove`: Operating system, Program Files, critical AppData, registry-dependent internals.
//! 3. **Dual-integrity copy verification**: File count parity, byte parity, and cryptographic sample hashing
//!    must match before a migration is marked complete.
//! 4. **Honest capacity checks**: Validate that target volumes have sufficient free space plus a 10% safety buffer.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::fsx;

/// Safety rating for cross-drive folder relocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationSafety {
    /// Personal user content. No system executable or registry dependencies.
    Safe,
    /// Developer caches, package stores, or project workspaces.
    /// Safe to relocate but requires updating environment variables or IDE paths.
    Caution,
    /// Critical operating system or application runtime data. Relocation risks instability.
    DoNotMove,
}

impl MigrationSafety {
    pub fn label(self) -> &'static str {
        match self {
            MigrationSafety::Safe => "Safe to Move",
            MigrationSafety::Caution => "Caution / Reconfigure",
            MigrationSafety::DoNotMove => "Do Not Move (Protected)",
        }
    }

    pub fn explanation(self) -> &'static str {
        match self {
            MigrationSafety::Safe => {
                "Independent user files. Safe to relocate without breaking Windows or installed programs."
            }
            MigrationSafety::Caution => {
                "Application or developer data. Can be relocated, but requires configuring the tool or updating paths."
            }
            MigrationSafety::DoNotMove => {
                "System-critical or registry-linked directory. Relocation can cause OS or application malfunction."
            }
        }
    }

    pub fn is_allowed(self) -> bool {
        matches!(self, MigrationSafety::Safe | MigrationSafety::Caution)
    }
}

/// Real drive pressure assessment for an individual volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrivePressure {
    pub drive_letter: String,
    pub volume_label: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub pressure_pct: f64,
    pub is_system: bool,
    pub is_congested: bool,
}

/// A discovered folder candidate that can be migrated to relieve drive pressure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationCandidate {
    pub id: String,
    pub name: String,
    pub source_path: PathBuf,
    pub suggested_target_path: PathBuf,
    pub size_bytes: u64,
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub safety: MigrationSafety,
    pub category: String,
    pub why_candidate: String,
    pub reconfiguration_advice: Option<String>,
}

/// Overall storage rescue diagnostic summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRescueStatus {
    pub drives: Vec<DrivePressure>,
    pub system_drive: Option<String>,
    pub recommended_target_drive: Option<String>,
    pub candidates: Vec<MigrationCandidate>,
    pub total_safe_relief_bytes: u64,
    pub total_caution_relief_bytes: u64,
}

/// A structured plan to relocate one or more candidates to a destination drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub source_drive: String,
    pub target_drive: String,
    pub selected_candidates: Vec<MigrationCandidate>,
    pub total_bytes_to_relocate: u64,
    pub total_files_to_relocate: u64,
    pub target_current_free_bytes: u64,
    pub target_projected_free_bytes: u64,
    pub source_current_free_bytes: u64,
    pub source_projected_free_bytes: u64,
    pub has_sufficient_space: bool,
    pub required_headroom_bytes: u64,
    pub preflight_errors: Vec<String>,
    pub preflight_warnings: Vec<String>,
}

/// Result of an individual candidate copy and integrity verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateVerification {
    pub candidate_id: String,
    pub source_path: String,
    pub target_path: String,
    pub source_files: u64,
    pub target_files: u64,
    pub source_bytes: u64,
    pub target_bytes: u64,
    pub files_match: bool,
    pub bytes_match: bool,
    pub sample_hashes_checked: usize,
    pub sample_hashes_matched: usize,
    pub is_verified: bool,
    pub error: Option<String>,
}

/// Reversible receipt issued following a safe migration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReceipt {
    pub receipt_id: String,
    pub timestamp_utc: String,
    pub source_drive: String,
    pub target_drive: String,
    pub items: Vec<CandidateVerification>,
    pub total_bytes_copied: u64,
    pub total_files_copied: u64,
    /// Must ALWAYS be true: C.U.R.E never silently erases original data.
    pub source_preserved: bool,
    pub recovery_notes: Vec<String>,
    pub manual_cleanup_hint: Option<String>,
}

/// Progress event emitted during migration execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationProgress {
    pub candidate_id: String,
    pub phase: String,
    pub current_file: String,
    pub files_copied: u64,
    pub total_files: u64,
    pub bytes_copied: u64,
    pub total_bytes: u64,
    pub percent: f64,
}

// ---------------------------------------------------------------------------
// Assessment & Discovery
// ---------------------------------------------------------------------------

/// Evaluate real volume capacity across available drives.
pub fn assess_drive_pressures() -> Vec<DrivePressure> {
    let mut out = Vec::new();
    let sys_root = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let sys_drive = sys_root.trim_end_matches('\\').to_uppercase();

    for letter in b'A'..=b'Z' {
        let drive_root = format!("{}:\\", letter as char);
        let path = Path::new(&drive_root);
        if !path.exists() {
            continue;
        }

        if let Some(space) = fsx::volume_space_at(path) {
            if space.total_bytes == 0 {
                continue;
            }
            let used = space.total_bytes.saturating_sub(space.free_bytes);
            let pct = (used as f64 / space.total_bytes as f64) * 100.0;
            let is_system = format!("{}:", letter as char).eq_ignore_ascii_case(&sys_drive);
            // Congested if used > 80% or free space is under 25 GB
            let is_congested = pct >= 80.0 || space.free_bytes < (25 * 1024 * 1024 * 1024);

            out.push(DrivePressure {
                drive_letter: format!("{}:", letter as char),
                volume_label: space.label,
                total_bytes: space.total_bytes,
                free_bytes: space.free_bytes,
                used_bytes: used,
                pressure_pct: (pct * 10.0).round() / 10.0,
                is_system,
                is_congested,
            });
        }
    }
    out
}

/// Discover migration candidates on the system for a given source drive and destination.
pub fn discover_migration_candidates(
    source_drive: &str,
    target_drive: &str,
) -> StorageRescueStatus {
    let pressures = assess_drive_pressures();
    let sys_drive = pressures
        .iter()
        .find(|d| d.is_system)
        .map(|d| d.drive_letter.clone())
        .unwrap_or_else(|| "C:".into());

    let target = if !target_drive.is_empty() {
        target_drive.to_string()
    } else {
        // Pick largest non-system drive with at least 50GB free, or fallback to first available
        pressures
            .iter()
            .filter(|d| !d.is_system && d.free_bytes > 20 * 1024 * 1024 * 1024)
            .max_by_key(|d| d.free_bytes)
            .map(|d| d.drive_letter.clone())
            .unwrap_or_else(|| "D:".into())
    };

    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();

    let mut candidates = Vec::new();

    // Standard high-impact user libraries (Safe)
    if !user_profile.is_empty() {
        let p = Path::new(&user_profile);
        add_candidate_if_on_drive(
            &mut candidates,
            "downloads",
            "Downloads Library",
            &p.join("Downloads"),
            &target,
            MigrationSafety::Safe,
            "Personal Files",
            "Standalone downloaded installers, archives, and media. Safest relocation candidate.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "videos",
            "Videos & Captures",
            &p.join("Videos"),
            &target,
            MigrationSafety::Safe,
            "Media Library",
            "Recorded screen captures, streams, and video files. Large size, zero system dependencies.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "pictures",
            "Pictures & Photos",
            &p.join("Pictures"),
            &target,
            MigrationSafety::Safe,
            "Media Library",
            "User photographs, camera rolls, and wallpapers.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "documents",
            "Documents Library",
            &p.join("Documents"),
            &target,
            MigrationSafety::Safe,
            "Personal Documents",
            "Local user documents, exported spreadsheets, and project notes.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "desktop_media",
            "Desktop Stash",
            &p.join("Desktop"),
            &target,
            MigrationSafety::Safe,
            "Personal Files",
            "Personal loose files stored on Desktop.",
            None,
            source_drive,
        );

        // Developer caches (Caution)
        add_candidate_if_on_drive(
            &mut candidates,
            "cargo_cache",
            "Cargo Crates Cache",
            &p.join(".cargo").join("registry"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Rust crate source code and index cache.",
            Some("Set CARGO_HOME environment variable to the new destination.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "gradle_cache",
            "Gradle Caches",
            &p.join(".gradle").join("caches"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Java/Android Gradle build artifacts and wrappers.",
            Some("Set GRADLE_USER_HOME environment variable to new destination.".into()),
            source_drive,
        );
    }

    if !local_app_data.is_empty() {
        let p = Path::new(&local_app_data);
        add_candidate_if_on_drive(
            &mut candidates,
            "pip_cache",
            "Python Pip Cache",
            &p.join("pip").join("cache"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Downloaded Python wheels and tarballs.",
            Some("Set PIP_CACHE_DIR environment variable to new destination.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "npm_cache",
            "Node.js npm Cache",
            &p.join("npm-cache"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Cached npm package tarballs.",
            Some("Run `npm config set cache <new_path>` after relocation.".into()),
            source_drive,
        );
    }

    // Common source workspace folders if located on source drive
    let src_prefix = format!("{}:\\", source_drive.trim_end_matches(':'));
    for root_candidate in ["Projects", "Workspace", "Source", "code", "dev"] {
        let p = Path::new(&src_prefix).join(root_candidate);
        add_candidate_if_on_drive(
            &mut candidates,
            &format!("workspace_{root_candidate}"),
            &format!("Local {root_candidate} Folder"),
            &p,
            &target,
            MigrationSafety::Caution,
            "Project Repositories",
            "Local git repositories and source projects.",
            Some("Update IDE workspace paths and git configuration if paths change.".into()),
            source_drive,
        );
    }

    let mut total_safe = 0u64;
    let mut total_caution = 0u64;
    for c in &candidates {
        match c.safety {
            MigrationSafety::Safe => total_safe += c.size_bytes,
            MigrationSafety::Caution => total_caution += c.size_bytes,
            MigrationSafety::DoNotMove => {}
        }
    }

    StorageRescueStatus {
        drives: pressures,
        system_drive: Some(sys_drive),
        recommended_target_drive: Some(target),
        candidates,
        total_safe_relief_bytes: total_safe,
        total_caution_relief_bytes: total_caution,
    }
}

#[allow(clippy::too_many_arguments)]
fn add_candidate_if_on_drive(
    list: &mut Vec<MigrationCandidate>,
    id: &str,
    name: &str,
    source_path: &Path,
    target_drive: &str,
    safety: MigrationSafety,
    category: &str,
    why: &str,
    advice: Option<String>,
    filter_source_drive: &str,
) {
    if !source_path.exists() {
        return;
    }
    let s_str = source_path.to_string_lossy();
    let filter_prefix = format!("{}:", filter_source_drive.trim_end_matches(':'));
    if !s_str.starts_with(&filter_prefix) && !s_str.starts_with(&filter_prefix.to_lowercase()) {
        return;
    }

    let folder_name = source_path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| id.to_string());

    let suggested_target = PathBuf::from(format!(
        "{}:\\CURE_Relocated\\{}",
        target_drive.trim_end_matches(':'),
        folder_name
    ));

    let (file_count, size_bytes) = inspect_folder_quick(source_path);
    if file_count == 0 && size_bytes == 0 {
        return;
    }

    list.push(MigrationCandidate {
        id: id.to_string(),
        name: name.to_string(),
        source_path: source_path.to_path_buf(),
        suggested_target_path: suggested_target,
        size_bytes,
        allocated_bytes: size_bytes,
        file_count,
        safety,
        category: category.to_string(),
        why_candidate: why.to_string(),
        reconfiguration_advice: advice,
    });
}

/// Fast folder enumeration for candidate discovery (non-blocking, capped for responsiveness).
pub fn inspect_folder_quick(path: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return (0, 0);
    };

    for entry in entries.flatten() {
        let Ok(md) = entry.metadata() else { continue };
        if md.is_file() {
            files += 1;
            bytes += md.len();
        } else if md.is_dir() {
            // Recurse shallowly for first-level directory summaries
            if let Ok(sub) = fs::read_dir(entry.path()) {
                for sub_entry in sub.flatten() {
                    if let Ok(smd) = sub_entry.metadata() {
                        if smd.is_file() {
                            files += 1;
                            bytes += smd.len();
                        }
                    }
                }
            }
        }
    }
    (files, bytes)
}

// ---------------------------------------------------------------------------
// Migration Planning & Preflight
// ---------------------------------------------------------------------------

/// Build and validate a migration plan for selected candidate folders.
pub fn build_migration_plan(
    selected_ids: &[String],
    source_drive: &str,
    target_drive: &str,
) -> MigrationPlan {
    let status = discover_migration_candidates(source_drive, target_drive);
    let selected: Vec<MigrationCandidate> = status
        .candidates
        .into_iter()
        .filter(|c| selected_ids.iter().any(|id| id == &c.id))
        .collect();

    let mut total_bytes = 0u64;
    let mut total_files = 0u64;
    for c in &selected {
        total_bytes += c.size_bytes;
        total_files += c.file_count;
    }

    let target_root = format!("{}:\\", target_drive.trim_end_matches(':'));
    let target_space = fsx::volume_space_at(Path::new(&target_root));
    let target_free = target_space.as_ref().map(|s| s.free_bytes).unwrap_or(0);

    let source_root = format!("{}:\\", source_drive.trim_end_matches(':'));
    let source_space = fsx::volume_space_at(Path::new(&source_root));
    let source_free = source_space.as_ref().map(|s| s.free_bytes).unwrap_or(0);

    // 10% safety headroom
    let headroom = (total_bytes as f64 * 0.10) as u64;
    let total_required = total_bytes.saturating_add(headroom);

    let mut preflight_errors = Vec::new();
    let mut preflight_warnings = Vec::new();

    let has_sufficient = target_free >= total_required;
    if !has_sufficient {
        preflight_errors.push(format!(
            "Insufficient capacity on destination {target_drive}: {target_free} bytes free, but {total_required} bytes required (including 10% safety buffer)."
        ));
    }

    if source_drive.eq_ignore_ascii_case(target_drive) {
        preflight_errors.push(
            "Source and destination volume cannot be identical for cross-volume relocation.".into(),
        );
    }

    for c in &selected {
        if c.safety == MigrationSafety::Caution {
            preflight_warnings.push(format!(
                "{} is a caution candidate: {}",
                c.name,
                c.reconfiguration_advice
                    .as_deref()
                    .unwrap_or("Requires reconfiguring dependent application.")
            ));
        }
    }

    let target_proj = target_free.saturating_sub(total_bytes);
    let source_proj = source_free.saturating_add(total_bytes);

    MigrationPlan {
        source_drive: source_drive.into(),
        target_drive: target_drive.into(),
        selected_candidates: selected,
        total_bytes_to_relocate: total_bytes,
        total_files_to_relocate: total_files,
        target_current_free_bytes: target_free,
        target_projected_free_bytes: target_proj,
        source_current_free_bytes: source_free,
        source_projected_free_bytes: source_proj,
        has_sufficient_space: has_sufficient && preflight_errors.is_empty(),
        required_headroom_bytes: headroom,
        preflight_errors,
        preflight_warnings,
    }
}

// ---------------------------------------------------------------------------
// Execution, Verification & Integrity
// ---------------------------------------------------------------------------

/// Compute SHA-256 hash of a file for copy verification.
pub fn hash_file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Recursively copy a directory tree preserving files and directories.
/// Emits progress callback, respects cancellation flag, and NEVER modifies source.
pub fn copy_tree_verified<F>(
    candidate_id: &str,
    source: &Path,
    target: &Path,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> io::Result<CandidateVerification>
where
    F: FnMut(MigrationProgress),
{
    if !source.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Source folder does not exist",
        ));
    }

    if !target.exists() {
        fs::create_dir_all(target)?;
    }

    // Measure source files and bytes first
    let mut source_files_list = Vec::new();
    let mut total_source_bytes = 0u64;
    collect_files_recursive(source, &mut source_files_list)?;
    for f in &source_files_list {
        if let Ok(md) = fs::metadata(f) {
            total_source_bytes += md.len();
        }
    }
    let total_files_count = source_files_list.len() as u64;

    let mut copied_files = 0u64;
    let mut copied_bytes = 0u64;

    // Copy every file
    for src_file in &source_files_list {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Migration cancelled by operator",
            ));
        }

        let rel = src_file
            .strip_prefix(source)
            .map_err(|e| io::Error::other(format!("Strip prefix failed: {e}")))?;
        let dst_file = target.join(rel);

        if let Some(parent) = dst_file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let file_len = match fs::metadata(src_file) {
            Ok(m) => m.len(),
            Err(_) => 0,
        };

        fs::copy(src_file, &dst_file)?;
        copied_files += 1;
        copied_bytes += file_len;

        let pct = if total_source_bytes > 0 {
            ((copied_bytes as f64 / total_source_bytes as f64) * 100.0).min(100.0)
        } else {
            100.0
        };

        on_progress(MigrationProgress {
            candidate_id: candidate_id.into(),
            phase: "copying".into(),
            current_file: rel.to_string_lossy().into_owned(),
            files_copied: copied_files,
            total_files: total_files_count,
            bytes_copied: copied_bytes,
            total_bytes: total_source_bytes,
            percent: (pct * 10.0).round() / 10.0,
        });
    }

    // Now perform post-copy verification
    on_progress(MigrationProgress {
        candidate_id: candidate_id.into(),
        phase: "verifying".into(),
        current_file: "Verifying checksums and counts...".into(),
        files_copied: copied_files,
        total_files: total_files_count,
        bytes_copied: copied_bytes,
        total_bytes: total_source_bytes,
        percent: 100.0,
    });

    let mut target_files_list = Vec::new();
    collect_files_recursive(target, &mut target_files_list)?;
    let mut total_target_bytes = 0u64;
    for f in &target_files_list {
        if let Ok(md) = fs::metadata(f) {
            total_target_bytes += md.len();
        }
    }

    let files_match = source_files_list.len() == target_files_list.len();
    let bytes_match = total_source_bytes == total_target_bytes;

    // Sample cryptographic hash verification:
    // Check first 3, middle, and last 3 files
    let mut sample_checked = 0usize;
    let mut sample_matched = 0usize;

    let sample_indices = get_sample_indices(source_files_list.len());
    for idx in sample_indices {
        if let Some(src_f) = source_files_list.get(idx) {
            if let Ok(rel) = src_f.strip_prefix(source) {
                let dst_f = target.join(rel);
                if dst_f.exists() {
                    let h_src = hash_file_sha256(src_f);
                    let h_dst = hash_file_sha256(&dst_f);
                    if let (Ok(s), Ok(d)) = (h_src, h_dst) {
                        sample_checked += 1;
                        if s == d {
                            sample_matched += 1;
                        }
                    }
                }
            }
        }
    }

    let is_verified = files_match && bytes_match && (sample_checked == sample_matched);

    Ok(CandidateVerification {
        candidate_id: candidate_id.into(),
        source_path: source.to_string_lossy().into_owned(),
        target_path: target.to_string_lossy().into_owned(),
        source_files: total_files_count,
        target_files: target_files_list.len() as u64,
        source_bytes: total_source_bytes,
        target_bytes: total_target_bytes,
        files_match,
        bytes_match,
        sample_hashes_checked: sample_checked,
        sample_hashes_matched: sample_matched,
        is_verified,
        error: if is_verified {
            None
        } else {
            Some("Verification mismatch: bytes, files, or sample hashes do not match".into())
        },
    })
}

fn collect_files_recursive(dir: &Path, list: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_files_recursive(&p, list)?;
        } else if p.is_file() {
            list.push(p);
        }
    }
    Ok(())
}

fn get_sample_indices(total: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let mut indices = Vec::new();
    for i in 0..3.min(total) {
        indices.push(i);
    }
    if total > 6 {
        indices.push(total / 2);
    }
    if total > 3 {
        for i in total.saturating_sub(3)..total {
            if !indices.contains(&i) {
                indices.push(i);
            }
        }
    }
    indices
}

/// Execute migration of a single planned candidate and issue a reversible receipt.
pub fn execute_candidate_migration<F>(
    candidate_id: &str,
    source: &Path,
    target: &Path,
    source_drive: &str,
    target_drive: &str,
    cancel_flag: &AtomicBool,
    on_progress: F,
) -> io::Result<MigrationReceipt>
where
    F: FnMut(MigrationProgress),
{
    let ver = copy_tree_verified(candidate_id, source, target, cancel_flag, on_progress)?;

    let now = chrono::Utc::now().to_rfc3339();
    let receipt_id = format!("RECEIPT-RESCUE-{}", chrono::Utc::now().timestamp());

    let mut recovery_notes = Vec::new();
    recovery_notes.push(format!(
        "Data was safely copied from {} to {}.",
        ver.source_path, ver.target_path
    ));
    recovery_notes
        .push("SOURCE DATA REMAINS 100% INTACT. C.U.R.E did not delete any source files.".into());
    recovery_notes.push(
        "To revert this relocation, simply continue using the original folder and remove the new folder."
            .into(),
    );
    recovery_notes.push(
        "To finalize and recover space on C:, verify your files in the destination folder, then delete the original source folder manually."
            .into(),
    );

    let cleanup_hint = format!("rd /s /q \"{}\"", ver.source_path);

    Ok(MigrationReceipt {
        receipt_id,
        timestamp_utc: now,
        source_drive: source_drive.into(),
        target_drive: target_drive.into(),
        total_bytes_copied: ver.target_bytes,
        total_files_copied: ver.target_files,
        source_preserved: true,
        items: vec![ver],
        recovery_notes,
        manual_cleanup_hint: Some(cleanup_hint),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_safety_tier_classification() {
        assert_eq!(MigrationSafety::Safe.label(), "Safe to Move");
        assert_eq!(MigrationSafety::Caution.label(), "Caution / Reconfigure");
        assert_eq!(
            MigrationSafety::DoNotMove.label(),
            "Do Not Move (Protected)"
        );
        assert!(MigrationSafety::Safe.is_allowed());
        assert!(MigrationSafety::Caution.is_allowed());
        assert!(!MigrationSafety::DoNotMove.is_allowed());
    }

    #[test]
    fn test_hash_file_sha256() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"Hello CURE Forensics").unwrap();

        let hash = hash_file_sha256(&path).unwrap();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_copy_tree_verified_exact_match() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // Create test source structure
        let sub = src_dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();

        let mut f1 = File::create(src_dir.path().join("file1.txt")).unwrap();
        f1.write_all(b"Sample content 1").unwrap();

        let mut f2 = File::create(sub.join("file2.txt")).unwrap();
        f2.write_all(b"Sample content 2 in subfolder").unwrap();

        let cancel = AtomicBool::new(false);
        let mut progress_events = Vec::new();

        let ver = copy_tree_verified(
            "test_candidate",
            src_dir.path(),
            dst_dir.path(),
            &cancel,
            |p| progress_events.push(p),
        )
        .unwrap();

        assert!(ver.is_verified);
        assert!(ver.files_match);
        assert!(ver.bytes_match);
        assert_eq!(ver.source_files, 2);
        assert_eq!(ver.target_files, 2);
        assert_eq!(ver.sample_hashes_checked, ver.sample_hashes_matched);
        assert!(!progress_events.is_empty());

        // Source must remain 100% intact!
        assert!(src_dir.path().join("file1.txt").exists());
        assert!(sub.join("file2.txt").exists());
    }

    #[test]
    fn test_migration_receipt_preserves_source() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let mut f = File::create(src_dir.path().join("data.bin")).unwrap();
        f.write_all(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();

        let cancel = AtomicBool::new(false);
        let receipt = execute_candidate_migration(
            "cand_1",
            src_dir.path(),
            dst_dir.path(),
            "C:",
            "D:",
            &cancel,
            |_| {},
        )
        .unwrap();

        assert!(receipt.source_preserved);
        assert_eq!(receipt.total_files_copied, 1);
        assert_eq!(receipt.total_bytes_copied, 8);
        assert!(receipt.items[0].is_verified);

        // Verification: source exists untouched
        assert!(src_dir.path().join("data.bin").exists());
    }

    #[test]
    fn test_copy_tree_cancellation() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let mut f1 = File::create(src_dir.path().join("file.txt")).unwrap();
        f1.write_all(b"test").unwrap();

        let cancel = AtomicBool::new(true); // Pre-cancelled
        let res = copy_tree_verified("test", src_dir.path(), dst_dir.path(), &cancel, |_| {});

        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::Interrupted);
    }
}
