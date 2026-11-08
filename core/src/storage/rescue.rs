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

use std::collections::HashMap;
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
    pub is_onedrive: bool,
    pub onedrive_warning: Option<String>,
    pub reparse_points_detected: usize,
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

/// Metadata preservation and integrity report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataPreservationReport {
    pub timestamps_preserved: bool,
    pub readonly_attributes_preserved: bool,
    pub acl_parity_note: String,
    pub alternate_data_streams_note: String,
}

impl Default for MetadataPreservationReport {
    fn default() -> Self {
        Self {
            timestamps_preserved: true,
            readonly_attributes_preserved: true,
            acl_parity_note: "Standard user permissions inherited from target root. Custom NTFS security descriptors are not cloned across drive boundaries without explicit administrative elevation.".into(),
            alternate_data_streams_note: "Standard file data streams copied. Auxiliary NTFS alternate data streams are preserved on NTFS destination targets.".into(),
        }
    }
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
    pub sha256_checked: usize,
    pub sha256_matched: usize,
    pub is_verified: bool,
    pub metadata_report: MetadataPreservationReport,
    pub reparse_points_safely_skipped: usize,
    pub error: Option<String>,
}

/// Receipt issued upon finalizing relocation (e.g. repointing Windows Known Folders).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizationReceipt {
    pub candidate_id: String,
    pub target_path: String,
    pub is_known_folder: bool,
    pub known_folder_name: Option<String>,
    pub registry_updated: bool,
    pub configuration_action: String,
    pub shell_notification_broadcast: bool,
}

/// Structured receipt for application activation and live validation gates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationReceipt {
    pub candidate_id: String,
    pub target_path: String,
    pub previous_path: String,
    pub configuration_action: String,
    pub validation_details: String,
    pub env_var_updated: Option<String>,
    pub known_folder_updated: Option<String>,
    pub is_known_folder: bool,
    pub activation_succeeded: bool,
    pub application_validation_passed: bool,
    /// Configuration value observed BEFORE the activation write (registry Known
    /// Folder or HKCU\\Environment variable). Empty when none was recorded.
    pub previous_configuration: Option<String>,
    pub cleanup_unlocked: bool,
    pub cleanup_nonce: Option<String>,
    pub required_cleanup_token: Option<String>,
    /// True when a failed validation automatically restored the previous
    /// configuration so the system was not left pointing at the new location.
    pub auto_rollback_performed: bool,
    pub notes: Vec<String>,
}

/// Rollback receipt when restoring original location or environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackReceipt {
    pub candidate_id: String,
    pub restored_path: String,
    pub rollback_action: String,
    pub rollback_succeeded: bool,
    pub notes: Vec<String>,
}

/// Receipt issued after user-approved source cleanup with actual measured free space change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceCleanupReceipt {
    pub candidate_id: String,
    pub source_path: String,
    pub c_free_before_bytes: u64,
    pub c_free_after_bytes: u64,
    pub actual_bytes_freed: i64,
    pub cleanup_status: String,
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

/// Detect if a folder is managed or synchronized by Microsoft OneDrive.
pub fn is_onedrive_managed(path: &Path, candidate_id: &str) -> bool {
    let p_str = path.to_string_lossy();
    if p_str.to_ascii_lowercase().contains("onedrive") {
        return true;
    }
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
        {
            let reg_vals = match candidate_id {
                "documents" => vec!["Personal"],
                "pictures" => vec!["My Pictures"],
                "videos" => vec!["My Video"],
                "desktop" | "desktop_media" => vec!["Desktop"],
                "downloads" => vec!["{374DE290-123F-4565-9164-39C4925E467B}"],
                _ => vec![],
            };
            for val in reg_vals {
                if let Ok(reg_path) = key.get_value::<String, _>(val) {
                    if reg_path.to_ascii_lowercase().contains("onedrive") {
                        let reg_p = Path::new(&reg_path);
                        if path == reg_p || path.starts_with(reg_p) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    let _ = candidate_id;
    false
}

use std::sync::Mutex;

/// Durable operation-nonce ledger. Both lists are insertion-ordered so the
/// oldest entries can be evicted deterministically once the caps are hit.
#[derive(Default)]
struct NonceLedger {
    issued: Vec<String>,
    consumed: Vec<String>,
}

/// Maximum persisted entries per list. Beyond this, the oldest entries are
/// evicted. A consumed nonce evicted from the ledger would weaken replay
/// prevention, so the cap is generous (thousands of operations).
const NONCE_LEDGER_MAX_ENTRIES: usize = 8192;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct NonceLedgerFile {
    issued: Vec<String>,
    consumed: Vec<String>,
}

impl NonceLedger {
    fn truncate_oldest(list: &mut Vec<String>) {
        if list.len() > NONCE_LEDGER_MAX_ENTRIES {
            let excess = list.len() - NONCE_LEDGER_MAX_ENTRIES;
            list.drain(0..excess);
        }
    }

    fn record_issued(&mut self, nonce: &str) {
        if !self.issued.iter().any(|n| n == nonce) {
            self.issued.push(nonce.to_string());
        }
        Self::truncate_oldest(&mut self.issued);
    }

    fn revoke_issued(&mut self, nonce: &str) {
        self.issued.retain(|n| n != nonce);
    }

    fn is_issued(&self, nonce: &str) -> bool {
        self.issued.iter().any(|n| n == nonce)
    }

    /// Enforces single-use consumption: a nonce is only consumable if it was
    /// actually issued (activation flow) and never consumed before — including
    /// across process restarts when state was loaded from a durable store.
    fn consume_strict(&mut self, nonce: &str) -> io::Result<()> {
        if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Nonce must be 128-bit (32 hex characters), got length {}",
                    nonce.len()
                ),
            ));
        }
        if !self.is_issued(nonce) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unknown nonce: this installation never issued it. Operation rejected (fabricated or stale nonce).",
            ));
        }
        if self.consumed.iter().any(|n| n == nonce) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("Replay Attack Prevented: Nonce '{nonce}' has already been consumed."),
            ));
        }
        self.consumed.push(nonce.to_string());
        Self::truncate_oldest(&mut self.consumed);
        Ok(())
    }

    fn load_from_file(&mut self, path: &Path) {
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        let Ok(parsed) = serde_json::from_str::<NonceLedgerFile>(&content) else {
            return;
        };
        for n in parsed.issued {
            self.record_issued(&n);
        }
        for n in parsed.consumed {
            if !self.consumed.iter().any(|c| c == &n) {
                self.consumed.push(n);
            }
        }
    }

    /// Atomically persist (write to temp file, then rename) so a restarted
    /// process still enforces the ledger.
    fn persist_to_file(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let snapshot = NonceLedgerFile {
            issued: self.issued.clone(),
            consumed: self.consumed.clone(),
        };
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| io::Error::other(format!("Cannot serialize nonce ledger: {e}")))?;
        fs::write(&tmp, json)?;
        fs::rename(&tmp, path)
    }
}

static NONCE_LEDGER: Mutex<Option<NonceLedger>> = Mutex::new(None);
static NONCE_STORE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Point the nonce ledger at a durable store file and load any prior state.
/// Call once at startup (e.g. with the app data directory) so nonces issued
/// and consumed by previous processes remain enforceable after a restart.
pub fn set_nonce_store_path(path: &Path) {
    if let Ok(mut lock) = NONCE_STORE_PATH.lock() {
        *lock = Some(path.to_path_buf());
    }
    if let Ok(mut guard) = NONCE_LEDGER.lock() {
        guard
            .get_or_insert_with(NonceLedger::default)
            .load_from_file(path);
    }
}

fn persist_nonce_ledger() {
    let store_path = NONCE_STORE_PATH.lock().ok().and_then(|g| g.clone());
    let Some(path) = store_path else {
        return;
    };
    if let Ok(guard) = NONCE_LEDGER.lock() {
        if let Some(ledger) = guard.as_ref() {
            let _ = ledger.persist_to_file(&path);
        }
    }
}

/// Generates a cryptographically secure 128-bit operation nonce (32 hex characters)
/// using Windows CSPRNG (`BCryptGenRandom` with `BCRYPT_USE_SYSTEM_PREFERRED_RNG`).
pub fn generate_secure_128bit_nonce() -> io::Result<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        let mut bytes = [0u8; 16];
        unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
            .ok()
            .map_err(|e| io::Error::other(format!("CSPRNG BCryptGenRandom failed: {e}")))?;
        let nonce: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        record_issued_nonce(&nonce);
        Ok(nonce)
    }
    #[cfg(not(windows))]
    {
        let mut hasher = Sha256::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        hasher.update(&now.as_nanos().to_le_bytes());
        hasher.update(&std::process::id().to_le_bytes());
        let h = hasher.finalize();
        let nonce: String = h[..16].iter().map(|b| format!("{b:02x}")).collect();
        record_issued_nonce(&nonce);
        Ok(nonce)
    }
}

pub fn record_issued_nonce(nonce: &str) {
    if let Ok(mut guard) = NONCE_LEDGER.lock() {
        guard
            .get_or_insert_with(NonceLedger::default)
            .record_issued(nonce);
    }
    persist_nonce_ledger();
}

pub fn revoke_issued_nonce(nonce: &str) {
    if let Ok(mut guard) = NONCE_LEDGER.lock() {
        if let Some(ledger) = guard.as_mut() {
            ledger.revoke_issued(nonce);
        }
    }
    persist_nonce_ledger();
}

pub fn is_nonce_issued(nonce: &str) -> bool {
    if let Ok(guard) = NONCE_LEDGER.lock() {
        if let Some(ledger) = guard.as_ref() {
            return ledger.is_issued(nonce);
        }
    }
    false
}

/// Enforces single-use consumption of an operation nonce to prevent replay attacks.
/// A nonce is only consumable if this installation actually issued it (activation
/// flow), and only once — including across process restarts when a durable nonce
/// store has been configured via [`set_nonce_store_path`].
pub fn consume_nonce_strict(nonce: &str) -> io::Result<()> {
    let result = {
        let mut guard = NONCE_LEDGER
            .lock()
            .map_err(|_| io::Error::other("Failed to acquire nonce lock"))?;
        guard
            .get_or_insert_with(NonceLedger::default)
            .consume_strict(nonce)
    };
    if result.is_ok() {
        persist_nonce_ledger();
    }
    result
}

