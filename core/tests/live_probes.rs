//! Live, read-only environment probes used during native acceptance.
//!
//! These are `#[ignore]`d so CI never depends on the host environment. Run
//! them explicitly with `cargo test -p cure_core --test live_probes -- --
//! ignored --nocapture`. Every probe is observation-only: no process is
//! terminated, no registry value written, no remediation executed.

use cure_core::agent::session::discover_agent_session_storage;
use cure_core::agent::AgentKind;
use cure_core::operator::{find_defender_cli, scan_file_with_defender};

#[test]
#[ignore = "live host probe — run manually during acceptance"]
fn live_agent_process_and_session_discovery() {
    let processes = cure_core::agent::enumerate_agent_processes();
    println!("== live agent process enumeration ==");
    if processes.is_empty() {
        println!("(no supported agent processes running on this host)");
    }
    for p in &processes {
        println!(
            "{:<12} pid={:<7} mem={:?} identity={:?} exe={}",
            p.kind.label(),
            p.pid,
            p.memory_usage_bytes,
            p.identity,
            p.exe_path
        );
        assert!(
            !p.exe_path.to_ascii_lowercase().contains("sk-"),
            "secrets must never leak into paths"
        );
    }

    println!("== offline session store discovery (12 agents) ==");
    for kind in AgentKind::ALL {
        let info = discover_agent_session_storage(kind);
        println!(
            "{:<12} supported={:<5} state={:?} count={:<4} latest={:?} path={}",
            kind.label(),
            info.supported,
            info.state_kind,
            info.session_count,
            info.latest_session_utc,
            info.storage_path.as_deref().unwrap_or("-")
        );
    }
}

#[test]
#[ignore = "live host probe — run manually during acceptance"]
fn live_signature_detail_of_installed_agents() {
    let candidates = [
        r"C:\Users\rupjy\AppData\Local\Programs\antigravity\Antigravity.exe",
        r"C:\Users\rupjy\AppData\Local\OpenAI\Codex\bin\22cfb1d302613eae\codex.exe",
    ];
    for c in candidates {
        let p = std::path::Path::new(c);
        if !p.is_file() {
            println!("{c} — not present");
            continue;
        }
        let d = cure_core::signature::signature_detail(p);
        println!("{c}\n    status={:?} publisher={:?}", d.status, d.publisher);
    }
}

#[test]
#[ignore = "live host probe — run manually during acceptance"]
fn live_defender_audit_only_scan_of_benign_fixture() {
    let Some(mp) = find_defender_cli() else {
        println!("MpCmdRun.exe not found on this host — probe inconclusive");
        return;
    };
    println!("Defender CLI: {}", mp.display());

    // Benign local fixture: this crate's lib.rs. The audit-only scan must not
    // modify it, and the outcome must be parsed honestly.
    let fixture = std::env::current_dir().unwrap().join("src").join("lib.rs");
    let before = std::fs::read(&fixture).unwrap();
    let result = scan_file_with_defender(&fixture).expect("defender scan should run");
    let after = std::fs::read(&fixture).unwrap();
    println!(
        "outcome={:?} code={} threats={:?} status={}",
        result.outcome, result.return_code, result.detected_threats, result.status
    );
    assert_eq!(before, after, "audit-only scan must not modify the file");
    assert!(!result.threat_detected || !result.detected_threats.is_empty());
}
