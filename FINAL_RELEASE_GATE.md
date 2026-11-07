# C.U.R.E. MVP — FINAL PRE-RELEASE HARDENING & AUDIT GATE

**Target Platform:** Windows 10 / 11 (x64)  
**Binary Verified:** `gui/src-tauri/target/release/cure-gui.exe`  
**Git Branch:** `feat/agent-intelligence-mvp`  
**Verdict:** **GO** (Ready for tagged release on main)

---

## 1. Executive Summary & Audit Mandate

This document serves as the formal pre-release acceptance audit for C.U.R.E. (Clean, Undoable, Resilient, Evidence-first) Portable Windows Security Toolkit, specifically closing out the two pre-release security and evidence-integrity requirements:

1. **AI Agent Radar Identity Accuracy**: Prevent unconfirmed or renamed executables (e.g. `opencode.exe`) from being represented as verified genuine AI tools. Enforce that an executable path alone *never* confers verified status without positive cryptographic Authenticode proof.
2. **Flight Recorder Evidence Accuracy**: Maintain strict forensic discipline between observed filesystem changes and process attribution. Ensure filesystem modifications are recorded as `Correlated` (not causally attributed to an AI agent) without handle-level tracing.

---

## 2. Hardening Implementations

### A. Process Identity & Detection Discipline (`core/src/agent/detection.rs`)
- **Resolved Path Rule:** Eliminated the legacy fallback that marked any unsigned or unverified binary as `ExecutableIdentity::Verified` merely because it resided within a canonical path (`Program Files`, `%LOCALAPPDATA%\Programs`, `.cargo\bin`).
- **Cryptographic Publisher Verification:** `ExecutableIdentity::Verified` strictly requires a valid Authenticode signature signed by the documented vendor:
  - **Ollama**: Signed by Ollama Inc.
  - **Cursor**: Signed by Anysphere Inc.
  - **Copilot**: Signed by Microsoft Corporation.
  - **Claude Code**: Signed by Anthropic PBC.
  - **Gemini CLI**: Signed by Google LLC.
  - **Codex CLI**: Signed by OpenAI.
  - **Aider**: Signed by Paul Gauthier / Aider.
  - **Continue**: Signed by Continue Dev Inc.
  - **Cline**: Signed by Cline Bot Inc.
- **Degradation Posture:**
  - Unsigned executables, scratch paths, or unverified locations degrade honestly to `ExecutableIdentity::Unverified` (Confidence: `"Unverified (Image name match; tool identity unconfirmed)"`).
  - Mismatched runtimes (e.g. Python or Node or Windows system binaries renamed to `opencode.exe`) degrade to `ExecutableIdentity::SuspectMismatch` (Confidence: `"Unverified (Metadata mismatch; likely renamed binary)"`).
  - Restricted or elevated processes return `ExecutableIdentity::AccessDenied` (Confidence: `"Unknown (Process query access denied)"`).
  - Missing or inaccessible paths return `ExecutableIdentity::PathMissing` or `ExecutableIdentity::ImageNameOnly`.

### B. UI Forensic Presentation (`gui/dist/agent-radar.js`, `gui/src-tauri/src/main.rs`)
- **Transparent Status Badges:**
  - If a process is unverified, the agent card chip displays `[Unverified Process]` in amber (instead of `[AI Agent]`).
  - If metadata contradicts the tool, the card chip displays `[Suspect Binary]` in red.
  - If access was denied, the card chip displays `[Access Denied]` in red.
  - The Inspector modal displays the exact identity status, full executable path, and explicit forensic note: *"Presence is not activity. Workspace modifications are recorded separately via Flight Recorder."*
- **Radar Readout Note:**
  - Distinctly indicates: `"{N} agent candidate(s) in {M} process(es) (unverified identity). Presence is not proof of genuine tool or activity."`

### C. Flight Recorder Attribution & Boundaries (`core/src/agent/recorder.rs`, `casefile.rs`)
- **Strict Evidence Boundaries:** The directory watcher monitors filesystem notifications only, not kernel process write handles. All filesystem modifications captured while an agent process is alive are classified as `EvidenceConfidence::Correlated` (or `Observed` if in isolation).
- **No Causal Language:** Generated reports and Case Files explicitly state:
  > *"Filesystem events were observed while a supported agent process was running in the background. The directory watcher records filesystem notifications, not process write handles. Time correlation does not establish causation or agent attribution."*
- **Offline & Zero Leakage:** Recorder buffers are bounded (max 5,000 events, 50,000 files). No file contents, prompts, or keystrokes are recorded or transmitted. Command-line secrets are masked with `***`.

---

## 3. Test Suites & Verification Results

### 1. Rust Workspace Test Suite (`cargo test --workspace`)
- **Crates:** `cure`, `cure_core`, `cure_dirwatch`, `cure_watch`
- **Tests Passed:** **532** (459 in core, 10 in dirwatch, 58 in watch, 5 in cli)
- **Failures:** 0
- **Ignored:** 0

### 2. GUI Crate Unit Tests (`cargo test --manifest-path gui/src-tauri/Cargo.toml`)
- **Tests Passed:** **10** (1 ignored: interactive desktop window dismiss test)
- **Failures:** 0

### 3. Clippy & Formatting
- `cargo fmt --check`: Clean (exit code 0)
- `cargo clippy --workspace --all-targets -- -D warnings`: Clean (0 warnings)
- `cargo clippy --manifest-path gui/src-tauri/Cargo.toml --all-targets -- -D warnings`: Clean (0 warnings)

