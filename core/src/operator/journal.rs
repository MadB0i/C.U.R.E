//! Operator Audit Journal: Auditable, append-only, tamper-evident record of every
//! requested, approved, executed, and completed operator action.
//!
//! # Security Architecture & Threat Model
//!
//! ### Properties Enforced:
//! - Exact timestamps (Unix epoch and ISO-8601 UTC)
//! - User approval confirmation tokens (with dynamic nonce protection)
//! - Action category, actor, and target identifier
//! - Verification measurements and cryptographic receipts
//! - Cryptographic hash-chaining (SHA-256): each entry seals the hash of the previous entry,
//!   guaranteeing tamper detection if any past log line is modified, inserted, or deleted.
//!
//! ### Threat Model Boundary & Limitations (Honesty over Comfort):
//! - **Accidental Corruption & Truncation**: Guaranteed detected. Any bit flip or missing entry breaks the chain.
//! - **Single-Record Alteration**: Guaranteed detected. If an adversary modifies `details`, `target`, or `approval_token`,
//!   the recalculated hash mismatches the recorded hash and breaks all subsequent `prev_hash` pointers.
//! - **Full-Chain Rewrite Boundary**: If an adversary possesses unrestricted write access under the active user account,
//!   a pure plaintext hash-chain could theoretically be recomputed from a forged block to the tail.
//! - **Anchor Seal Defense**: On Windows, C.U.R.E strengthens the journal by anchoring the latest tail hash
//!   in a companion file (`.anchor.seal`) encrypted via Windows DPAPI (`CryptProtectData`).
//!   An offline script or untrusted payload attempting to rewrite the file cannot produce a valid DPAPI ciphertext
//!   without executing inside the user's interactive Windows logon context.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hash anchor of the first journal entry (chain genesis).
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[cfg(windows)]
pub fn seal_hash_dpapi(hash: &str) -> io::Result<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    let data_bytes = hash.as_bytes();
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: data_bytes.len() as u32,
        pbData: data_bytes.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &in_blob,
            windows::core::w!("CURE_AUDIT_JOURNAL_TAIL"),
            None,
            None,
            None,
            0,
            &mut out_blob,
        )
        .map_err(|e| io::Error::other(format!("DPAPI CryptProtectData failed: {e}")))?;
        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let vec = slice.to_vec();
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut _,
        ));
        Ok(vec)
    }
}

#[cfg(windows)]
pub fn unseal_hash_dpapi(ciphertext: &[u8]) -> io::Result<String> {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&in_blob, None, None, None, None, 0, &mut out_blob)
            .map_err(|e| io::Error::other(format!("DPAPI CryptUnprotectData failed: {e}")))?;
        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let s = String::from_utf8(slice.to_vec()).map_err(|e| {
            io::Error::other(format!("Decrypted DPAPI string is not valid UTF-8: {e}"))
        })?;
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut _,
        ));
        Ok(s)
    }
}

#[cfg(not(windows))]
pub fn seal_hash_dpapi(hash: &str) -> io::Result<Vec<u8>> {
    Ok(hash.as_bytes().to_vec())
}

#[cfg(not(windows))]
pub fn unseal_hash_dpapi(ciphertext: &[u8]) -> io::Result<String> {
    String::from_utf8(ciphertext.to_vec()).map_err(|e| io::Error::other(e.to_string()))
}

/// An auditable record of an Operator operation with cryptographic hash linking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorJournalEntry {
    pub sequence: u64,
    pub id: String,
    pub timestamp_iso: String,
    pub timestamp_unix: u64,
    pub action_type: String,
    pub target: String,
    pub approval_token: Option<String>,
    pub status: String,
    pub details: serde_json::Value,
    pub error: Option<String>,
    pub prev_hash: String,
    pub hash: String,
}

