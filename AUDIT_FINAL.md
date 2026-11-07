# C.U.R.E Independent Security Audit & Codebase Verification (Final)

**Date**: 2026-10-28  
**Target Commit / Branch**: `cda1ef3` on branch `chore/final-cleanup` (based on `c82b74f`)  
**Scope**: Full repository audit across all workspace crates (`core`, `cli`, `watch`, `winwatch`), Tauri desktop crate (`gui/src-tauri`), and frontend distribution bundles (`gui/dist`, `gui/dist-demo`).  
**Audit Posture**: Adversarial, zero-trust security assessment for an offline Windows incident investigation and storage remediation workstation.

---

## 1. Phase 1 Report: Dead Code & Dependency Cleanup

### 1.1 Baseline Test Suite Execution
Prior to any modifications, the complete test suite was executed to establish an immutable baseline:

```
running unittests src\main.rs (target\debug\deps\cure-6fcdf323555a8e0e.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running unittests src\lib.rs (target\debug\deps\cure_core-db0eb59062969445.exe)
test result: ok. 422 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.77s

running unittests src\lib.rs (target\debug\deps\cure_dirwatch-602654e60f26a7e7.exe)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.04s

running unittests src\main.rs (target\debug\deps\cure_watch-9b4f2907438d9a3e.exe)
test result: ok. 58 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

Doc-tests: 0

gui\src-tauri:
test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s
(1 test ignored: overlay_fixture_tests::overlay_fixture_dismisses_fake_overlay_and_spares_notepad - requires interactive desktop session)

TOTAL BASELINE PASSING: 502 passed, 1 ignored, 0 failed.
```

### 1.2 What Was Removed
Cleanups were organized into discrete atomic commits per category:

1. **Category `deps`** (Commit `9a3254f`):
   - `core/Cargo.toml`: Removed redundant `quick-xml = "0.41"` duplicate declaration under `[target.'cfg(windows)'.dependencies]` (it was already specified under `[dependencies]` as `quick-xml = "0.41.0"`).
   - `gui/devtools/package.json` & `gui/devtools/package-lock.json`: Removed unused `"prettier": "^3.9.9"` devDependency (unreferenced across all dev scripts and CI; verified with `depcheck` and codebase search).

2. **Category `frontend`** (Commit `cda1ef3`):
   - `gui/dist/style.css` & `gui/dist-demo/style.css`: Removed 305 lines of obsolete "Disk cleanup" styles (lines 1908 to 2205) and dead media query references at lines 2319-2324 (`.cleanup-cols`, `.cleanup-summary`) and line 2471 (`.cleanup-grid,`). These styles were legacy artifacts from an earlier disk cleanup UI superseded by the Storage Workstation (`storage-workstation.js` / `storage-workstation.css`).
   - Byte-identity between `gui/dist/style.css` and `gui/dist-demo/style.css` was strictly maintained:
     - `gui/dist/style.css` SHA-256: `2D23FB9E0AF6657428E464857F99E56FA845EC4EF2B40964D97144F58ACBAF71`
     - `gui/dist-demo/style.css` SHA-256: `2D23FB9E0AF6657428E464857F99E56FA845EC4EF2B40964D97144F58ACBAF71`

### 1.3 What Was Kept and Why
- **`watch/src/main.rs:1-16`**: `#[cfg_attr(not(target_os = "windows"), allow(dead_code))]` on modules (`canary`, `consent`, `detector`, `drives`, `logger`, `pairing`, `self_update`, `uninstall`).  
  *Reason*: Required for cross-platform compilation checks and non-Windows CI stubs where the Windows main loop is omitted.
- **`gui/src-tauri/src/main.rs:208`**: `#[allow(unused_mut)] let mut entries: Vec<PersistenceEntry> = Vec::new();`  
  *Reason*: Gated for non-Windows builds where `scanners::registry` is conditionally compiled out.
- **`core/src/scanners/services.rs:347`**: `#[allow(clippy::cast_ptr_alignment)]`  
  *Reason*: Windows API service configuration buffer traversal (`QUERY_SERVICE_CONFIGW`).