### 4. Real Native Windows Desktop Acceptance Audit (`testing/native/native-agent-audit.py`)
Driven against production release binary `cure-gui.exe` (port 9444 via CDP):
- **Phase 1: Startup & Bridge:** Native Tauri IPC confirmed; production backend active (no mock shell).
- **Phase 2: LUMA Mascot & Themes:** Verified feline anatomy (2 ears, 6 eye glints, blush pads/ticks, 4-joint tail, 18 stripes, white paws, 3 tail rings). All 7 states (`idle`, `scanning`, `excited`, `curious`, `warning`, `success`, `sleepy`) activated and verified. Reduced motion pauses all animations. Genuinely distinct Dark and Light screenshots captured (`01-native-overview-dark.png` vs `02-native-overview-light.png`).
- **Phase 3: AI Agent Radar:**
  - Tested dummy renamed binary `opencode.exe` spawned with sensitive flags (`--api-key`, `--token`, `Authorization: Bearer`).
  - Binary classified as `Unverified` (`Unverified (Image name match; tool identity unconfirmed)`).
  - UI displayed `[Unverified Process]` in amber with state `unverified_process`.
  - Command-line arguments thoroughly masked with `***` (no secrets in JSON payload).
  - Clean process termination verified: process vanished from radar upon exit.
- **Phase 4: Flight Recorder Real Activity:**
  - Clean workspace session recorded 4 filesystem events (`created`, `modified`, `renamed`, `removed`).
  - Privacy preserved: zero file contents captured.
  - Coverage confirmed complete (`complete=True`, `dropped=0`).
- **Phase 5: Windows Change Radar:**
  - Baseline persistence snapshot saved (114 trusted entries).
  - Controlled registry test run key created and detected as `ADDED persistence: 'CureAuditTemporaryTest' in Registry Run key`.
  - Cleanly removed and confirmed non-malware lead note.
- **Phase 6: Privacy-Safe Case Files:**
  - Exported JSON, Markdown, and HTML.
  - Privacy verified: local usernames sanitized (`<user>`), zero personal paths.
  - Strict HTML entity escaping verified: special characters and scripts neutralized.
- **Phase 7: Win32 Desktop Window:** Native window captured.
- **Audit Total:** **45 / 45 Checks Passed** (0 Failures).

---

## 4. Files Changed

| File Path | Description of Changes |
| :--- | :--- |
| `core/src/agent/detection.rs` | Hardened `verify_process_identity` to require vendor Authenticode verification. Disallowed canonical path fallback for unsigned binaries. Added comprehensive regression tests. |
| `gui/src-tauri/src/main.rs` | Refined `agent_state_for` to return `unverified_process` for unconfirmed binaries. Enhanced radar summary notes to distinguish verified from unverified agents. |
| `gui/dist/agent-radar.js` | Updated agent card rendering to show `[Unverified Process]` and `[Suspect Binary]` chips. Hardened Inspector modal to show `[Access Denied]` and unverified badges. |
| `gui/dist-demo/agent-radar.js` | Synchronized byte-identically with `gui/dist/agent-radar.js`. |
| `testing/native/screenshots-audit/*` | Updated with genuine native screenshots captured from the hardened release binary. |

---

## 5. What is NOT Verified (Blunt Honesty)

1. **Kernel Handle Tracing (ETW / Sysmon):** C.U.R.E. uses user-mode `ReadDirectoryChangesW` and `CreateToolhelp32Snapshot`. It does not install a kernel filter driver (`fltmgr`) or consume ETW kernel providers. Therefore, it cannot prove *which specific PID* wrote a byte to disk during a shared time window; all timeline events remain `Correlated`, not causally attributed.
2. **Revocation Online CRL Verification:** Revocation checking defaults to `CacheOnly` (zero network calls). If Windows has not locally cached the CRL/OCSP response for a signed binary, the verdict is `ValidRevocationUnknown`. Online revocation requires explicit opt-in via CLI `--online-revocation`.
3. **Non-Windows Environments:** Agent detection and Windows Change Radar are Windows-specific (`#[cfg(windows)]`). On Linux/macOS, process enumeration returns empty, and tests use mocked descriptors.

---

## 6. Risks & Recommended Follow-ups

- **Risk: Unsigned Open-Source CLI Binaries:** Legitimate open-source CLI agents compiled from source (e.g. `opencode-cli` or `aider` built via `cargo install`) will naturally report as `Unverified Process` because they lack commercial Authenticode signatures.
  - *Recommendation:* Keep this behavior. Honesty over comfort: users should be clearly told that the binary is unsigned, even if located in `.cargo\bin`.
- **Follow-up:** Consider adding optional local SHA-256 allowlisting for developers who compile their own agents from source so they can manually endorse trusted local hashes.

---

## 7. Release Verdict

### **VERDICT: GO**
The C.U.R.E. Agent Intelligence MVP meets all non-negotiable product principles:
- **Local Only:** Zero network calls for scanning or flight recording.
- **Evidence-First:** Strict separation of `Observed`, `Correlated`, and `Unknown`.
- **Honesty Over Comfort:** Unsigned dummy executables are labeled `Unverified`, never verified genuine tools.
- **Privacy & Safety:** Secrets masked (`***`), paths sanitized, HTML exports escaped, zero file contents recorded.
- **Quality Bar:** 532 Rust tests, 10 GUI tests, 45 native Win32 checks passing cleanly.
