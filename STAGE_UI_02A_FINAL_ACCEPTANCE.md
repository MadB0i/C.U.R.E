# STAGE UI-02A — Final Agent Radar Polish + Safe Process Control

**Branch:** `feat/cure-operator-storage-defense` · **Baseline:** `c760a48` (Stage UI-02)
**Binary:** `gui/src-tauri/target/release/cure-gui.exe` — all native checks run against the real Tauri app (real IPC, real Win32 APIs, no mocks).

## What was removed from the Agents layout

- The **Windows Change Radar** section (Capture Trusted Snapshot / Compare Now) is no longer a large bottom section of the Agents page.
- The **Agent Case File** section (Preview, Evidence Card, Markdown, JSON, HTML) is no longer a standalone bottom section.
- Result: the Agents page is one screen — compact radar + agent inventory on the left, Flight Recorder on the right. No scrolling required to reach agent-management controls at 1600×900 or 1366×768.

## Where Windows Change Radar is now accessible

A new Investigate sub-view **`Investigate / Windows changes`** (`view-winchanges`) reachable from the Findings view's tool-nav ("Windows changes") and from the other Investigate tool-navs. It hosts the same controls and the same saved snapshot storage (`agent::windows` load/save) — nothing was deleted, no baselines lost, and the regression coverage follows the feature (devtools + native checks exercise capture/compare at the new location). This is the only cross-page change; no other page was redesigned.

## Where Case File export and Replay moved

Both now live **inside Flight Recorder** as compact, request-gated controls:

- **Export** chip (visible only for a stopped session) reveals the privacy-safe export workflow: optional screened operator note, Preview, Evidence Card, Markdown, JSON, HTML. Hidden until requested.
- **Review timeline** chip (stopped sessions only) reveals the replay slider + timeline. During live monitoring the replay controls do not exist on screen; closing review returns the event list to the live position.
- A **previous (recovered) recording** appears collapsed as a "Previous recording" bar (workspace · event count · stopped time) with **View events**; it never resumes, and the badge says **Stopped**. The radar stays "Not scanned this session" across restarts.

## Agent process-control implementation

**Core** (`core/src/agent/session.rs`):
- `process_has_windows(pid)` — read-only EnumWindows probe (graceful WM_CLOSE is only meaningful when a window exists; console tools have none).
- `process_exit_confirmed(pid)` — post-action verification via `GetExitCodeProcess` (not image-name queries, which fail misleadingly on terminated processes).
- `end_agent_process_single(pid, expected_exe, expected_start, token)` — ends **one** process, never its children: token gate (`END_<pid>`), critical-system refusal, `verify_pid_identity` anchor (rejects exited / PID-recycled / name-mismatched / stale-time targets), graceful WM_CLOSE **only** if the process owns a window with a bounded 2 s wait, then single-PID `TerminateProcess`, then up to 3 s of `process_exit_confirmed` polling. Success is only reported after Windows itself confirms the exit. Every attempt is written to the audit journal (`AgentProcessEnd`) without secrets; the receipt reports the method used (`graceful` / `forced` / `already_exited`).
- `DetectedAgentProcess.has_windows` — surfaced so the UI can label the action honestly.

**Tauri command** (`gui/src-tauri/src/main.rs`): `operator_agent_end_process` is **agent-scoped by construction** — the backend re-enumerates supported agent processes itself and refuses any PID that is not a currently detected supported agent process, any name mismatch, and any start-time disagreement (>2 s) with the renderer's snapshot. The renderer cannot name an arbitrary process; there is no unrestricted process-killer IPC. Tree termination remains behind the separate, explicitly token-authorised `operator_agent_terminate`.

**UI** (`gui/dist/agent-radar.js` + `index.html`):
- Each agent card expands into **one row per process**: PID, image name, start time, measured working set, identity label (with full classification on hover), sanitised path, and per-process actions `Details` (inspector) and `End process` — labelled `Force stop` when the process owns no windows, with the reason in the confirmation.
- Confirmation modal (shared `CureConfirm` from app.js): "End \<Agent\> process?" with PID / Executable / Started / Identity / **Potential impact** ("Active coding tasks and unsaved session work may be interrupted.") and the honest method note; `Cancel` | `End Selected Process` / `Force Stop`.
- Post-action: the outcome line reports closed-gracefully / force-stopped / already-exited / refusal / staleness; the radar automatically re-enumerates and reflects the exit; `Cancel` reports "no change was made".

## Windows safety boundaries (implemented and tested)

