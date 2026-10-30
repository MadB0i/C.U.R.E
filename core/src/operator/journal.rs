//! Operator Audit Journal: Auditable, append-only record of every requested,
//! approved, executed, and completed operator action.
//!
//! Preserves:
//! - Exact timestamps (Unix epoch)
//! - User approval confirmation token
//! - Action category and target identifier
//! - Verification measurements and cryptographic receipts
//! - Rollback instructions and final disposition

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// An auditable record of an Operator operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorJournalEntry {
    pub id: String,
    pub timestamp_iso: String,
    pub timestamp_unix: u64,
    pub action_type: String,
    pub target: String,
    pub approval_token: Option<String>,
    pub status: String,
    pub details: serde_json::Value,
    pub error: Option<String>,
}

/// Thread-safe in-memory and on-disk audit journal.
pub struct OperatorAuditJournal {
    log_path: Option<PathBuf>,
    in_memory: Mutex<Vec<OperatorJournalEntry>>,
}

impl OperatorAuditJournal {
    pub fn new(log_path: Option<PathBuf>) -> Self {
        Self {
            log_path,
            in_memory: Mutex::new(Vec::new()),
        }
    }

    /// Record an action in the audit journal.
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
        let entry = OperatorJournalEntry {
            id: format!("OP-LOG-{}", now.timestamp_millis()),
            timestamp_iso: now.to_rfc3339(),
            timestamp_unix: now.timestamp() as u64,
            action_type: action_type.to_string(),
            target: target.to_string(),
            approval_token: approval_token.map(|s| s.to_string()),
            status: status.to_string(),
            details,
            error: error.map(|s| s.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_records_and_retrieves_entries() {
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

        let entries = journal.recent_entries(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action_type, "RelocateFolder");
        assert_eq!(entries[0].status, "success");
        assert_eq!(entries[0].approval_token, Some("CONFIRM_DOWNLOADS".into()));

        // Verify file was written
        assert!(log_file.exists());
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("RelocateFolder"));
    }
}