- **`gui/dist/index.dev.html` & `gui/dist/mock-tauri.js`**:  
  *Reason*: Required by the browser verification suite (`gui/devtools/evidence-console.py`) and development preview harnesses. They are explicitly excluded from production binaries via `tauri.conf.json` (`frontendDist` allowlist) and rejected by the release gate (`tools/audit-candidate.py`).
- **`docs/LUMA-PREVIEW.html`**:  
  *Reason*: Dedicated development preview page for companion visual states. Gitignored for release candidates.
- **`docs/screenshots/01-overview.png` through `09-storage-preview.png`**:  
  *Reason*: Mandated documentation assets enforced by `tools/audit-candidate.py` checksum validation.

### 1.4 Needs Human Decision List
1. **Uncommitted Task 2A Draft Files**:  
   - Untracked files `core/src/agents.rs`, `core/src/agents/`, `testing/agents/` were present in the working tree from an interrupted task prior to this session. They were safely stashed (`git stash push -u -m "wip-task-2a"`). A human operator must decide when to pop and resume Task 2A.
2. **`tools/docs-capture/` Legacy Node Modules**:  
   - `tools/docs-capture` contains minimal `.mjs` forwarding wrappers to `gui/devtools/evidence-console.py`. The folder has an empty dependency list in `package.json`. A human maintainer may decide to consolidate it entirely into `gui/devtools`.

### 1.5 Verification of Post-Cleanup Integrity
- **Workspace Tests**: 492 passed, 0 failed (cure: 5, cure_core: 422, cure_dirwatch: 7, cure_watch: 58).
- **GUI Tests**: 10 passed, 1 ignored (desktop window test), 0 failed.
- **Combined Test Count**: Exactly 502 passed, 1 ignored (100% matches baseline count; 0 tests removed).
- **Frontend Verification Suite**: `gui/devtools/evidence-console.py verify` -> `PASS: 569 checks (verify)`.
- **Release Compilation**:
  - `cargo build --release --workspace`: Success (exited code 0 in 18.39s).
  - `cargo build --release --manifest-path gui/src-tauri/Cargo.toml`: Success (exited code 0 in 1m 53s).
- **Lints & Formats**:
  - `cargo clippy --workspace --all-targets -- -D warnings`: Clean (0 warnings).
  - `cargo clippy --manifest-path gui/src-tauri/Cargo.toml --all-targets -- -D warnings`: Clean (0 warnings).
  - `cargo fmt --check`: Clean (0 diffs).

---

## 2. Findings Table

| ID | Severity | File:Line | Evidence | Impact | Suggested Fix |
|---|---|---|---|---|---|
| **F-AUD-01** | **P2** | `core/src/quarantine.rs:322-345`, `core/src/quarantine.rs:279-290` | `let original = PathBuf::from(&entry.location); if !original.is_file()` does not use `symlink_metadata` to check for reparse points. If an attacker plants a file symlink in Startup, `move_file`'s fallback (`fs::copy`) follows the symlink into the target and then removes the symlink via `fs::remove_file`. | A persistence entry that is a symlink can cause the target file to be copied into quarantine rather than refusing or operating only on the link itself. In contrast, `core/src/cleanup.rs:417` explicitly checks `is_reparse_point` and aborts. | Add `let md = fs::symlink_metadata(&original)?; if md.file_type().is_symlink() || is_reparse_point(&md) { return Err(...); }` before moving files in quarantine, mirroring `cleanup.rs`. |
| **F-AUD-02** | **P2** | `winwatch/src/lib.rs:183` | `let info = unsafe { &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION) };` where `buffer` is `Vec<u8>`. `FILE_NOTIFY_INFORMATION` requires 4-byte (DWORD) alignment. A byte buffer from `vec![0u8; 64 * 1024]` only guarantees 1-byte alignment. | Creating a Rust reference `&T` to misaligned memory is immediate undefined behavior (UB), even if x86_64 CPU instructions tolerate unaligned DWORD reads. | Use an aligned buffer type (e.g. `Vec<u32>` or an aligned wrapper struct), or read fields using `std::ptr::read_unaligned`. |
| **F-AUD-03** | **P3** | Multiple (`process_scan.rs:227`, `winwatch/src/lib.rs:93`, `signature.rs:500`) | Approximately 40 `unsafe` blocks in Windows FFI interop lack explicit `// SAFETY:` rationale comments explaining buffer validity, lifetime guarantees, and null-termination invariants. | Code maintainability and auditability risk; does not cause active exploitation, but impairs long-term audit trail. | Add explicit `// SAFETY:` comments detailing pointer lifetimes and OS preconditions to all Windows API calls. |
| **F-AUD-04** | **P3** | Root repository (`Cargo.lock`, `deny.toml`) | No `deny.toml` configuration file is checked into the repository root. Running `cargo deny check` without a config fails spuriously on license verification because `cargo-deny`'s default policy rejects all licenses until explicitly allowed. | Automated CI supply-chain audits using `cargo-deny` produce spurious failures on legitimate MIT/Apache-2.0 dependencies. | Check in a minimal `deny.toml` allowing standard permissive open-source licenses (MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0) and enforcing advisory checks. |