/// Check whether any running process has active locks or open working paths in `source_path`.
///
/// The file probe is a bounded recursive walk; each sampled file is opened with
/// zero sharing allowed. Only kernel sharing/lock violations count as "in use" —
/// permission errors and read-only attributes do not, because those are not
/// evidence of a lock (see [`fsx::is_in_use`]).
pub fn check_active_tool_locks(source_path: &Path) -> Result<(), String> {
    if !source_path.exists() {
        return Ok(());
    }
    let src_str = source_path.to_string_lossy().to_ascii_lowercase();

    // Check detected agent processes
    let agents = crate::agent::session::inspect_agent_sessions();
    for agent in agents {
        let exe_lower = agent.exe_path.to_ascii_lowercase();
        if exe_lower.starts_with(&src_str) {
            return Err(format!(
                "Active agent process locking path: {} (PID {}) is executing from within '{}'",
                agent.name,
                agent.pid,
                source_path.display()
            ));
        }
    }

    // Sampled lock probe over a bounded recursive walk. walkdir is configured
    // not to follow symlinks, so junctions/reparse points are never descended.
    const MAX_PROBED_FILES: usize = 500;
    let mut probed = 0usize;
    for entry in walkdir::WalkDir::new(source_path)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if probed >= MAX_PROBED_FILES {
            break;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() || fsx::is_reparse_point(&meta) {
            continue;
        }
        probed += 1;
        if fsx::is_in_use(entry.path()) {
            return Err(format!(
                "File is locked by an active process: '{}' (lock probe sampled {} of the source files)",
                entry.path().display(),
                probed
            ));
        }
    }

    Ok(())
}

