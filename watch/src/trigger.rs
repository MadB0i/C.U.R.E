use std::fs;
use std::path::Path;

pub const TRIGGER_FILE_NAME: &str = ".cure-trigger";
pub const TRIGGER_CONTENT: &str = "CURE-TRIGGER-V1";

/// NOTE ON SECURITY (MVP-level check):
/// This only stops a *random* USB stick from launching the rescue GUI. It is
/// NOT cryptographically secure: anyone who inspects one rescue USB can copy
/// the `.cure-trigger` file onto their own drive. Hardening path: sign the
/// drive identity with Ed25519 and verify against a key pinned in this binary.
pub fn has_valid_trigger(drive_root: &Path) -> bool {
    let Ok(content) = fs::read_to_string(drive_root.join(TRIGGER_FILE_NAME)) else {
        return false;
    };
    content.trim_end() == TRIGGER_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accepts_exact_trigger_content() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(TRIGGER_FILE_NAME), b"CURE-TRIGGER-V1").unwrap();
        assert!(has_valid_trigger(dir.path()));
    }

    #[test]
    fn tolerates_trailing_whitespace() {
        let dir = tempdir().unwrap();
        for content in ["CURE-TRIGGER-V1\n", "CURE-TRIGGER-V1\r\n", "CURE-TRIGGER-V1   "] {
            fs::write(dir.path().join(TRIGGER_FILE_NAME), content).unwrap();
            assert!(has_valid_trigger(dir.path()), "rejected: {content:?}");
        }
    }

    #[test]
    fn rejects_missing_file() {
        let dir = tempdir().unwrap();
        assert!(!has_valid_trigger(dir.path()));
        assert!(!has_valid_trigger(Path::new("Z:/definitely/not/a/drive")));
    }

    #[test]
    fn rejects_wrong_or_empty_content() {
        let dir = tempdir().unwrap();
        for content in ["", "cure-trigger-v1", "CURE-TRIGGER-V2", "not a trigger"] {
            fs::write(dir.path().join(TRIGGER_FILE_NAME), content).unwrap();
            assert!(!has_valid_trigger(dir.path()), "accepted: {content:?}");
        }
    }

    #[test]
    fn rejects_leading_whitespace() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(TRIGGER_FILE_NAME), b"\n CURE-TRIGGER-V1").unwrap();
        assert!(!has_valid_trigger(dir.path()));
    }
}