---

## 3. Things Verified as OK (Adversarial Audit Results)

### 3.1 Verification of Earlier P0 Audit Findings
1. **Watcher USB Execution Prevention (Prior P0 F-01)**:  
   - *Checked*: `watch/src/main.rs:602-615` and `watch/src/pairing.rs`.
   - *Result*: **Verified Fully Fixed**. `watch` strictly launches `host_exe` located under `%LOCALAPPDATA%\CURE\cure-gui.exe`. The USB drive's path is passed solely as a data parameter (`--data-dir <drive>`). The host GUI binary is verified against `gui_sha256_hex` before execution; the watcher will never execute an executable resident on the USB drive.
2. **CLI Quarantine Interactive Confirmation (Prior P0 F-02)**:  
   - *Checked*: `cli/src/main.rs:611-672`, `1234-1246`.
   - *Result*: **Verified Fully Fixed**. The CLI requires an explicit confirmation prompt displaying the item ID, name, and location. In non-interactive or piped sessions, quarantine is rejected with an error unless `--yes` is explicitly passed. Parsing is strictly constrained to `"y"` or `"yes"`.
3. **Quarantine Ordering & Record Durability (Prior P0 F-04)**:  
   - *Checked*: `core/src/quarantine.rs:350-390`.
   - *Result*: **Verified Fully Fixed**. The quarantine sequence enforces:  
     1) `save_records` with `state: RecordState::Pending` *before* the file is touched;  
     2) atomic move via `MoveFileExW` (with rollback on failure);  
     3) update manifest to `RecordState::Committed`. Atomic file writes (`atomic_save`) use temporary files with `sync_all` and atomic rename. ACLs and timestamps are captured via `core/src/acl.rs:capture` and restored upon `undo`.
4. **Authenticode Certificate Revocation (Prior P0 F-03)**:  
   - *Checked*: `core/src/signature.rs:104-155, 617-626`.
   - *Result*: **Verified Fully Fixed**. `WTD_REVOKE_WHOLECHAIN` is set for all verifications. In default offline mode, `WTD_CACHE_ONLY_URL_RETRIEVAL` forbids network requests. Revocation returns (`CERT_E_REVOKED`, `CRYPT_E_REVOKED`) map to `SignatureStatus::Invalid` (+40 risk penalty). Offline cache misses map to `ValidRevocationUnknown` with honest disclosure.

### 3.2 Destructive Actions: Safe Storage Cleanup & Process Termination
- **Process Kill Safety (`gui/src-tauri/src/main.rs:1105-1138`)**:  
  - Tested `kill_high_risk_processes`: Before issuing `TerminateProcess`, the engine calls `pid_still_matches` against a live snapshot of `process_scan::enumerate_processes()`. If the PID was recycled by the operating system for a different executable name between scan and action, termination is refused.
- **Overlay Window Dismissal (`gui/src-tauri/src/main.rs:2670-2730`)**:  
  - The historical bulk auto-closer (`dismiss_overlays`) remains completely removed. Individual windows are presented to the operator. The default action is a graceful `PostMessageW(hwnd, WM_CLOSE)`. Process termination is available only as an explicit, separate, per-window force-close button.
- **Storage Workstation Safe Deletion (`core/src/cleanup.rs:386-444`, `core/src/storage/safedelete.rs`)**:  
  - Before any deletion, `fs::symlink_metadata` is checked. Any file symlink or directory junction (`FILE_ATTRIBUTE_REPARSE_POINT`) is refused and spared (`delete_refuses_file_symlink_and_spares_target`, `delete_refuses_dir_junction_and_spares_target`).
  - Storage deletion requires typed confirmation ("CLEANUP") and re-validates targets immediately before deletion.