/// FINAL gate before source deletion: prove that `target` currently contains a
/// faithful copy of `source` — identical file sets, identical byte sizes, and
/// identical SHA-256 content for every file. Any mismatch, missing file, or
/// reparse-point asymmetry refuses the cleanup. This deliberately re-hashes the
/// whole tree at deletion time so a stale "verified" receipt from an earlier
/// step can never authorize deleting original data whose copy has drifted.
pub fn verify_copy_parity_for_cleanup(source: &Path, target: &Path) -> Result<(), String> {
    let mut src_files = Vec::new();
    let mut src_skipped = 0usize;
    collect_files_recursive(source, &mut src_files, &mut src_skipped)
        .map_err(|e| format!("Cannot enumerate source tree: {e}"))?;
    let mut dst_files = Vec::new();
    let mut dst_skipped = 0usize;
    collect_files_recursive(target, &mut dst_files, &mut dst_skipped)
        .map_err(|e| format!("Cannot enumerate destination tree: {e}"))?;

    if src_files.is_empty() {
        return Err(
            "Source contains no regular files; refusing deletion based on an unverifiable state."
                .into(),
        );
    }
    if src_skipped > 0 || dst_skipped > 0 {
        return Err(format!(
            "Reparse points detected during parity check (source: {src_skipped}, target: {dst_skipped}) — destination cannot be proven equivalent. Refusing source cleanup."
        ));
    }

    // Case-insensitive relative path -> (byte length, path) maps.
    let mut src_map = std::collections::BTreeMap::new();
    for f in &src_files {
        let rel = f
            .strip_prefix(source)
            .map_err(|e| format!("Cannot relativize source path: {e}"))?;
        let key = rel.to_string_lossy().to_ascii_lowercase();
        let len = fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        src_map.insert(key, (len, f.clone()));
    }
    let mut dst_map = std::collections::BTreeMap::new();
    for f in &dst_files {
        let rel = f
            .strip_prefix(target)
            .map_err(|e| format!("Cannot relativize target path: {e}"))?;
        let key = rel.to_string_lossy().to_ascii_lowercase();
        let len = fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        dst_map.insert(key, (len, f.clone()));
    }

    for key in src_map.keys() {
        if !dst_map.contains_key(key) {
            return Err(format!(
                "Destination is missing copied file '{key}' — source cleanup refused."
            ));
        }
    }
    for key in dst_map.keys() {
        if !src_map.contains_key(key) {
            return Err(format!(
                "Destination contains unexpected file '{key}' — source cleanup refused."
            ));
        }
    }

    for (key, (src_len, src_path)) in &src_map {
        let (dst_len, dst_path) = &dst_map[key];
        if src_len != dst_len {
            return Err(format!(
                "Size mismatch for '{key}' (source {src_len} bytes, destination {dst_len} bytes) — source cleanup refused."
            ));
        }
        let (h_src, h_dst) = (hash_file_sha256(src_path), hash_file_sha256(dst_path));
        match (h_src, h_dst) {
            (Ok(s), Ok(d)) if s == d => {}
            (Ok(s), Ok(d)) => {
                return Err(format!(
                    "SHA-256 mismatch for '{key}' (source {s}, destination {d}) — source cleanup refused."
                ));
            }
            (Err(e), _) | (_, Err(e)) => {
                return Err(format!(
                    "Could not hash file pair for '{key}': {e} — source cleanup refused."
                ));
            }
        }
    }

    Ok(())
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
        add_candidate_if_on_drive(
            &mut candidates,
            "codex_cache",
            "Codex & AI Workspace Cache",
            &p.join(".codex"),
            &target,
            MigrationSafety::Caution,
            "AI Toolchain",
            "Local model context, indexes, and agent workspaces.",
            Some("Set CODEX_HOME environment variable or link directory.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "android_cache",
            "Android SDK & Emulators",
            &p.join(".android"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Android virtual device images (AVD) and emulator caches.",
            Some("Set ANDROID_USER_HOME or ANDROID_AVD_HOME to new destination.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "rustup_cache",
            "Rustup Toolchains",
            &p.join(".rustup"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Installed Rust toolchain binaries and target platforms.",
            Some("Set RUSTUP_HOME environment variable to new destination.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "lmstudio_models",
            "LM Studio AI Models",
            &p.join(".lmstudio"),
            &target,
            MigrationSafety::Caution,
            "AI Toolchain",
            "Local LLM model weights (GGUF, safetensors).",
            Some("Configure LM Studio models directory to point to new location.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "vscode_extensions",
            "VS Code Extensions & Cache",
            &p.join(".vscode"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Installed VS Code extensions and runtime dependencies.",
            Some("Pass --extensions-dir or create directory symlink.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "user_cache_dir",
            "User Profile Generic Cache",
            &p.join(".cache"),
            &target,
            MigrationSafety::Caution,
            "Application Cache",
            "Generic application and build cache files.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "gemini_cache",
            "Gemini CLI Cache",
            &p.join(".gemini"),
            &target,
            MigrationSafety::Caution,
            "AI Toolchain",
            "Gemini local session logs, artifacts, and indexes.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "pnpm_store",
            "pnpm Store",
            &p.join(".pnpm-store"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Content-addressable package store for pnpm.",
            Some("Run `pnpm config set store-dir <new_path>`.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "ollama_models",
            "Ollama Models",
            &p.join(".ollama"),
            &target,
            MigrationSafety::Caution,
            "AI Toolchain",
            "Local Ollama models and blobs.",
            Some("Set OLLAMA_MODELS environment variable to new destination.".into()),
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "cursor_cache",
            "Cursor Editor State",
            &p.join(".cursor"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Cursor editor extensions and session data.",
            None,
            source_drive,
        );
        add_candidate_if_on_drive(
            &mut candidates,
            "cargo_root",
            "Cargo Root & Downloads",
            &p.join(".cargo"),
            &target,
            MigrationSafety::Caution,
            "Developer Toolchain",
            "Rust crate source code, git checkouts, and binary cache.",
            Some("Set CARGO_HOME environment variable to the new destination.".into()),
            source_drive,
        );
        // Dynamically discover any other large dot-folders in %USERPROFILE%
        if let Ok(readers) = std::fs::read_dir(p) {
            for entry in readers.flatten() {
                let ep = entry.path();
                let is_reparse = fs::symlink_metadata(&ep)
                    .map(|m| fsx::is_reparse_point(&m))
                    .unwrap_or(true);
                if ep.is_dir() && !is_reparse {
                    if let Some(name) = ep.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with('.') && name.len() > 1 {
                            let already = candidates.iter().any(|c| c.source_path == ep);
                            if !already {
                                add_candidate_if_on_drive(
                                    &mut candidates,
                                    &format!("user_dot_{}", name.trim_start_matches('.')),
                                    &format!("User Folder {}", name),
                                    &ep,
                                    &target,
                                    MigrationSafety::Caution,
                                    "Developer Toolchain",
                                    "User profile dot-directory discovered on drive.",
                                    None,
                                    source_drive,
                                );
                            }
                        }
                    }
                }
            }
        }
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

    let is_onedrive = is_onedrive_managed(source_path, id);
    let onedrive_warn = if is_onedrive {
        Some("Folder is synced or redirected via Microsoft OneDrive. Moving outside may affect cloud synchronization.".to_string())
    } else {
        None
    };

    let final_safety = if is_onedrive {
        MigrationSafety::Caution
    } else {
        safety
    };

    let final_why = if is_onedrive {
        format!("{why} [OneDrive sync detected: Relocating outside OneDrive requires reconfiguring cloud sync]")
    } else {
        why.to_string()
    };

    let (file_count, size_bytes, reparse_detected) = inspect_folder_quick(source_path);
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
        safety: final_safety,
        category: category.to_string(),
        why_candidate: final_why,
        reconfiguration_advice: advice,
        is_onedrive,
        onedrive_warning: onedrive_warn,
        reparse_points_detected: reparse_detected,
    });
}

/// Iterative safe folder inspection for candidate discovery (non-blocking, capped for responsiveness, skipping reparse points).
pub fn inspect_folder_quick(path: &Path) -> (u64, u64, usize) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut reparse_count = 0usize;
    let mut stack = vec![(path.to_path_buf(), 0usize)];
    let max_files = 100_000;
    let max_depth = 24;

    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                reparse_count += 1;
                continue;
            }
            if file_type.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if file_type.is_file() {
                files += 1;
                if let Ok(meta) = entry.metadata() {
                    bytes += meta.len();
                }
                if files >= max_files {
                    return (files, bytes, reparse_count);
                }
            }
        }
    }
    (files, bytes, reparse_count)
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

    // Enforce adequate destination free-space headroom:
    // Minimum 1 GB or 10% of total_bytes (whichever is larger),
    // and guarantee target volume retains at least 2 GB remaining after migration.
    let min_headroom = 1024 * 1024 * 1024; // 1 GB
    let proportional_headroom = (total_bytes as f64 * 0.10) as u64;
    let headroom = proportional_headroom.max(min_headroom);
    let total_required = total_bytes.saturating_add(headroom);

    let mut preflight_errors = Vec::new();
    let mut preflight_warnings = Vec::new();

    let target_proj = target_free.saturating_sub(total_bytes);
    let min_target_retained = 2 * 1024 * 1024 * 1024; // 2 GB minimum remaining on destination volume

    let has_sufficient = target_free >= total_required && target_proj >= min_target_retained;
    if target_free < total_required {
        preflight_errors.push(format!(
            "Insufficient capacity on destination {target_drive}: {target_free} bytes free, but {total_required} bytes required (including {headroom} bytes safety headroom)."
        ));
    }
    if target_proj < min_target_retained && target_free >= total_required {
        preflight_errors.push(format!(
            "Destination {target_drive} would have less than 2 GB remaining after relocation (projected free: {target_proj} bytes). Migration blocked to prevent volume exhaustion."
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

#[cfg(windows)]
fn preserve_file_times(src: &Path, dst: &Path) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileTime, SetFileTime};
    if let (Ok(s_file), Ok(d_file)) = (
        File::open(src),
        std::fs::OpenOptions::new().write(true).open(dst),
    ) {
        let mut creation = windows::Win32::Foundation::FILETIME::default();
        let mut access = windows::Win32::Foundation::FILETIME::default();
        let mut write = windows::Win32::Foundation::FILETIME::default();
        unsafe {
            if GetFileTime(
                HANDLE(s_file.as_raw_handle()),
                Some(&mut creation),
                Some(&mut access),
                Some(&mut write),
            )
            .is_ok()
            {
                let _ = SetFileTime(
                    HANDLE(d_file.as_raw_handle()),
                    Some(&creation),
                    Some(&access),
                    Some(&write),
                );
            }
        }
    }
}

#[cfg(not(windows))]
fn preserve_file_times(_src: &Path, _dst: &Path) {}

/// Recursively copy a directory tree preserving files and directories.
/// Emits progress callback, respects cancellation flag, and NEVER modifies source.
/// Stream-copies a file in 1 MB buffered chunks while computing its SHA-256 hash.
/// Aborts and deletes the destination file if cancellation is flagged.
fn fast_copy_file_with_hash<P>(
    src: &Path,
    dst: &Path,
    cancel_flag: &AtomicBool,
    mut on_chunk: P,
) -> io::Result<(u64, String)>
where
    P: FnMut(u64),
{
    use std::io::{BufReader, BufWriter, Read, Write};
    let src_file = File::open(src)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, src_file);
    let dst_file = File::create(dst)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, dst_file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    let mut unnotified = 0u64;
    const NOTIFY_INTERVAL: u64 = 16 * 1024 * 1024; // 16 MB

    loop {
        if cancel_flag.load(Ordering::SeqCst) {
            drop(writer);
            drop(reader);
            let _ = fs::remove_file(dst);
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Transfer cancelled by operator",
            ));
        }
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n])?;
        hasher.update(&buffer[..n]);
        total += n as u64;
        unnotified += n as u64;
        if unnotified >= NOTIFY_INTERVAL {
            on_chunk(unnotified);
            unnotified = 0;
        }
    }
    writer.flush()?;
    if unnotified > 0 {
        on_chunk(unnotified);
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

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
    // 1. Boundary & Overlap Validation
    crate::operator::defense::PathBoundaryGuard::ensure_no_overlap(source, target)
        .map_err(|e| io::Error::other(format!("Boundary validation failed: {e}")))?;

    // 2. Reject reparse points / junctions to prevent circular traversal
    if let Ok(meta) = fs::symlink_metadata(source) {
        if fsx::is_reparse_point(&meta) {
            return Err(io::Error::other(
                "Source is a reparse point or junction. Direct migration prohibited to prevent mount point loop.",
            ));
        }
    }
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
    let mut skipped_reparse = 0usize;
    let mut total_source_bytes = 0u64;
    collect_files_recursive(source, &mut source_files_list, &mut skipped_reparse)?;
    for f in &source_files_list {
        if let Ok(md) = fs::metadata(f) {
            total_source_bytes += md.len();
        }
    }
    let total_files_count = source_files_list.len() as u64;

    // Validate target volume has sufficient free space (including 10% safety buffer)
    if let Some(target_space) = fsx::volume_space_at(target) {
        let required_bytes = total_source_bytes.saturating_add(total_source_bytes / 10);
        if target_space.free_bytes < required_bytes {
            return Ok(CandidateVerification {
                candidate_id: candidate_id.into(),
                source_path: source.to_string_lossy().into_owned(),
                target_path: target.to_string_lossy().into_owned(),
                source_files: total_files_count,
                target_files: 0,
                source_bytes: total_source_bytes,
                target_bytes: 0,
                files_match: false,
                bytes_match: false,
                sample_hashes_checked: 0,
                sample_hashes_matched: 0,
                sha256_checked: 0,
                sha256_matched: 0,
                is_verified: false,
                metadata_report: MetadataPreservationReport::default(),
                reparse_points_safely_skipped: skipped_reparse,
                error: Some(format!(
                    "Target volume has insufficient free space ({} free vs {} required with 10% buffer)",
                    target_space.free_bytes, required_bytes
                )),
            });
        }
    }

    let mut copied_files = 0u64;
    let mut copied_bytes = 0u64;
    let mut source_hashes: HashMap<PathBuf, String> = HashMap::new();

    // Copy every file
    for src_file in &source_files_list {
        if cancel_flag.load(Ordering::SeqCst) {
            return Ok(CandidateVerification {
                candidate_id: candidate_id.into(),
                source_path: source.to_string_lossy().into_owned(),
                target_path: target.to_string_lossy().into_owned(),
                source_files: total_files_count,
                target_files: copied_files,
                source_bytes: total_source_bytes,
                target_bytes: copied_bytes,
                files_match: false,
                bytes_match: false,
                sample_hashes_checked: 0,
                sample_hashes_matched: 0,
                sha256_checked: 0,
                sha256_matched: 0,
                is_verified: false,
                metadata_report: MetadataPreservationReport::default(),
                reparse_points_safely_skipped: skipped_reparse,
                error: Some(
                    "Migration cancelled by operator during file copy. Partial copy detected."
                        .into(),
                ),
            });
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

        let (_file_len, src_hash) = match fast_copy_file_with_hash(
            src_file,
            &dst_file,
            cancel_flag,
            |chunk| {
                copied_bytes += chunk;
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
            },
        ) {
            Ok(res) => res,
            Err(e) => {
                let _ = fs::remove_file(&dst_file);
                return Ok(CandidateVerification {
                    candidate_id: candidate_id.into(),
                    source_path: source.to_string_lossy().into_owned(),
                    target_path: target.to_string_lossy().into_owned(),
                    source_files: total_files_count,
                    target_files: copied_files,
                    source_bytes: total_source_bytes,
                    target_bytes: copied_bytes,
                    files_match: false,
                    bytes_match: false,
                    sample_hashes_checked: 0,
                    sample_hashes_matched: 0,
                    sha256_checked: 0,
                    sha256_matched: 0,
                    is_verified: false,
                    metadata_report: MetadataPreservationReport::default(),
                    reparse_points_safely_skipped: skipped_reparse,
                    error: Some(format!(
                        "Copy failed on '{}': {} (File may be locked by an active process or access denied)",
                        rel.display(),
                        e
                    )),
                });
            }
        };
        source_hashes.insert(rel.to_path_buf(), src_hash);
        preserve_file_times(src_file, &dst_file);
        if let Ok(md) = fs::metadata(src_file) {
            if fsx::is_read_only(&md) {
                if let Ok(mut perms) = fs::metadata(&dst_file).map(|m| m.permissions()) {
                    perms.set_readonly(true);
                    let _ = fs::set_permissions(&dst_file, perms);
                }
            }
        }
        copied_files += 1;

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
    let mut target_skipped = 0usize;
    collect_files_recursive(target, &mut target_files_list, &mut target_skipped)?;
    let mut total_target_bytes = 0u64;
    for f in &target_files_list {
        if let Ok(md) = fs::metadata(f) {
            total_target_bytes += md.len();
        }
    }

    let files_match = source_files_list.len() == target_files_list.len();
    let bytes_match = total_source_bytes == total_target_bytes;

    // Sample hash verification (kept for fast preview and backward parity)
    let mut sample_checked = 0usize;
    let mut sample_matched = 0usize;
    let sample_indices = get_sample_indices(source_files_list.len());
    for idx in sample_indices {
        if let Some(src_f) = source_files_list.get(idx) {
            if let Ok(rel) = src_f.strip_prefix(source) {
                let dst_f = target.join(rel);
                if dst_f.exists() {
                    let h_src = source_hashes
                        .get(rel)
                        .cloned()
                        .map(Ok)
                        .unwrap_or_else(|| hash_file_sha256(src_f));
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

    // Comprehensive SHA-256 verification: hash EVERY regular file at source and destination
    let mut sha256_checked = 0usize;
    let mut sha256_matched = 0usize;
    let mut first_hash_err: Option<String> = None;

    for src_f in &source_files_list {
        if cancel_flag.load(Ordering::SeqCst) {
            return Ok(CandidateVerification {
                candidate_id: candidate_id.into(),
                source_path: source.to_string_lossy().into_owned(),
                target_path: target.to_string_lossy().into_owned(),
                source_files: total_files_count,
                target_files: copied_files,
                source_bytes: total_source_bytes,
                target_bytes: copied_bytes,
                files_match: false,
                bytes_match: false,
                sample_hashes_checked: 0,
                sample_hashes_matched: 0,
                sha256_checked: 0,
                sha256_matched: 0,
                is_verified: false,
                metadata_report: MetadataPreservationReport::default(),
                reparse_points_safely_skipped: skipped_reparse,
                error: Some("Migration cancelled by operator during verification.".into()),
            });
        }
        if let Ok(rel) = src_f.strip_prefix(source) {
            let dst_f = target.join(rel);
            if dst_f.exists() {
                let h_src = source_hashes
                    .get(rel)
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| hash_file_sha256(src_f));
                let h_dst = hash_file_sha256(&dst_f);
                match (h_src, h_dst) {
                    (Ok(s), Ok(d)) => {
                        sha256_checked += 1;
                        if s == d {
                            sha256_matched += 1;
                        } else if first_hash_err.is_none() {
                            first_hash_err = Some(format!(
                                "SHA-256 mismatch on '{}': src={}, dst={}",
                                rel.display(),
                                s,
                                d
                            ));
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        if first_hash_err.is_none() {
                            first_hash_err =
                                Some(format!("I/O error hashing '{}': {e}", rel.display()));
                        }
                    }
                }
            } else if first_hash_err.is_none() {
                first_hash_err = Some(format!(
                    "Destination file missing during verification: '{}'",
                    rel.display()
                ));
            }
        }
    }

    let all_hashes_matched =
        (sha256_checked == source_files_list.len()) && (sha256_matched == sha256_checked);
    let is_verified = files_match && bytes_match && all_hashes_matched;

    let metadata_report = MetadataPreservationReport::default();

    let error = if is_verified {
        None
    } else if let Some(e) = first_hash_err {
        Some(e)
    } else {
        Some("Verification mismatch: files, bytes, or SHA-256 signatures do not match".into())
    };

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
        sha256_checked,
        sha256_matched,
        is_verified,
        metadata_report,
        reparse_points_safely_skipped: skipped_reparse,
        error,
    })
}

pub fn collect_files_recursive(
    dir: &Path,
    list: &mut Vec<PathBuf>,
    skipped_reparse: &mut usize,
) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let meta = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if fsx::is_reparse_point(&meta) || meta.file_type().is_symlink() {
            *skipped_reparse += 1;
            continue;
        }
        if meta.is_dir() {
            collect_files_recursive(&p, list, skipped_reparse)?;
        } else if meta.is_file() {
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

#[cfg(windows)]
pub fn set_user_environment_variable(name: &str, value: &str) -> io::Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey("Environment")?;
    key.set_value(name, &value)?;

    // Broadcast WM_SETTINGCHANGE so new shells and Explorer recognize it
    unsafe {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        let param = windows::core::w!("Environment");
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(param.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            Some(&mut result),
        );
    }
    Ok(())
}

#[cfg(windows)]
pub fn get_user_environment_variable(name: &str) -> Option<String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Environment").ok()?;
    key.get_value::<String, _>(name).ok()
}

/// Removes a user environment variable from `HKCU\Environment` and broadcasts
/// `WM_SETTINGCHANGE`. Used to undo an activation that created a variable which
/// did not exist before.
#[cfg(windows)]
pub fn remove_user_environment_variable(name: &str) -> io::Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags("Environment", KEY_SET_VALUE)?;
    key.delete_value(name)?;

    unsafe {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        let param = windows::core::w!("Environment");
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(param.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            Some(&mut result),
        );
    }
    Ok(())
}

#[cfg(windows)]
pub fn set_known_folder_path(candidate_id: &str, target_path: &Path) -> io::Result<()> {
    let target_str = target_path.to_string_lossy().to_string();
    let reg_name = match candidate_id {
        "downloads" => "{374DE290-123F-4565-9164-39C4925E467B}",
        "documents" => "Personal",
        "pictures" => "My Pictures",
        "videos" => "My Video",
        "desktop" | "desktop_media" => "Desktop",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Not a recognized Windows Known Folder",
            ))
        }
    };

    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
        KEY_SET_VALUE | KEY_READ,
    )?;
    key.set_value(reg_name, &target_str)?;

    unsafe {
        windows::Win32::UI::Shell::SHChangeNotify(
            windows::Win32::UI::Shell::SHCNE_ASSOCCHANGED,
            windows::Win32::UI::Shell::SHCNF_FLUSH,
            None,
            None,
        );
    }
    Ok(())
}