1. Target must be a **currently detected supported agent process** (backend re-enumeration — enforced).
2. Live identity anchor: image name + creation time must match (PID-reuse and stale-snapshot rejection — enforced, tested).
3. Critical/system process refusal (`CRITICAL_SYSTEM_PROCESSES`, PID 0/4 — enforced, tested).
4. OS permissions respected; failures surfaced as "Access denied"/"Windows refused" — never retried with elevation (no elevation logic exists).
5. No tree kill without the separate explicit command (single-PID only in the new path — by design, verified).
6. No automatic termination after detection; nothing is killed for being unsigned/unverified (identity is displayed, never used as a kill criterion).
7. Success claims require Windows-side exit confirmation (`GetExitCodeProcess`).
8. All attempts journalled; secrets never recorded.

## Real native termination test evidence

`gui/devtools/ui02a-native-verify.py` — **26/26 PASS**. The harness spawns its own harmless process (a copy of `cmd.exe` named `codex.exe` in a disposable `D:\CURE_UI02A_WS` workspace, running a bounded ping), so the owner's real Codex CLI was never targeted:

- detected alongside real agents → expanded rows → cancel (process unaffected) → confirm → **"Force-stopped — PID \<pid\> force-stopped (TerminateProcess)."** → `poll()` confirms exit → radar refresh no longer lists the PID → audit journal contains the `AgentProcessEnd` entry → stale-target IPC (wrong start time) rejected → PID 4 (System) rejected as "not a currently detected supported agent process".

Recorder scenarios on the same disposable workspace: start/monitoring sweep, real created+modified events (past the 2 s dedupe window), replay hidden while live, review-mode replay, privacy-audited case export, restart-collapsed history. Plus: fresh-launch honesty, no mascot, 1366×768 without overflow, light theme.

## Regression results

| Suite | Result |
|---|---|
| Devtools `verify` (incl. updated agent flow: process control, cancel, review gating, export menu, collapsed history, WCR under Investigate) | **589 PASS** |
| Devtools `layout` / `pixel` | **184 / 112 PASS** |
| Native `ui02a-native-verify.py` | **26/26 PASS** |
| `native-e2e.py` (full app regression incl. storage, receipts, mascot) | **37 PASS** |
| UI-01A native regression (`ui01a-native-verify.py`) | **13/13 PASS** |
| `cargo fmt --check` / `cargo clippy --workspace --all-targets -D warnings` | PASS |
| `cargo test --workspace` | **514/515** — 5 new session tests pass; the single failure remains `acl::tests::sddl_and_attributes_roundtrip`, the known pre-existing environmental failure (needs elevation to restore a security descriptor; reported honestly, not suppressed — this stage's Rust additions all pass) |

## Before / after

- Before: tall radar panel with the instrument above the list; two large bottom sections (Windows Change Radar, Case File); always-visible purple replay slider; process controls buried in a modal dialog with `prompt()` tokens; no per-process actions.
- After: one-screen command center — compact radar with side readout, per-process rows with `Details | End process`, confirm-gated scoped termination, recorder with live list / review-mode replay / collapsed history / request-gated export; Windows Change Radar under Investigate.

## Remaining unsupported functionality

- Session **recovery/resume** still not offered anywhere (artifacts reported, opaque formats labelled unsupported).
- No graceful-close guarantee for console processes: without a window there is nothing to close, so the action is labelled `Force stop` and the method is reported in the outcome.
- No CPU% (not measured by the backend); memory shown only when Windows returns a working-set value.
- Process exit between manual scans is only visible in the next snapshot (no background process watcher).
- `acl::tests::sddl_and_attributes_roundtrip` remains environmentally red (pre-existing, needs elevation).

## Owner-impact disclosure

During an intermediate test run, a shell-level cleanup command (`taskkill /IM codex.exe`) killed the owner's two running Codex CLI processes; it did **not** touch session data and was immediately recognised as a mistake. The committed harness never does this: it terminates only its own spawned child **by PID**, and reaps its orphaned ping by parent-PID. The product code can never do it by construction (agent-scope guard). The owner may need to restart their Codex CLI sessions.

## Final executable

`D:\Projects\CURE\gui\src-tauri\target\release\cure-gui.exe`

## Screenshots (all self-reviewed; no clipping found)

`testing/native/screenshots-ui02a/native/`:
- `01-native-fresh-agents.png` — fresh idle radar, compact layout
- `02-native-detected-agents.png` — real agents + collapsed history bar
- `03-native-end-process-confirm.png` — End Process confirmation (PID, executable, identity, impact, Force Stop)
- `04-native-recorder-compact.png` — compact Flight Recorder while recording
- `05-native-light-theme.png` — light theme
- `06-native-history-open.png` — opened previous recording
- `07-native-1366.png` — 1366×768 layout
