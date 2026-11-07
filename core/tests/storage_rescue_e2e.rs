//! Controlled storage-rescue lifecycle E2E on a disposable D: root.
//!
//! Everything happens under `D:\CURE_V13_ACCEPTANCE\` (created and removed by
//! these tests). The owner's real user data, caches, and registry are never
//! touched. If D: is unavailable or low on space, the tests degrade to a
//! system-temp fixture so the same lifecycle is still exercised end to end.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use cure_core::storage::{
    check_active_tool_locks, execute_candidate_migration, execute_source_cleanup,
    generate_secure_128bit_nonce, reconcile_transaction_record, verify_copy_parity_for_cleanup,
    MigrationTransactionRecord,
};

/// Resolve a disposable acceptance root on D: when practical, else temp.
fn acceptance_root(tag: &str) -> PathBuf {
    let d_root = PathBuf::from(r"D:\CURE_V13_ACCEPTANCE");
    let free_ok = cure_core::storage::assess_drive_pressures()
        .into_iter()
        .find(|d| d.drive_letter == "D:")
        .map(|d| d.free_bytes > 2 * 1024 * 1024 * 1024)
        .unwrap_or(false);
    let base = if d_root.parent().is_some() && Path::new(r"D:\").is_dir() && free_ok {
        d_root
    } else {
        std::env::temp_dir().join("CURE_V13_ACCEPTANCE_FALLBACK")
    };
    let dir = base.join(tag);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_file(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = File::create(path).unwrap();
    f.write_all(content).unwrap();
}

#[test]
fn full_lifecycle_copy_verify_cleanup_on_disposable_volume() {
    let root = acceptance_root("lifecycle");
    let source = root.join("source_cache");
    let target = root.join("target_copy");

    // Disposable "developer cache"-shaped tree with nested directories.
    write_file(&source.join("registry").join("index.toml"), b"index-v2");
    write_file(
        &source.join("registry").join("cache").join("a.crate"),
        &[7u8; 4096],
    );
    write_file(&source.join("git").join("checkouts.json"), b"[]");
    write_file(&source.join("empty_dir").join(".keep"), b"");

    // 1. COPY + VERIFY (full SHA-256 of every file, source preserved).
    let cancel = Arc::new(AtomicBool::new(false));
    let receipt =
        execute_candidate_migration("e2e_cache", &source, &target, "D:", "D:", &cancel, |_| {})
            .unwrap();
    assert!(receipt.items[0].is_verified, "copy must verify");
    assert_eq!(receipt.items[0].sha256_checked, 4);
    assert_eq!(receipt.items[0].sha256_matched, 4);
    assert!(source.join("registry").join("index.toml").exists());

    // 2. Cleanup nonce + token from a simulated validated activation.
    let nonce = generate_secure_128bit_nonce().unwrap();
    let token = format!("CONFIRM_CLEANUP_e2e_cache_{nonce}");

    // 3. Tampering with the destination must REFUSE the cleanup.
    write_file(
        &target.join("registry").join("cache").join("a.crate"),
        &[9u8; 4096],
    );
    let refused = execute_source_cleanup("e2e_cache", &source, &target, &token, Some(&nonce));
    let err = refused.unwrap_err().to_string();
    assert!(err.contains("SHA-256 mismatch"), "got: {err}");
    assert!(source.join("registry").join("index.toml").exists());

    // 4. Restore the faithful copy; the same nonce must still be consumable
    //    (the refused attempt did not burn it).
    write_file(
        &target.join("registry").join("cache").join("a.crate"),
        &[7u8; 4096],
    );
    let record = MigrationTransactionRecord {
        record_id: format!("e2e-{}", cure_core::agent::now_unix()),
        kind: "migration_copy".into(),
        candidate_id: "e2e_cache".into(),
        timestamp_unix: cure_core::agent::now_unix(),
        timestamp_utc: "e2e".into(),
        source_path: Some(source.to_string_lossy().into_owned()),
        target_path: Some(target.to_string_lossy().into_owned()),
        outcome: "copy_verified".into(),
        payload: serde_json::json!({}),
    };
    assert!(reconcile_transaction_record(&record).contains("both still present"));

    // 5. Approved cleanup: parity gate passes, source is deleted, nonce burned.
    let receipt2 =
        execute_source_cleanup("e2e_cache", &source, &target, &token, Some(&nonce)).unwrap();
    assert!(!source.exists(), "approved cleanup deletes the source");
    assert!(target.join("registry").join("index.toml").exists());
    assert!(receipt2.cleanup_status.contains("freed"));

    // 6. Replay of the same nonce must be refused even in the same process.
    //    Recreate the source so the attempt reaches the consumption gate
    //    (with the source gone, the NotFound gate fires first).
    write_file(&source.join("registry").join("index.toml"), b"index-v2");
    write_file(
        &source.join("registry").join("cache").join("a.crate"),
        &[7u8; 4096],
    );
    write_file(&source.join("git").join("checkouts.json"), b"[]");
    write_file(&source.join("empty_dir").join(".keep"), b"");
    let replay = execute_source_cleanup("e2e_cache", &source, &target, &token, Some(&nonce));
    assert!(replay.unwrap_err().to_string().contains("Replay Attack"));
    assert!(source.exists(), "refused replay must not delete anything");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn locked_source_file_blocks_cleanup_and_parity_gate_holds() {
    let root = acceptance_root("locks");
    let source = root.join("src");
    let target = root.join("dst");
    write_file(&source.join("busy.bin"), b"busy");
    write_file(&target.join("busy.bin"), b"busy");

    // Hold an exclusive-ish handle open in this process: the probe must see it.
    let _held = File::open(source.join("busy.bin")).unwrap();
    // A plain read handle opened by us conflicts with a share_mode(0) probe.
    assert!(
        cure_core::storage::fsx::is_in_use(&source.join("busy.bin")),
        "open handle must be detected as in-use"
    );
    let lock_err = check_active_tool_locks(&source).unwrap_err();
    assert!(
        lock_err.contains("locked by an active process"),
        "got: {lock_err}"
    );
    drop(_held);
    assert!(check_active_tool_locks(&source).is_ok());

    // Parity gate refuses when the source has a file the destination lacks.
    write_file(&source.join("extra.bin"), b"more");
    let err = verify_copy_parity_for_cleanup(&source, &target).unwrap_err();
    assert!(err.contains("missing copied file"), "got: {err}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn junction_in_source_refuses_deletion_and_is_never_followed_by_copy() {
    let root = acceptance_root("junction");
    let source = root.join("src");
    let target = root.join("dst");
    write_file(&source.join("real.txt"), b"real");
    let outside = root.join("outside_victim");
    write_file(&outside.join("secret.txt"), b"do not touch");

    // Create a junction pointing at the outside directory.
    let junction = source.join("link_out");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output();
    if !made.map(|o| o.status.success()).unwrap_or(false) {
        // Environment refuses junction creation; verify nothing else broke.
        assert!(source.join("real.txt").exists());
        fs::remove_dir_all(&root).unwrap();
        return;
    }

    // Copy must NOT traverse the junction (the outside victim stays untouched
    // and no link content appears in the destination).
    let cancel = Arc::new(AtomicBool::new(false));
    let receipt = execute_candidate_migration(
        "e2e_junction",
        &source,
        &target,
        "D:",
        "D:",
        &cancel,
        |_| {},
    )
    .unwrap();
    assert!(receipt.items[0].is_verified);
    assert!(receipt.items[0].reparse_points_safely_skipped >= 1);
    assert!(
        !target.join("link_out").exists() || !target.join("link_out").join("secret.txt").exists()
    );
    assert!(
        outside.join("secret.txt").exists(),
        "junction target must be untouched"
    );

    // Parity gate refuses source deletion when reparse points are involved.
    let nonce = generate_secure_128bit_nonce().unwrap();
    let token = format!("CONFIRM_CLEANUP_e2e_junction_{nonce}");
    let refused = execute_source_cleanup("e2e_junction", &source, &target, &token, Some(&nonce));
    let err = refused.unwrap_err().to_string();
    assert!(
        err.contains("Refused") || err.contains("missing copied file") || err.contains("Reparse"),
        "got: {err}"
    );
    assert!(outside.join("secret.txt").exists());

    fs::remove_dir_all(&root).unwrap();
}