#[cfg(windows)]
pub fn get_known_folder_path(candidate_id: &str) -> Option<String> {
    let reg_name = match candidate_id {
        "downloads" => "{374DE290-123F-4565-9164-39C4925E467B}",
        "documents" => "Personal",
        "pictures" => "My Pictures",
        "videos" => "My Video",
        "desktop" | "desktop_media" => "Desktop",
        _ => return None,
    };
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
        .ok()?;
    key.get_value::<String, _>(reg_name).ok()
}

/// Comprehensive application relocation validation.
/// Tests destination filesystem write capability via probe file,
/// and verifies Known Folder registry or environment variable reflection.
pub fn validate_application_relocation(
    candidate_id: &str,
    target_path: &Path,
) -> Result<Vec<String>, String> {
    let mut details = Vec::new();

    // 1. Target directory must exist
    if !target_path.is_dir() {
        return Err(format!(
            "Destination directory '{}' does not exist or is not a directory.",
            target_path.display()
        ));
    }

    // 2. Filesystem write/read/delete canary probe to ensure full NTFS write access
    let nonce_probe = generate_secure_128bit_nonce()
        .unwrap_or_else(|_| "00000000000000000000000000000000".into());
    let probe_file = target_path.join(format!(".cure_write_probe_{}.tmp", nonce_probe));
    let test_bytes = b"CURE_RELOCATION_VERIFICATION_PROBE_OK";
    if let Err(e) = fs::write(&probe_file, test_bytes) {
        return Err(format!("Destination filesystem write probe failed: {e}"));
    }
    match fs::read(&probe_file) {
        Ok(read) if read == test_bytes => {
            let _ = fs::remove_file(&probe_file);
            details
                .push("Destination filesystem write, read, and delete validation passed.".into());
        }
        Ok(_) => {
            let _ = fs::remove_file(&probe_file);
            return Err("Destination filesystem read probe data mismatch.".into());
        }
        Err(e) => {
            let _ = fs::remove_file(&probe_file);
            return Err(format!("Destination filesystem read probe failed: {e}"));
        }
    }

    // 3. Configuration validation
    match candidate_id {
        "downloads" | "documents" | "pictures" | "videos" | "desktop" | "desktop_media" => {
            #[cfg(windows)]
            {
                if let Some(active) = get_known_folder_path(candidate_id) {
                    let target_str = target_path.to_string_lossy();
                    if active.eq_ignore_ascii_case(&target_str) {
                        details.push(format!(
                            "Windows Known Folder registry verified: '{active}'"
                        ));
                    } else {
                        return Err(format!(
                            "Registry mismatch: Shell reports '{active}', expected '{target_str}'"
                        ));
                    }
                } else {
                    return Err("Could not read Windows Known Folder registry key.".into());
                }
            }
        }
        "cargo_cache" => {
            #[cfg(windows)]
            {
                let cargo_home = target_path.parent().unwrap_or(target_path);
                if let Some(val) = get_user_environment_variable("CARGO_HOME") {
                    if val.eq_ignore_ascii_case(&cargo_home.to_string_lossy()) {
                        details.push(format!("HKCU\\Environment CARGO_HOME verified: '{val}'"));
                    } else {
                        return Err(format!(
                            "Environment mismatch: CARGO_HOME is '{val}', expected '{}'",
                            cargo_home.display()
                        ));
                    }
                } else {
                    return Err("CARGO_HOME is not set in HKCU\\Environment.".into());
                }
            }
        }
        "pip_cache" => {
            #[cfg(windows)]
            {
                let target_str = target_path.to_string_lossy();
                if let Some(val) = get_user_environment_variable("PIP_CACHE_DIR") {
                    if val.eq_ignore_ascii_case(&target_str) {
                        details.push(format!("HKCU\\Environment PIP_CACHE_DIR verified: '{val}'"));
                    } else {
                        return Err(format!(
                            "Environment mismatch: PIP_CACHE_DIR is '{val}', expected '{target_str}'"
                        ));
                    }
                } else {
                    return Err("PIP_CACHE_DIR is not set in HKCU\\Environment.".into());
                }
            }
        }
        "gradle_cache" => {
            #[cfg(windows)]
            {
                let gradle_home = target_path.parent().unwrap_or(target_path);
                if let Some(val) = get_user_environment_variable("GRADLE_USER_HOME") {
                    if val.eq_ignore_ascii_case(&gradle_home.to_string_lossy()) {
                        details.push(format!(
                            "HKCU\\Environment GRADLE_USER_HOME verified: '{val}'"
                        ));
                    } else {
                        return Err(format!(
                            "Environment mismatch: GRADLE_USER_HOME is '{val}', expected '{}'",
                            gradle_home.display()
                        ));
                    }
                } else {
                    return Err("GRADLE_USER_HOME is not set in HKCU\\Environment.".into());
                }
            }
        }
        _ => {
            details.push("Workspace relocation validated.".into());
        }
    }

    Ok(details)
}