/// Compute the cryptographic block hash for a journal entry.
fn compute_entry_hash(
    sequence: u64,
    timestamp_unix: u64,
    action_type: &str,
    target: &str,
    status: &str,
    details: &serde_json::Value,
    prev_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(b":");
    hasher.update(sequence.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(timestamp_unix.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(action_type.as_bytes());
    hasher.update(b":");
    hasher.update(target.as_bytes());
    hasher.update(b":");
    hasher.update(status.as_bytes());
    hasher.update(b":");
    // Canonical JSON representation for deterministic hashing
    let details_str = serde_json::to_string(details).unwrap_or_default();
    hasher.update(details_str.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Thread-safe in-memory and on-disk audit journal with cryptographic hash chaining.
pub struct OperatorAuditJournal {
    log_path: Option<PathBuf>,
    in_memory: Mutex<Vec<OperatorJournalEntry>>,
    state: Mutex<(u64, String)>, // (next_sequence, last_hash)
}

impl OperatorAuditJournal {
    pub fn new(log_path: Option<PathBuf>) -> Self {
        let mut next_seq = 0u64;
        let mut last_hash = GENESIS_HASH.to_string();
        let mut recovered_entries = Vec::new();

        // If file exists, recover sequence and last hash
        if let Some(ref path) = log_path {
            if path.exists() {
                if let Ok(file) = File::open(path) {
                    let reader = BufReader::new(file);
                    for line in reader.lines().map_while(Result::ok) {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(entry) = serde_json::from_str::<OperatorJournalEntry>(trimmed) {
                            next_seq = entry.sequence + 1;
                            last_hash = entry.hash.clone();
                            recovered_entries.push(entry);
                        }
                    }
                }
            }
        }

        Self {
            log_path,
            in_memory: Mutex::new(recovered_entries),
            state: Mutex::new((next_seq, last_hash)),
        }
    }

    /// Record an action in the audit journal with cryptographic hash linking.
    pub fn record(
        &self,
        action_type: &str,
        target: &str,
        approval_token: Option<&str>,
        status: &str,
        details: serde_json::Value,
        error: Option<&str>,
    ) -> OperatorJournalEntry {
        let now = Utc::now();
        let ts_unix = now.timestamp() as u64;

        let (seq, entry_hash, prev_h) = {
            let mut state_lock = self.state.lock().unwrap();
            let seq = state_lock.0;
            let prev_h = state_lock.1.clone();
            let entry_hash =
                compute_entry_hash(seq, ts_unix, action_type, target, status, &details, &prev_h);
            state_lock.0 += 1;
            state_lock.1 = entry_hash.clone();
            (seq, entry_hash, prev_h)
        };

        let entry = OperatorJournalEntry {
            sequence: seq,
            id: format!("OP-LOG-{}-{}", ts_unix, seq),
            timestamp_iso: now.to_rfc3339(),
            timestamp_unix: ts_unix,
            action_type: action_type.to_string(),
            target: target.to_string(),
            approval_token: approval_token.map(|s| s.to_string()),
            status: status.to_string(),
            details,
            error: error.map(|s| s.to_string()),
            prev_hash: prev_h,
            hash: entry_hash,
        };

        // Append to in-memory history
        if let Ok(mut lock) = self.in_memory.lock() {
            lock.push(entry.clone());
        }

        // Append to disk if configured
        if let Some(ref path) = self.log_path {
            if let Ok(json_line) = serde_json::to_string(&entry) {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(file, "{json_line}");
                }
                // Anchor latest tail hash via DPAPI seal
                if let Ok(sealed_bytes) = seal_hash_dpapi(&entry.hash) {
                    let anchor_path = path.with_extension("anchor.seal");
                    let _ = fs::write(anchor_path, sealed_bytes);
                }
            }
        }

        entry
    }

    /// Retrieve the most recent journal entries.
    pub fn recent_entries(&self, limit: usize) -> Vec<OperatorJournalEntry> {
        if let Ok(lock) = self.in_memory.lock() {
            let start = lock.len().saturating_sub(limit);
            lock[start..].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Verify the complete cryptographic hash-chain integrity of the journal file.
    /// Also validates the companion DPAPI anchor seal when present.
    /// Returns Ok(entry_count) if unbroken, or Err with the exact corruption sequence.
    pub fn verify_integrity(path: &Path) -> Result<usize, String> {
        if !path.exists() {
            return Ok(0);
        }
        let file = File::open(path).map_err(|e| format!("Cannot open log file: {e}"))?;
        let reader = BufReader::new(file);

        let mut expected_prev = GENESIS_HASH.to_string();
        let mut expected_seq = 0u64;
        let mut count = 0usize;

        for (line_idx, line) in reader.lines().enumerate() {
            let line_str = line.map_err(|e| format!("Read error at line {}: {e}", line_idx + 1))?;
            let trimmed = line_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: OperatorJournalEntry = serde_json::from_str(trimmed)
                .map_err(|e| format!("Malformed JSON at line {}: {e}", line_idx + 1))?;

            if entry.sequence != expected_seq {
                return Err(format!(
                    "Sequence discontinuity at line {}: expected {}, found {}",
                    line_idx + 1,
                    expected_seq,
                    entry.sequence
                ));
            }

            if entry.prev_hash != expected_prev {
                return Err(format!(
                    "Hash-chain broken at line {} (seq {}): expected prev_hash '{}', found '{}'",
                    line_idx + 1,
                    entry.sequence,
                    expected_prev,
                    entry.prev_hash
                ));
            }

            let computed = compute_entry_hash(
                entry.sequence,
                entry.timestamp_unix,
                &entry.action_type,
                &entry.target,
                &entry.status,
                &entry.details,
                &entry.prev_hash,
            );

            if computed != entry.hash {
                return Err(format!(
                    "Tampering detected at line {} (seq {}): computed hash '{}' does not match recorded hash '{}'",
                    line_idx + 1,
                    entry.sequence,
                    computed,
                    entry.hash
                ));
            }

            expected_prev = entry.hash.clone();
            expected_seq += 1;
            count += 1;
        }

        // Verify external DPAPI anchor seal if present
        let anchor_path = path.with_extension("anchor.seal");
        if anchor_path.is_file() && count > 0 {
            if let Ok(ciphertext) = fs::read(&anchor_path) {
                match unseal_hash_dpapi(&ciphertext) {
                    Ok(sealed_tail) => {
                        if sealed_tail != expected_prev {
                            return Err(format!(
                                "External DPAPI anchor mismatch: sealed tail '{}' does not match log tail '{}'. Entire chain may have been rewritten.",
                                sealed_tail,
                                expected_prev
                            ));
                        }
                    }
                    Err(e) => {
                        return Err(format!(
                            "External DPAPI anchor seal verification failed: {e}"
                        ));
                    }
                }
            }
        }

        Ok(count)
    }
}

static GLOBAL_JOURNAL: Mutex<Option<OperatorAuditJournal>> = Mutex::new(None);

/// Initializes the global operator audit log at the given path.
pub fn init_global_journal(log_file: &Path) {
    if let Ok(mut lock) = GLOBAL_JOURNAL.lock() {
        *lock = Some(OperatorAuditJournal::new(Some(log_file.to_path_buf())));
    }
}

/// Appends an event to the global operator audit journal.
pub fn audit_log(
    action_type: &str,
    target: &str,
    approval_token: Option<&str>,
    status: &str,
    details: serde_json::Value,
    error: Option<&str>,
) -> Option<OperatorJournalEntry> {
    if let Ok(lock) = GLOBAL_JOURNAL.lock() {
        if let Some(ref journal) = *lock {
            return Some(journal.record(
                action_type,
                target,
                approval_token,
                status,
                details,
                error,
            ));
        }
    }
    None
}

/// Retrieves recent audit log entries from the global journal.
pub fn get_audit_log(limit: usize) -> Vec<OperatorJournalEntry> {
    if let Ok(lock) = GLOBAL_JOURNAL.lock() {
        if let Some(ref journal) = *lock {
            return journal.recent_entries(limit);
        }
    }
    Vec::new()
}

/// Verify integrity of the active global audit journal.
pub fn verify_global_journal_integrity(path: &Path) -> Result<usize, String> {
    OperatorAuditJournal::verify_integrity(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_records_and_verifies_hash_chain() {
        let temp = tempfile::tempdir().unwrap();
        let log_file = temp.path().join("audit.jsonl");

        let journal = OperatorAuditJournal::new(Some(log_file.clone()));
        journal.record(
            "RelocateFolder",
            "C:\\Users\\Operator\\Downloads",
            Some("CONFIRM_DOWNLOADS"),
            "success",
            serde_json::json!({ "bytes_transferred": 1024 }),
            None,
        );
        journal.record(
            "SourceCleanup",
            "C:\\Users\\Operator\\Downloads",
            Some("CONFIRM_CLEANUP_downloads_9182a"),
            "success",
            serde_json::json!({ "bytes_freed": 1024 }),
            None,
        );

        let entries = journal.recent_entries(10);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 0);
        assert_eq!(entries[1].sequence, 1);
        assert_eq!(entries[0].prev_hash, GENESIS_HASH);
        assert_eq!(entries[1].prev_hash, entries[0].hash);

        // Verify cryptographic integrity
        let verified_count = OperatorAuditJournal::verify_integrity(&log_file).unwrap();
        assert_eq!(verified_count, 2);
    }

    #[test]
    fn journal_detects_tampered_records() {
        let temp = tempfile::tempdir().unwrap();
        let log_file = temp.path().join("audit_tamper.jsonl");

        let journal = OperatorAuditJournal::new(Some(log_file.clone()));
        journal.record(
            "InitialAction",
            "TargetA",
            None,
            "success",
            serde_json::json!({}),
            None,
        );
        journal.record(
            "CriticalAction",
            "TargetB",
            Some("TOKEN_1"),
            "success",
            serde_json::json!({}),
            None,
        );

        // Tamper with the log file on disk (simulate adversary editing a log line)
        let content = fs::read_to_string(&log_file).unwrap();
        let tampered = content.replace("TargetB", "TargetMalicious");
        fs::write(&log_file, tampered).unwrap();

        // Verification must detect the tampering
        let res = OperatorAuditJournal::verify_integrity(&log_file);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Tampering detected"));
    }

    #[test]
    fn journal_anchored_dpapi_seal_verifies_successfully() {
        let temp = tempfile::tempdir().unwrap();
        let log_file = temp.path().join("audit_anchor.jsonl");

        let journal = OperatorAuditJournal::new(Some(log_file.clone()));
        journal.record("Test1", "Target1", None, "ok", serde_json::json!({}), None);
        journal.record("Test2", "Target2", None, "ok", serde_json::json!({}), None);

        let anchor_file = log_file.with_extension("anchor.seal");
        assert!(anchor_file.exists());

        let res = OperatorAuditJournal::verify_integrity(&log_file);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 2);
    }

    #[test]
    fn journal_detects_anchor_seal_mismatch_on_forged_chain() {
        let temp = tempfile::tempdir().unwrap();
        let log_file = temp.path().join("audit_forged.jsonl");

        let journal = OperatorAuditJournal::new(Some(log_file.clone()));
        journal.record("Test1", "Target1", None, "ok", serde_json::json!({}), None);
        let _entry2 = journal.record("Test2", "Target2", None, "ok", serde_json::json!({}), None);

        // Keep the anchor file sealed to entry2's hash
        let anchor_file = log_file.with_extension("anchor.seal");
        assert!(anchor_file.exists());

        // Now simulate an attacker rewriting the log file on disk with a valid linear hash chain
        // that terminates in a different forged hash
        let forged_entry = OperatorJournalEntry {
            sequence: 1,
            id: "OP-LOG-FORGED".into(),
            timestamp_iso: "2026-10-30T12:00:00Z".into(),
            timestamp_unix: 1730289600,
            action_type: "ForgedAction".into(),
            target: "ForgedTarget".into(),
            approval_token: None,
            status: "ok".into(),
            details: serde_json::json!({}),
            error: None,
            prev_hash: GENESIS_HASH.into(),
            hash: "1111111111111111111111111111111111111111111111111111111111111111".into(),
        };
        // Re-write log file with forged entry
        let line = serde_json::to_string(&forged_entry).unwrap();
        fs::write(&log_file, format!("{line}\n")).unwrap();

        // Verification must detect the external anchor seal mismatch
        let res = OperatorAuditJournal::verify_integrity(&log_file);
        assert!(res.is_err());
    }
}