### 3.3 Trust Boundaries & Malicious Payloads
- **USB Trigger Parser (`watch/src/pairing.rs:199-215`)**:  
  - Trigger files are capped at `MAX_TRIGGER_BYTES = 256`. The reader uses `file.take(257).read_to_end(&mut buf)`. Oversized files, symlinks, or infinite stream files cannot exhaust host memory.
  - Pairing token comparisons use constant-time byte comparisons (`pairing::constant_time_eq_slices`).
- **Scheduled Tasks XML Parser (`core/src/scanners/scheduled_tasks.rs`)**:  
  - XML files exceeding 4 MiB are skipped. The streaming XML parser enforces `MAX_EVENTS = 4096` and `MAX_ACTIONS = 32`, preventing XML entity expansion attacks (Billion Laughs) from exhausting memory.
- **Case File Export Validation (`gui/src-tauri/src/main.rs:3250-3300`)**:  
  - Base64 input is capped at 10 MB. Strict alphabet checking is enforced. The decoded payload must match the standard 8-byte PNG file magic header (`\x89PNG\r\n\x1a\n`). Destination paths are strictly sanitized against directory traversal.

### 3.4 Detection Logic & Evidence Model
- **Non-Causal Evidence Guarantee**:  
  - `EvidenceLevel` defines no `Default` trait; instances can only be minted via `Evidence::observed`, `Evidence::correlated`, or `Evidence::unknown`.
  - In `core/src/evidence.rs`, constructors for `Correlated` and `Unknown` pass text through `sanitize_causal_phrases()`, rewriting terms like `caused by`, `dropped by`, `created by`, `installed by` into `associated with`.
  - Attribution to AI agents or processes is never asserted unless directly observed.
- **Scoring Robustness**:  
  - Scores are clamped between 0 and 100.
  - Standard Windows LOLBins (`rundll32.exe`, `cmd.exe`, `powershell.exe`) executing benign scripts score `Safe` through verified heuristics, preventing false positives on standard administrative operations.

### 3.5 Privacy & Telemetry
- **Zero Remote Telemetry**:  
  - Repository-wide grep for `TcpStream`, `UdpSocket`, `reqwest`, `hyper`, `curl`, and `fetch` confirmed no background sockets or HTTP clients exist in the product.
  - Webview CSP in `tauri.conf.json` restricts connections to IPC:  
    `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; frame-src 'none'`
- **Case File Privacy**:  
  - Case file models exclude usernames, computer names, and raw paths. Export paths are redacted by default.

### 3.6 Supply Chain
- `cargo deny check advisories`: Passed (`advisories ok`, 0 security vulnerabilities in dependency graph).
- `cargo deny check bans`: Passed (`bans ok`, 0 banned crates).
- `npm audit` in `gui/devtools`: Passed (`found 0 vulnerabilities`).

---

## 4. Final Verdict

### Verdict: **SHIP AFTER FIXES**

### Summary of Justification:
The C.U.R.E codebase demonstrates exceptional defensive engineering, strict adherence to evidence-based security principles, and robust remediation of all previously identified P0 vulnerabilities. Error handling across production Rust code is virtually free of unexpected panics (only 2 justifiable `unwrap`/`expect` calls across the entire codebase), and destructive operations enforce strict confirmation, durable ordering, and non-destructive fallbacks.

To reach a full production-ready release state, two **P2** issues should be addressed in the next maintenance pass:
1. **F-AUD-01**: Extend the `symlink_metadata` reparse-point refusal check from `cleanup.rs` into `quarantine.rs` to ensure persistence entries that are symlinks cannot cause arbitrary target copying during quarantine.
2. **F-AUD-02**: Correct the pointer alignment in `winwatch/src/lib.rs` when casting raw byte buffers to `*const FILE_NOTIFY_INFORMATION` using `std::ptr::read_unaligned` or an aligned backing buffer.

Neither issue presents a remote vulnerability; both represent edge-case local hardening opportunities. The core security guarantees—no auto-scanning, local-only processing, verified non-destructive quarantine, and honest evidence reporting—are fully met.