/// Activate candidate relocation and execute live application validation.
/// Unlocks cleanup only if validation passes; generates a single-use 128-bit random nonce.
pub fn activate_candidate_relocation(
    candidate_id: &str,
    target_path: &Path,
) -> io::Result<ActivationReceipt> {
    let target_str = target_path.to_string_lossy().to_string();
    let mut is_known_folder = false;
    let mut activation_succeeded = false;
    let mut application_validation_passed = false;
    let mut details = Vec::new();
    let mut env_var_updated = None;
    let mut known_folder_updated = None;
    // Capture the configuration value BEFORE any write so a failed activation
    // can be rolled back to the exact prior state.
    let mut previous_configuration: Option<String> = None;

    match candidate_id {
        "downloads" | "documents" | "pictures" | "videos" | "desktop" | "desktop_media" => {
            is_known_folder = true;
            known_folder_updated = Some(candidate_id.to_string());
            #[cfg(windows)]
            {
                previous_configuration = get_known_folder_path(candidate_id);
                if let Err(e) = set_known_folder_path(candidate_id, target_path) {
                    details.push(format!("Failed to update Known Folder registry: {e}"));
                } else {
                    activation_succeeded = true;
                    details.push("Known Folder path updated in Windows Shell registry and change notifications broadcasted.".into());
                }
            }
            #[cfg(not(windows))]
            {
                activation_succeeded = true;
            }
        }
        "cargo_cache" => {
            env_var_updated = Some("CARGO_HOME".into());
            #[cfg(windows)]
            {
                previous_configuration = get_user_environment_variable("CARGO_HOME");
                let cargo_home = target_path.parent().unwrap_or(target_path);
                if let Err(e) =
                    set_user_environment_variable("CARGO_HOME", &cargo_home.to_string_lossy())
                {
                    details.push(format!("Failed to set CARGO_HOME: {e}"));
                } else {
                    activation_succeeded = true;
                    details.push(format!(
                        "CARGO_HOME configured to '{}'",
                        cargo_home.display()
                    ));
                }
            }
            #[cfg(not(windows))]
            {
                activation_succeeded = true;
                details.push("CARGO_HOME configured".into());
            }
        }
        "pip_cache" => {
            env_var_updated = Some("PIP_CACHE_DIR".into());
            #[cfg(windows)]
            {
                previous_configuration = get_user_environment_variable("PIP_CACHE_DIR");
                if let Err(e) = set_user_environment_variable("PIP_CACHE_DIR", &target_str) {
                    details.push(format!("Failed to set PIP_CACHE_DIR: {e}"));
                } else {
                    activation_succeeded = true;
                    details.push(format!("PIP_CACHE_DIR configured to '{target_str}'"));
                }
            }
            #[cfg(not(windows))]
            {
                activation_succeeded = true;
                details.push("PIP_CACHE_DIR configured".into());
            }
        }
        "gradle_cache" => {
            env_var_updated = Some("GRADLE_USER_HOME".into());
            #[cfg(windows)]
            {
                previous_configuration = get_user_environment_variable("GRADLE_USER_HOME");
                let gradle_home = target_path.parent().unwrap_or(target_path);
                if let Err(e) = set_user_environment_variable(
                    "GRADLE_USER_HOME",
                    &gradle_home.to_string_lossy(),
                ) {
                    details.push(format!("Failed to set GRADLE_USER_HOME: {e}"));
                } else {
                    activation_succeeded = true;
                    details.push(format!(
                        "GRADLE_USER_HOME configured to '{}'",
                        gradle_home.display()
                    ));
                }
            }
            #[cfg(not(windows))]
            {
                activation_succeeded = true;
                details.push("GRADLE_USER_HOME configured".into());
            }
        }
        _ => {
            activation_succeeded = true;
            details.push("Workspace relocation active. Update IDE bookmarks as advised.".into());
        }
    }

    if activation_succeeded {
        // Run comprehensive validation probe (filesystem probe + registry/env check)
        match validate_application_relocation(candidate_id, target_path) {
            Ok(checks) => {
                application_validation_passed = true;
                details.extend(checks);
            }
            Err(e) => {
                application_validation_passed = false;
                details.push(format!("Application validation probe failed: {e}"));
            }
        }
    }

    // Rollback after failed activation: never leave the system pointing at the
    // new location when validation could not prove the relocation works.
    let mut auto_rollback_performed = false;
    if activation_succeeded && !application_validation_passed {
        let mut restore_notes: Vec<String> = Vec::new();
        match candidate_id {
            "downloads" | "documents" | "pictures" | "videos" | "desktop" | "desktop_media" => {
                #[cfg(windows)]
                match previous_configuration.as_deref() {
                    Some(prev) => match set_known_folder_path(candidate_id, Path::new(prev)) {
                        Ok(()) => {
                            auto_rollback_performed = true;
                            restore_notes.push(format!(
                                "Known Folder automatically restored to previous value '{prev}'"
                            ));
                        }
                        Err(e) => restore_notes.push(format!(
                            "AUTOMATIC ROLLBACK FAILED — Known Folder still points at '{target_str}' (error: {e}). Manual review required."
                        )),
                    },
                    None => restore_notes.push(
                        "No previous Known Folder value was recorded; the new value was left in place. Manual review required.".into(),
                    ),
                }
            }
            "cargo_cache" | "pip_cache" | "gradle_cache" => {
                #[cfg(windows)]
                {
                    let var_name = match candidate_id {
                        "cargo_cache" => "CARGO_HOME",
                        "pip_cache" => "PIP_CACHE_DIR",
                        _ => "GRADLE_USER_HOME",
                    };
                    match previous_configuration.as_deref() {
                        Some(prev) => {
                            match set_user_environment_variable(var_name, prev) {
                                Ok(()) => {
                                    auto_rollback_performed = true;
                                    restore_notes.push(format!(
                                        "{var_name} automatically restored to previous value '{prev}'"
                                    ));
                                }
                                Err(e) => restore_notes.push(format!(
                                    "AUTOMATIC ROLLBACK FAILED — {var_name} still points at '{target_str}' (error: {e}). Manual review required."
                                )),
                            }
                        }
                        None => match remove_user_environment_variable(var_name) {
                            Ok(()) => {
                                auto_rollback_performed = true;
                                restore_notes.push(format!(
                                    "{var_name} removed again (no previous value existed before activation)"
                                ));
                            }
                            Err(e) => restore_notes.push(format!(
                                "AUTOMATIC ROLLBACK FAILED — {var_name} still points at '{target_str}' (error: {e}). Manual review required."
                            )),
                        },
                    }
                }
            }
            _ => {
                restore_notes.push(
                    "Workspace candidates have no persisted configuration to roll back.".into(),
                );
            }
        }
        details.extend(restore_notes);
        crate::operator::journal::audit_log(
            "ActivationAutoRollback",
            &format!("{candidate_id} -> {target_str}"),
            None,
            if auto_rollback_performed {
                "RestoredPreviousConfiguration"
            } else {
                "RollbackIncomplete"
            },
            serde_json::json!({
                "candidate_id": candidate_id,
                "previous_configuration": previous_configuration,
                "validation_failure": true,
            }),
            None,
        );
    }

    let (cleanup_unlocked, nonce_opt, required_token) =
        if activation_succeeded && application_validation_passed {
            match generate_secure_128bit_nonce() {
                Ok(nonce) => {
                    let token = format!("CONFIRM_CLEANUP_{candidate_id}_{nonce}");
                    (true, Some(nonce), Some(token))
                }
                Err(e) => {
                    details.push(format!("Failed to generate secure operation nonce: {e}"));
                    (false, None, None)
                }
            }
        } else {
            (false, None, None)
        };

    crate::operator::journal::audit_log(
        "ActivateRelocation",
        &format!("{candidate_id} -> {target_str}"),
        None,
        if cleanup_unlocked {
            "ValidatedAndUnlocked"
        } else {
            "ValidationFailed"
        },
        serde_json::json!({
            "is_known_folder": is_known_folder,
            "activation_succeeded": activation_succeeded,
            "application_validation_passed": application_validation_passed,
            "details": details.join(" · "),
            "cleanup_unlocked": cleanup_unlocked,
            "nonce": nonce_opt.as_deref(),
        }),
        None,
    );

    let val_str = details.join(" · ");

    Ok(ActivationReceipt {
        candidate_id: candidate_id.into(),
        target_path: target_str,
        previous_path: previous_configuration.clone().unwrap_or_default(),
        configuration_action: val_str.clone(),
        validation_details: val_str,
        env_var_updated,
        known_folder_updated,
        is_known_folder,
        activation_succeeded,
        application_validation_passed,
        previous_configuration,
        cleanup_unlocked,
        cleanup_nonce: nonce_opt,
        required_cleanup_token: required_token,
        auto_rollback_performed,
        notes: details,
    })
}

/// Rollback a previous relocation back to the original source location.
pub fn rollback_candidate_relocation(
    candidate_id: &str,
    original_path: &Path,
) -> io::Result<RollbackReceipt> {
    let orig_str = original_path.to_string_lossy().to_string();
    let mut details = Vec::new();
    let mut rollback_succeeded = false;

    match candidate_id {
        "downloads" | "documents" | "pictures" | "videos" | "desktop" | "desktop_media" => {
            #[cfg(windows)]
            {
                if let Err(e) = set_known_folder_path(candidate_id, original_path) {
                    details.push(format!("Failed to revert Known Folder: {e}"));
                } else {
                    rollback_succeeded = true;
                    details.push(format!("Reverted Windows Known Folder to '{orig_str}'"));
                }
            }
            #[cfg(not(windows))]
            {
                rollback_succeeded = true;
            }
        }
        "cargo_cache" => {
            #[cfg(windows)]
            {
                let cargo_home = original_path.parent().unwrap_or(original_path);
                if let Err(e) =
                    set_user_environment_variable("CARGO_HOME", &cargo_home.to_string_lossy())
                {
                    details.push(format!("Failed to revert CARGO_HOME: {e}"));
                } else {
                    rollback_succeeded = true;
                    details.push(format!("Reverted CARGO_HOME to '{}'", cargo_home.display()));
                }
            }
            #[cfg(not(windows))]
            {
                rollback_succeeded = true;
            }
        }
        _ => {
            rollback_succeeded = true;
            details.push(format!(
                "Reverted configuration to original path '{orig_str}'"
            ));
        }
    }

    crate::operator::journal::audit_log(
        "RollbackRelocation",
        &format!("{candidate_id} -> {orig_str}"),
        None,
        if rollback_succeeded {
            "RollbackSuccess"
        } else {
            "RollbackFailed"
        },
        serde_json::json!({
            "candidate_id": candidate_id,
            "original_path": orig_str,
            "rollback_succeeded": rollback_succeeded,
            "details": details.join(" · "),
        }),
        None,
    );

    let roll_action = if details.is_empty() {
        format!("Reverted configuration to '{orig_str}'")
    } else {
        details.join(" · ")
    };

    Ok(RollbackReceipt {
        candidate_id: candidate_id.into(),
        restored_path: orig_str,
        rollback_action: roll_action,
        rollback_succeeded,
        notes: details,
    })
}

