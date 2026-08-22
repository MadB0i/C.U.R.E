//! Local hash-based IOC list (Detection Engine v2).
//!
//! A small known-bad SHA-256 list is compiled INTO the binary via
//! `include_str!` so the tool stays a single portable exe that works fully
//! offline from a USB drive — no external data dependency.
//!
//! The shipped seed is synthetic demo data (see `known_bad_hashes.json`);
//! it exists to exercise the pipeline, not to catch real threats. A real
//! deployment would source hashes from a public IOC feed such as abuse.ch
//! MalwareBazaar (https://bazaar.abuse.ch/) and regenerate the JSON at build
//! time. A future version could also allow updating this list from a file on
//! the USB drive instead of only the compiled-in default.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

const KNOWN_BAD_JSON: &str = include_str!("known_bad_hashes.json");

fn known_bad() -> &'static HashMap<String, String> {
    static LIST: OnceLock<HashMap<String, String>> = OnceLock::new();
    LIST.get_or_init(|| {
        serde_json::from_str::<HashMap<String, String>>(KNOWN_BAD_JSON)
            .expect("embedded known_bad_hashes.json must be valid JSON")
            .into_iter()
            .filter(|(hash, _)| hash.len() == 64)
            .map(|(hash, desc)| (hash.to_ascii_lowercase(), desc))
            .collect()
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Looks the file's SHA-256 up in the embedded known-bad list.
/// Returns `Some(description)` on a match and `None` otherwise (including
/// when the file cannot be read — absence of evidence is not evidence).
pub fn check_hash(exe_path: &Path) -> Option<String> {
    let bytes = std::fs::read(exe_path).ok()?;
    let hex = sha256_hex(&bytes);
    known_bad().get(&hex).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_list_is_loaded_and_documented_as_demo_data() {
        let list = known_bad();
        assert!(
            list.len() >= 2,
            "expected the seeded demo fixtures in the known-bad list"
        );
        assert!(
            KNOWN_BAD_JSON.contains("TEST/DEMO fixtures"),
            "seed list must stay clearly labelled as test/demo data"
        );
    }

    #[test]
    fn fixture_bytes_match_the_embedded_list() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("fixture.exe");
        std::fs::write(&exe, b"CURE-TEST-MALWARE-SIGNATURE-DO-NOT-USE").unwrap();
        let hit = check_hash(&exe).expect("fixture bytes must match seed entry #1");
        assert!(hit.contains("CURE-TEST-MALWARE-SIGNATURE-DO-NOT-USE"));
    }

    #[test]
    fn ordinary_content_never_matches() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("benign.exe");
        std::fs::write(&exe, b"MZ ... perfectly normal unsigned tool bytes ...").unwrap();
        assert_eq!(check_hash(&exe), None);
    }

    #[test]
    fn unreadable_file_is_none_not_an_error() {
        let ghost = std::env::temp_dir().join("cure-no-such-hash-target-evert.bin");
        let _ = std::fs::remove_file(&ghost);
        assert_eq!(check_hash(&ghost), None);
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        // sha256("abc") per NIST FIPS 180-4 test vector
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
