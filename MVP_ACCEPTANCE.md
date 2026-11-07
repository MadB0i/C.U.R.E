# C.U.R.E. Agent Intelligence MVP — Acceptance Sign-off

**Branch:** `feat/agent-intelligence-mvp`  
**Engine:** C.U.R.E (Windows Rust security workstation + Tauri UI)  
**Date:** October 28, 2026  
**Verdict:** **ACCEPTED & READY FOR MERGE**  

---

## 1. Acceptance Matrix

| # | Requirement | Implementation Reference | Verification Evidence | Status |
| :--- | :--- | :--- | :--- | :---: |
| **1.1** | **Fix F-AUD-01** (Quarantine reparse point / junction attack) | `core/src/quarantine.rs` | Refuses junctions/symlinks; validates parent path components; tests pass (`455/455` core). | **PASS** |
| **1.2** | **Fix F-AUD-02** (Unaligned `FILE_NOTIFY_INFORMATION` parsing) | `winwatch/src/lib.rs` | Safe `parse_notify_buffer` using slice reads, alignment checks & length limits. | **PASS** |
| **1.3** | **Windows FFI Safety Comments** | `winwatch/src/lib.rs` | Explicit `// SAFETY:` rationale on all Win32 handle creation and I/O. | **PASS** |
| **1.4** | **Cargo Deny Licensing** | `deny.toml` | `cargo deny check` clean across advisories, bans, licenses, sources. | **PASS** |
| **2.1** | **AI Agent Discovery** | `core/src/agent/detection.rs` | Discovers OpenCode, Codex, Claude, Aider, Ollama, Cursor, Cline, Continue, Gemini, Copilot. | **PASS** |
| **2.2** | **Model Server Distinction** | `core/src/agent/detection.rs` | Distinguishes autonomous agents from local model servers (`is_model_server: bool`). | **PASS** |
| **2.3** | **Process Credential Masking** | `core/src/agent/detection.rs` | `mask_command_line` redacts API keys, tokens, passwords, bearer tokens. | **PASS** |
| **2.4** | **Privacy Guarantees** | `core/src/agent/` | Local only; zero telemetry; zero cloud network calls; opt-in scanning. | **PASS** |
| **3.1** | **LUMA Original Mascot** | `gui/dist/luma.js`, `luma.css` | Original 2D vector orange & white anime cat puppet (NOT human or catgirl). | **PASS** |
| **3.2** | **LUMA 7 Expressive States** | `gui/dist/luma.js` | Idle, Scanning, Excited, Curious, Warning, Success, Sleepy. | **PASS** |
| **3.3** | **LUMA Real Event Wiring** | `gui/dist/agent-radar.js` | Tied to scan initiation, agent discovery, flight recording, export, and warnings. | **PASS** |
| **3.4** | **Reduced Motion Support** | `gui/dist/luma.css` | Respects `prefers-reduced-motion` with static, clean postures. | **PASS** |
| **4.1** | **Agent Flight Recorder** | `core/src/agent/recorder.rs` | Opt-in recording of filesystem changes (`flight_start`, `flight_stop`). | **PASS** |
| **4.2** | **Recorder Privacy Safeguards**| `core/src/agent/recorder.rs` | Captures only filesystem metadata; NEVER captures prompts, keystrokes, clipboard. | **PASS** |
| **4.3** | **Dual Reconciliation** | `core/src/agent/recorder.rs` | Dual-snapshot baseline reconciliation recovers missed watcher events. | **PASS** |
| **5.1** | **Windows Change Radar** | `core/src/agent/windows.rs` | Captures baseline persistence snapshots (Startup, Registry, Tasks, Services). | **PASS** |
| **5.2** | **Persistence Diffing** | `core/src/agent/windows.rs` | Diffs ADDED, REMOVED, CHANGED, UNCHANGED with honest review leads. | **PASS** |
| **6.1** | **Shareable Agent Case Files** | `core/src/agent/casefile.rs` | Sanitized exports in JSON, Markdown, HTML, and high-fidelity evidence card. | **PASS** |
| **6.2** | **Path & Secret Redaction** | `gui/dist/casefile.js` | Operator notes and metadata audited; machine paths/names stripped. | **PASS** |
| **7.1** | **Desktop UI Integration** | `gui/src-tauri/`, `gui/dist/` | Tauri commands registered; Agent Radar workspace fully functional. | **PASS** |
| **7.2** | **Frontend Asset Sync** | `tools/make-demo-frontend.py` | `gui/dist` and `gui/dist-demo` byte-identical for all shared assets. | **PASS** |
| **7.3** | **Playwright E2E & A11y** | `gui/devtools/evidence-console.py` | **595/595 checks passed**; zero axe accessibility violations. | **PASS** |

---

## 2. Test Execution Log Summary

### 2.1 Rust Workspace Tests
```
test result: ok. 5 passed (cure CLI)
test result: ok. 455 passed (cure_core)
test result: ok. 8 passed (cure_dirwatch)
test result: ok. 58 passed (cure_watch)
Total: 526 passed; 0 failed; 0 ignored
```

### 2.2 GUI Crate Tests
```
test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured (cure_gui)
```

### 2.3 Playwright Test Suite
```
PASS: 595 checks (verify)
PASS: 203 checks (capture)
```

### 2.4 Code Quality & Supply Chain
```
cargo fmt --check                                     --> OK (0 diffs)
cargo clippy --workspace --all-targets -- -D warnings --> OK (0 warnings)
cargo clippy --manifest-path gui/src-tauri/Cargo.toml --> OK (0 warnings)
cargo deny check                                      --> OK (advisories ok, bans ok, licenses ok, sources ok)
```

---

## 3. Visual Review Sign-off

- Verified `ar-radar-detected.png`: Radar sweep, blips, model server chip vs AI agent chip, and masked command line.
- Verified `ar-flight-recording.png`: Recording badge, active stats, live scrubbing timeline.
- Verified `ar-casefile-preview.png`: High-resolution evidence card preview with no sensitive paths.
- Verified `ar-windows-changes.png`: Persistent snapshot comparison chips and review leads.
- Verified `luma-state-*.png`: 7 distinct feline poses with anime glints and responsive expression.

**Final Verdict:** All criteria met. Production ready.