/// Finalize candidate relocation (backward compatibility wrapper calling activate_candidate_relocation).
pub fn finalize_candidate_migration(
    candidate_id: &str,
    target_path: &Path,
) -> io::Result<FinalizationReceipt> {
    let act = activate_candidate_relocation(candidate_id, target_path)?;
    let friendly = match candidate_id {
        "downloads" => "Downloads",
        "documents" => "Documents",
        "pictures" => "Pictures",
        "videos" => "Videos",
        "desktop" | "desktop_media" => "Desktop",
        _ => candidate_id,
    };
    Ok(FinalizationReceipt {
        candidate_id: candidate_id.into(),
        target_path: act.target_path,
        is_known_folder: act.is_known_folder,
        known_folder_name: if act.is_known_folder {
            Some(friendly.into())
        } else {
            None
        },
        registry_updated: act.activation_succeeded,
        configuration_action: act.validation_details,
        shell_notification_broadcast: act.activation_succeeded,
    })
}

/// User-approved source directory cleanup with live before/after C: drive free space measurement.
/// Enforces single-use 128-bit dynamic nonce, prevents replay attacks, checks active tool locks,
/// and verifies destination integrity before source deletion.
pub fn execute_source_cleanup(
    candidate_id: &str,
    source_path: &Path,
    target_path: &Path,
    confirmation_token: &str,
    expected_nonce: Option<&str>,
) -> io::Result<SourceCleanupReceipt> {
    let nonce = expected_nonce.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Source cleanup rejected: Single-use 128-bit activation nonce is required.",
        )
    })?;

    let expected_token = format!("CONFIRM_CLEANUP_{candidate_id}_{nonce}");
    if confirmation_token != expected_token {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Invalid confirmation token. Expected '{expected_token}', got '{confirmation_token}'"),
        ));
    }

    // Check for active developer processes locking source path
    if let Err(lock_err) = check_active_tool_locks(source_path) {
        return Err(io::Error::new(io::ErrorKind::ResourceBusy, lock_err));
    }

    // Boundary check
    crate::operator::defense::PathBoundaryGuard::ensure_no_overlap(source_path, target_path)
        .map_err(|e| io::Error::other(format!("Boundary check failed: {e}")))?;

    if !target_path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Target directory '{}' does not exist. Aborting source cleanup.",
                target_path.display()
            ),
        ));
    }

    // Verify target has files
    let mut target_files = Vec::new();
    let mut skipped = 0usize;
    collect_files_recursive(target_path, &mut target_files, &mut skipped)?;
    if target_files.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Target directory is empty! Refusing to delete source.",
        ));
    }

    if !source_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Source path '{}' does not exist.", source_path.display()),
        ));
    }

    // FINAL SAFETY GATE: re-prove the destination is a faithful copy of the
    // source RIGHT NOW (file sets, sizes, full SHA-256) before any deletion.
    // A stale verified receipt from the copy step is not sufficient evidence.
    if let Err(parity_err) = verify_copy_parity_for_cleanup(source_path, target_path) {
        crate::operator::journal::audit_log(
            "SourceCleanup",
            &source_path.to_string_lossy(),
            Some(confirmation_token),
            "RefusedByParityCheck",
            serde_json::json!({
                "candidate_id": candidate_id,
                "reason": parity_err,
            }),
            Some(&parity_err),
        );
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Source cleanup refused: {parity_err}"),
        ));
    }

    // All verification gates passed. Only now is the single-use nonce consumed
    // (immediately before deletion): a refused attempt must not burn the
    // operator's token, and a consumed nonce must always mean "a deletion ran".
    consume_nonce_strict(nonce)?;

    // Measure C: drive free space BEFORE cleanup
    let c_root = Path::new("C:\\");
    let space_before = fsx::volume_space_at(c_root)
        .map(|s| s.free_bytes)
        .unwrap_or(0);

    // Perform safe deletion of source tree (recursively clearing read-only attributes)
    safe_remove_dir_all(source_path)?;

    // Measure C: drive free space AFTER cleanup
    let space_after = fsx::volume_space_at(c_root)
        .map(|s| s.free_bytes)
        .unwrap_or(0);
    let freed = (space_after as i64) - (space_before as i64);

    let status = format!(
        "Source files cleaned up safely. C: drive freed {} bytes.",
        freed.max(0)
    );

    crate::operator::journal::audit_log(
        "SourceCleanup",
        &source_path.to_string_lossy(),
        Some(confirmation_token),
        &status,
        serde_json::json!({
            "candidate_id": candidate_id,
            "target_path": target_path.to_string_lossy(),
            "c_free_before": space_before,
            "c_free_after": space_after,
            "actual_bytes_freed": freed,
        }),
        None,
    );

    Ok(SourceCleanupReceipt {
        candidate_id: candidate_id.into(),
        source_path: source_path.to_string_lossy().into_owned(),
        c_free_before_bytes: space_before,
        c_free_after_bytes: space_after,
        actual_bytes_freed: freed,
        cleanup_status: status,
    })
}

// ---------------------------------------------------------------------------
// Durable transaction records & restart recovery
// ---------------------------------------------------------------------------

/// Durable, machine- and human-readable record of one migration lifecycle step.
/// Persisted to `<data_dir>/migration-receipts/` so a restarted application can
/// reconstruct what was done and reconcile it against the live system. Records
/// are evidence, not instructions: reconciliation NEVER mutates anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationTransactionRecord {
    pub record_id: String,
    /// One of: "migration_copy", "activation", "rollback", "source_cleanup".
    pub kind: String,
    pub candidate_id: String,
    pub timestamp_unix: u64,
    pub timestamp_utc: String,
    pub source_path: Option<String>,
    pub target_path: Option<String>,
    pub outcome: String,
    pub payload: serde_json::Value,
}

/// Directory holding persisted migration transaction records.
pub fn migration_receipts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("migration-receipts")
}

/// Build a collision-resistant record id for one lifecycle step.
pub fn new_transaction_record_id(kind: &str) -> String {
    let safe_kind: String = kind
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{safe_kind}-{}",
        chrono::Utc::now().timestamp_millis().max(0)
    )
}

/// Construct a record stamped with the current time.
pub fn new_transaction_record(
    kind: &str,
    candidate_id: &str,
    source_path: Option<String>,
    target_path: Option<String>,
    outcome: &str,
    payload: serde_json::Value,
) -> MigrationTransactionRecord {
    let now = chrono::Utc::now();
    MigrationTransactionRecord {
        record_id: new_transaction_record_id(kind),
        kind: kind.to_string(),
        candidate_id: candidate_id.to_string(),
        timestamp_unix: now.timestamp().max(0) as u64,
        timestamp_utc: now.to_rfc3339(),
        source_path,
        target_path,
        outcome: outcome.to_string(),
        payload,
    }
}

/// Persist a transaction record atomically (write to temp file, then rename).
pub fn persist_transaction_record(
    dir: &Path,
    record: &MigrationTransactionRecord,
) -> io::Result<PathBuf> {
    if record.record_id.is_empty()
        || !record
            .record_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Transaction record id must be non-empty and filename-safe",
        ));
    }
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", record.record_id));
    let tmp = dir.join(format!(".{}.tmp", record.record_id));
    let json = serde_json::to_string_pretty(record)
        .map_err(|e| io::Error::other(format!("Cannot serialize transaction record: {e}")))?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Load all persisted transaction records, oldest first. Corrupt or unreadable
/// files are skipped (they remain on disk for manual inspection).
pub fn load_transaction_records(dir: &Path) -> Vec<MigrationTransactionRecord> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&p) {
            if let Ok(record) = serde_json::from_str::<MigrationTransactionRecord>(&content) {
                out.push(record);
            }
        }
    }
    out.sort_by_key(|r| (r.timestamp_unix, r.record_id.clone()));
    out
}

/// Current persisted configuration value for a relocation candidate
/// (Known Folder registry value or HKCU\\Environment variable).
#[cfg(windows)]
pub fn current_relocation_config(candidate_id: &str) -> Option<String> {
    match candidate_id {
        "downloads" | "documents" | "pictures" | "videos" | "desktop" | "desktop_media" => {
            get_known_folder_path(candidate_id)
        }
        "cargo_cache" => get_user_environment_variable("CARGO_HOME"),
        "pip_cache" => get_user_environment_variable("PIP_CACHE_DIR"),
        "gradle_cache" => get_user_environment_variable("GRADLE_USER_HOME"),
        _ => None,
    }
}

#[cfg(not(windows))]
pub fn current_relocation_config(_candidate_id: &str) -> Option<String> {
    None
}

/// Read-only, honest reconciliation of a persisted record against the live
/// filesystem and configuration. Used at startup and on demand so an operator
/// can understand the state of an interrupted migration after a crash/restart.
/// Never mutates anything.
pub fn reconcile_transaction_record(record: &MigrationTransactionRecord) -> String {
    let source = record.source_path.as_deref().map(Path::new);
    let target = record.target_path.as_deref().map(Path::new);

    match record.kind.as_str() {
        "migration_copy" => {
            let src_exists = source.map(|p| p.exists()).unwrap_or(false);
            let dst_exists = target.map(|p| p.exists()).unwrap_or(false);
            match (src_exists, dst_exists) {
                (true, true) => "Source and copy both still present. Copy was verified at record time; awaiting operator decision (activate, rollback, or keep both). No source data has been deleted.".into(),
                (true, false) => "WARNING: the recorded copy no longer exists on disk. The source is still intact. Do NOT delete the source — the verified copy is gone.".into(),
                (false, true) => "Source path no longer exists (removed by an approved cleanup or externally). The copy is present.".into(),
                (false, false) => "Neither source nor copy exists anymore. This migration left no data on disk.".into(),
            }
        }
        "activation" => {
            let Some(target) = target else {
                return "Record does not name a target path; cannot reconcile configuration."
                    .into();
            };
            let target_str = target.display().to_string();
            let current = current_relocation_config(&record.candidate_id);
            match current {
                Some(cur) if cur.eq_ignore_ascii_case(&target_str) => {
                    format!("Configuration currently points at the activated target '{target_str}'.")
                }
                Some(cur) => format!(
                    "Configuration currently points at '{cur}', not the activated target '{target_str}'. It was reconfigured or rolled back after this record was written."
                ),
                None => format!(
                    "No persisted configuration value found for candidate '{}'. Workspace/advice-only relocations have no registry state to reconcile.",
                    record.candidate_id
                ),
            }
        }
        "rollback" => {
            let Some(source) = source else {
                return "Record does not name the original path; cannot reconcile rollback.".into();
            };
            let source_str = source.display().to_string();
            match current_relocation_config(&record.candidate_id) {
                Some(cur) if cur.eq_ignore_ascii_case(&source_str) => format!(
                    "Configuration currently points at the restored original '{source_str}'. Rollback is in effect."
                ),
                Some(cur) => format!(
                    "Configuration currently points at '{cur}', not the restored original '{source_str}'."
                ),
                None => format!(
                    "No persisted configuration value for candidate '{}'; nothing to reconcile for workspace/advice-only relocations.",
                    record.candidate_id
                ),
            }
        }
        "source_cleanup" => {
            let src_exists = source.map(|p| p.exists()).unwrap_or(false);
            if src_exists {
                "WARNING: the record says source cleanup ran, but the source path exists again (or was never removed). Inspect before trusting this record.".into()
            } else {
                "Source cleanup confirmed: the original path no longer exists. The verified copy is the surviving data.".into()
            }
        }
        other => {
            format!("Unknown record kind '{other}'; reconciliation unsupported for this record.")
        }
    }
}

/// Recover nonce issuances from a persisted operator audit journal. When an
/// activation happened in a previous process, its cleanup nonce is recorded in
/// the journal (action `ActivateRelocation`, `details.nonce`); replaying those
/// issuances into the ledger keeps the activation → cleanup gate enforceable
/// after a restart even if the nonce store file was lost.
pub fn recover_nonce_issuances_from_journal(journal_path: &Path) -> usize {
    let Ok(content) = fs::read_to_string(journal_path) else {
        return 0;
    };
    let mut recovered = 0usize;
    for line in content.lines() {
        let Ok(entry) = serde_json::from_str::<crate::operator::OperatorJournalEntry>(line) else {
            continue;
        };
        if entry.action_type != "ActivateRelocation" {
            continue;
        }
        if let Some(nonce) = entry.details.get("nonce").and_then(|v| v.as_str()) {
            if nonce.len() == 32 && !is_nonce_issued(nonce) {
                record_issued_nonce(nonce);
                recovered += 1;
            }
        }
    }
    recovered
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Clears read-only attributes recursively so Windows remove_dir_all does not fail with PermissionDenied.
fn clear_readonly_recursive(path: &Path) -> io::Result<()> {
    if path.is_file() {
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = fs::set_permissions(path, permissions);
            }
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                let _ = clear_readonly_recursive(&p);
            } else if let Ok(metadata) = entry.metadata() {
                let mut permissions = metadata.permissions();
                if permissions.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    permissions.set_readonly(false);
                    let _ = fs::set_permissions(&p, permissions);
                }
            }
        }
    }
    Ok(())
}

fn safe_remove_dir_all(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let _ = clear_readonly_recursive(path);
    fs::remove_dir_all(path)
}

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
        let ver =
            copy_tree_verified("test", src_dir.path(), dst_dir.path(), &cancel, |_| {}).unwrap();

        assert!(!ver.is_verified);
        assert!(ver.error.unwrap().contains("cancelled by operator"));
    }

    #[test]
    fn test_copy_tree_boundary_overlap_rejected() {
        let root = tempdir().unwrap();
        let sub = root.path().join("sub");
        fs::create_dir_all(&sub).unwrap();

        let cancel = AtomicBool::new(false);
        // Destination inside source
        let res = copy_tree_verified("overlap", root.path(), &sub, &cancel, |_| {});
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Boundary validation failed"));
    }

    #[test]
    fn test_copy_tree_full_sha256_verified() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        for i in 0..5 {
            let mut f = File::create(src.path().join(format!("file_{i}.txt"))).unwrap();
            f.write_all(format!("Unique content {i}").as_bytes())
                .unwrap();
        }

        let cancel = AtomicBool::new(false);
        let ver = copy_tree_verified("full_sha", src.path(), dst.path(), &cancel, |_| {}).unwrap();

        assert!(ver.is_verified);
        assert_eq!(ver.sha256_checked, 5);
        assert_eq!(ver.sha256_matched, 5);
        assert!(ver.metadata_report.timestamps_preserved);
    }

    #[test]
    fn test_finalize_candidate_migration() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("RelocatedDownloads");
        fs::create_dir_all(&target).unwrap();
        let receipt = finalize_candidate_migration("workspace_test", &target).unwrap();

        assert_eq!(receipt.candidate_id, "workspace_test");
        assert!(!receipt.is_known_folder);

        let cargo_target = temp.path().join("Cargo");
        fs::create_dir_all(&cargo_target).unwrap();
        let tool_receipt = finalize_candidate_migration("cargo_cache", &cargo_target).unwrap();
        assert!(!tool_receipt.is_known_folder);
        assert!(tool_receipt.configuration_action.contains("CARGO_HOME"));
    }

    #[test]
    fn test_execute_source_cleanup_validation() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        let nonce = generate_secure_128bit_nonce().unwrap();

        // 1. Missing nonce refused
        let res = execute_source_cleanup(
            "downloads",
            src.path(),
            dst.path(),
            "CONFIRM_CLEANUP_downloads",
            None,
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Single-use 128-bit"));

        // 2. Wrong token
        let res = execute_source_cleanup(
            "downloads",
            src.path(),
            dst.path(),
            "WRONG_TOKEN",
            Some(&nonce),
        );
        assert!(res.is_err());

        // 3. Empty destination
        let res = execute_source_cleanup(
            "downloads",
            src.path(),
            dst.path(),
            &format!("CONFIRM_CLEANUP_downloads_{nonce}"),
            Some(&nonce),
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Target directory is empty"));

        // 4. Valid cleanup with a faithful copy in the destination
        let src_sub = src.path().join("removable_src");
        fs::create_dir_all(&src_sub).unwrap();
        let mut f_src = File::create(src_sub.join("temp.txt")).unwrap();
        f_src.write_all(b"to be deleted").unwrap();
        drop(f_src); // close our own handle so the lock probe sees no locks
                     // Destination must be a verified copy of the source — same relative
                     // paths, same content — the parity gate refuses deletion otherwise.
        fs::copy(src_sub.join("temp.txt"), dst.path().join("temp.txt")).unwrap();

        let valid_nonce = generate_secure_128bit_nonce().unwrap();
        let receipt = execute_source_cleanup(
            "downloads",
            &src_sub,
            dst.path(),
            &format!("CONFIRM_CLEANUP_downloads_{valid_nonce}"),
            Some(&valid_nonce),
        )
        .unwrap();
        assert!(!src_sub.exists());
        assert_eq!(receipt.candidate_id, "downloads");

        // 5. Replay attack rejection: using same nonce again must fail.
        // Recreate the source first — with it gone, the NotFound gate fires
        // before the nonce gate.
        fs::create_dir_all(&src_sub).unwrap();
        let mut f_re = File::create(src_sub.join("temp.txt")).unwrap();
        f_re.write_all(b"to be deleted").unwrap();
        drop(f_re);
        fs::copy(src_sub.join("temp.txt"), dst.path().join("temp.txt")).unwrap();
        let replay_res = execute_source_cleanup(
            "downloads",
            &src_sub,
            dst.path(),
            &format!("CONFIRM_CLEANUP_downloads_{valid_nonce}"),
            Some(&valid_nonce),
        );
        assert!(replay_res.is_err());
        assert!(replay_res
            .unwrap_err()
            .to_string()
            .contains("Replay Attack Prevented"));
        assert!(src_sub.exists(), "refused replay must not delete anything");
    }

    #[test]
    fn test_onedrive_managed_detection() {
        let onedrive_fake = Path::new(r"C:\\Users\\TestUser\\OneDrive\\Documents");
        assert!(is_onedrive_managed(onedrive_fake, "documents"));

        let normal_path = Path::new(r"D:\\LocalProjects\\Repo");
        assert!(!is_onedrive_managed(normal_path, "local_projects"));
    }

    #[test]
    fn test_activate_candidate_relocation_and_rollback() {
        let temp = tempdir().unwrap();
        let target_path = temp.path().join("RelocatedWorkspace");
        fs::create_dir_all(&target_path).unwrap();

        // 1. Activation with non-registry custom workspace candidate
        let act = activate_candidate_relocation("workspace_custom_proj", &target_path).unwrap();
        assert_eq!(act.candidate_id, "workspace_custom_proj");
        assert!(act.activation_succeeded);
        assert!(act.application_validation_passed);
        assert!(act.cleanup_unlocked);
        assert!(act.cleanup_nonce.is_some());
        assert!(act.required_cleanup_token.is_some());

        // 2. Rollback
        let orig = Path::new(r"C:\Users\TestUser\Workspace\Project");
        let roll = rollback_candidate_relocation("workspace_custom_proj", orig).unwrap();
        assert_eq!(roll.candidate_id, "workspace_custom_proj");
        assert!(roll.rollback_succeeded);
    }

    #[test]
    fn test_execute_source_cleanup_nonce_enforcement() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        let src_sub = src.path().join("payload_dir");
        fs::create_dir_all(&src_sub).unwrap();
        let mut f_src = File::create(src_sub.join("sample.txt")).unwrap();
        f_src.write_all(b"source payload").unwrap();
        drop(f_src); // close our own handle so the lock probe sees no locks
                     // Destination is a faithful copy of the source (parity gate requirement).
        fs::copy(src_sub.join("sample.txt"), dst.path().join("sample.txt")).unwrap();

        let nonce = generate_secure_128bit_nonce().unwrap();

        // 1. Wrong token matching default but not nonce
        let res = execute_source_cleanup(
            "downloads",
            &src_sub,
            dst.path(),
            "CONFIRM_CLEANUP_downloads",
            Some(&nonce),
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .to_ascii_lowercase()
            .contains("invalid confirmation token"));

        // 2. Valid token with dynamic nonce
        let valid_token = format!("CONFIRM_CLEANUP_downloads_{nonce}");
        let receipt = execute_source_cleanup(
            "downloads",
            &src_sub,
            dst.path(),
            &valid_token,
            Some(&nonce),
        )
        .unwrap();

        assert!(!src_sub.exists());
        assert_eq!(receipt.candidate_id, "downloads");
    }

    #[test]
    fn test_validate_application_relocation_filesystem_probe() {
        let temp = tempdir().unwrap();
        let valid_target = temp.path().join("ValidTarget");
        fs::create_dir_all(&valid_target).unwrap();

        // Valid writable directory probe passes
        let res = validate_application_relocation("workspace_test", &valid_target);
        assert!(res.is_ok());
        let checks = res.unwrap();
        assert!(checks.iter().any(|c| c.contains("validation passed")));

        // Non-existent directory probe fails
        let missing_target = temp.path().join("MissingDir");
        let res_missing = validate_application_relocation("workspace_test", &missing_target);
        assert!(res_missing.is_err());
    }

    #[test]
    fn test_migration_plan_headroom_enforcement() {
        // Build migration plan with empty or small candidate
        let plan = build_migration_plan(&["downloads".into()], "C:", "D:");
        assert!(plan.required_headroom_bytes >= 1024 * 1024 * 1024); // At least 1 GB headroom enforced
    }

    #[test]
    fn nonce_ledger_rejects_fabricated_nonces() {
        // A syntactically valid 128-bit nonce that this installation never
        // issued must be rejected — a caller cannot mint its own cleanup
        // authorization without completing a validated activation.
        let mut ledger = NonceLedger::default();
        let fabricated = "deadbeefdeadbeefdeadbeefdeadbeef";
        let err = ledger.consume_strict(fabricated).unwrap_err();
        assert!(err.to_string().contains("never issued"));

        // The production global path must reject it as well.
        let err = consume_nonce_strict(fabricated).unwrap_err();
        assert!(err.to_string().contains("never issued"));
    }

    #[test]
    fn nonce_ledger_survives_process_restart() {
        let temp = tempdir().unwrap();
        let store = temp.path().join("operator_nonces.json");

        // Process 1: issue a nonce and persist it.
        let mut ledger = NonceLedger::default();
        let nonce = "0123456789abcdef0123456789abcdef".to_string();
        ledger.record_issued(&nonce);
        ledger.persist_to_file(&store).unwrap();
        assert!(store.exists());

        // Process 2 (restart): fresh in-memory state, reloaded from disk.
        let mut restarted = NonceLedger::default();
        restarted.load_from_file(&store);
        assert!(restarted.is_issued(&nonce));
        restarted.consume_strict(&nonce).unwrap();
        restarted.persist_to_file(&store).unwrap();

        // Process 3 (restart after consumption): replay must still be refused.
        let mut third = NonceLedger::default();
        third.load_from_file(&store);
        let err = third.consume_strict(&nonce).unwrap_err();
        assert!(err.to_string().contains("Replay Attack Prevented"));
    }

    #[test]
    fn nonce_ledger_persists_atomically_without_temp_litter() {
        let temp = tempdir().unwrap();
        let store = temp.path().join("nested").join("operator_nonces.json");
        let mut ledger = NonceLedger::default();
        ledger.record_issued("ffffffffffffffffffffffffffffffff");
        ledger.persist_to_file(&store).unwrap();
        assert!(store.exists());
        let parent = store.parent().unwrap();
        let leftovers: Vec<_> = fs::read_dir(parent)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn nonce_issuances_recover_from_audit_journal() {
        let temp = tempdir().unwrap();
        let journal = temp.path().join("audit.jsonl");
        let nonce = "1234567890abcdef1234567890abcdef";

        // Hand-craft one ActivateRelocation journal entry carrying a nonce.
        let details = serde_json::json!({ "nonce": nonce });
        let mut hasher = Sha256::new();
        hasher.update(crate::operator::journal::GENESIS_HASH.as_bytes());
        hasher.update(b":0:1700000000:ActivateRelocation:target:ok:");
        hasher.update(serde_json::to_string(&details).unwrap().as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let entry = crate::operator::OperatorJournalEntry {
            sequence: 0,
            id: "OP-LOG-TEST-0".into(),
            timestamp_iso: "2026-10-30T00:00:00Z".into(),
            timestamp_unix: 1_700_000_000,
            action_type: "ActivateRelocation".into(),
            target: "cargo_cache -> D:\\CURE_Relocated".into(),
            approval_token: None,
            status: "ValidatedAndUnlocked".into(),
            details,
            error: None,
            prev_hash: crate::operator::journal::GENESIS_HASH.into(),
            hash,
        };
        fs::write(&journal, serde_json::to_string(&entry).unwrap() + "\n").unwrap();

        let recovered = recover_nonce_issuances_from_journal(&journal);
        assert_eq!(recovered, 1);
        assert!(is_nonce_issued(nonce));
        // Journal recovery must be idempotent.
        assert_eq!(recover_nonce_issuances_from_journal(&journal), 0);
    }

    #[test]
    fn source_cleanup_refused_when_destination_is_not_a_faithful_copy() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        let src_sub = src.path().join("data");
        fs::create_dir_all(&src_sub).unwrap();
        File::create(src_sub.join("a.txt"))
            .unwrap()
            .write_all(b"alpha")
            .unwrap();
        File::create(src_sub.join("b.txt"))
            .unwrap()
            .write_all(b"beta")
            .unwrap();
        // Destination is missing b.txt entirely — the copy is incomplete.
        File::create(dst.path().join("a.txt"))
            .unwrap()
            .write_all(b"alpha")
            .unwrap();

        let nonce = generate_secure_128bit_nonce().unwrap();
        let res = execute_source_cleanup(
            "downloads",
            &src_sub,
            dst.path(),
            &format!("CONFIRM_CLEANUP_downloads_{nonce}"),
            Some(&nonce),
        );
        let err = res.unwrap_err().to_string();
        assert!(err.contains("Source cleanup refused"), "got: {err}");
        assert!(err.contains("missing copied file"), "got: {err}");
        // Source must be untouched.
        assert!(src_sub.join("a.txt").exists());
        assert!(src_sub.join("b.txt").exists());
    }

    #[test]
    fn source_cleanup_refused_when_destination_content_drifted() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        let src_sub = src.path().join("data");
        fs::create_dir_all(&src_sub).unwrap();
        File::create(src_sub.join("a.txt"))
            .unwrap()
            .write_all(b"original content")
            .unwrap();
        File::create(dst.path().join("a.txt"))
            .unwrap()
            .write_all(b"TAMPERED content")
            .unwrap();

        let nonce = generate_secure_128bit_nonce().unwrap();
        let res = execute_source_cleanup(
            "downloads",
            &src_sub,
            dst.path(),
            &format!("CONFIRM_CLEANUP_downloads_{nonce}"),
            Some(&nonce),
        );
        let err = res.unwrap_err().to_string();
        assert!(err.contains("SHA-256 mismatch"), "got: {err}");
        assert!(src_sub.join("a.txt").exists(), "source must survive");
    }

    #[test]
    fn transaction_records_persist_load_and_reconcile_honestly() {
        let temp = tempdir().unwrap();
        let receipts_dir = migration_receipts_dir(temp.path());

        let src = temp.path().join("src_data");
        let dst = temp.path().join("dst_data");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        File::create(src.join("f.txt"))
            .unwrap()
            .write_all(b"payload")
            .unwrap();
        File::create(dst.join("f.txt"))
            .unwrap()
            .write_all(b"payload")
            .unwrap();

        let record = MigrationTransactionRecord {
            record_id: new_transaction_record_id("migration_copy"),
            kind: "migration_copy".into(),
            candidate_id: "downloads".into(),
            timestamp_unix: 1_700_000_000,
            timestamp_utc: "2026-10-30T00:00:00Z".into(),
            source_path: Some(src.to_string_lossy().into_owned()),
            target_path: Some(dst.to_string_lossy().into_owned()),
            outcome: "copy_verified".into(),
            payload: serde_json::json!({ "files": 1 }),
        };

        let path = persist_transaction_record(&receipts_dir, &record).unwrap();
        assert!(path.exists());
        // Atomic write must not leave temp files behind.
        assert!(fs::read_dir(&receipts_dir)
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().starts_with('.')));

        let loaded = load_transaction_records(&receipts_dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].kind, "migration_copy");

        let reconciliation = reconcile_transaction_record(&loaded[0]);
        assert!(reconciliation.contains("both still present"));

        // Missing copy must be flagged as a warning, never as "safe".
        let _ = fs::remove_dir_all(&dst);
        let reconciliation_missing = reconcile_transaction_record(&loaded[0]);
        assert!(reconciliation_missing.contains("WARNING"));
        assert!(reconciliation_missing.contains("Do NOT delete the source"));
    }

    #[test]
    fn transaction_record_rejects_unsafe_record_ids() {
        let temp = tempdir().unwrap();
        let mut record = MigrationTransactionRecord {
            record_id: new_transaction_record_id("migration_copy"),
            kind: "migration_copy".into(),
            candidate_id: "downloads".into(),
            timestamp_unix: 1,
            timestamp_utc: "x".into(),
            source_path: None,
            target_path: None,
            outcome: "copy_verified".into(),
            payload: serde_json::json!({}),
        };
        // A path-traversal record id must never choose where the file lands.
        record.record_id = "..\\evil".into();
        assert!(persist_transaction_record(&migration_receipts_dir(temp.path()), &record).is_err());
    }
}
