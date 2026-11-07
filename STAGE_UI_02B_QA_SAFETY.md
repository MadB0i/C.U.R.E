# STAGE UI-02B — QA Safety Gate Report

**Branch:** `feat/cure-operator-storage-defense` · **Baseline:** `a0fbe8d`
**Scope:** test-harness process-safety only. No UI redesign, no product behaviour changes except one pure-function extraction for testability.
**Acceptance:** no automated test or cleanup routine can terminate an owner-controlled process through broad name-based matching.

## 1. Root cause of the Codex interruption

The UI-02A acceptance harness itself never killed the owner's Codex (it terminates only its own spawned child by PID). The kill came from an **interactive shell cleanup command** I ran between test iterations: `taskkill /IM codex.exe /F`, which matches *every* process with that image name — including the two owner-launched Codex CLI processes (verified-signature, running since 07:49/07:50 that morning). Name-based killing cannot distinguish "the test's fixture" from "the owner's tool", which is why this entire class of command is now banned from the repository, not just corrected.

## 2. Read-only audit findings (before correction)

| File | Pattern | Risk |
|---|---|---|
| `testing/native/native-agent-audit.py` | psutil `process_iter` kill loop matching `msedgewebview2`, `cure-gui`, **`opencode`** | **critical** — would kill the owner's real OpenCode processes and every WebView2 app on the machine |
| `testing/native/native-e2e.py` | `taskkill /F /IM msedgewebview2.exe` + psutil name-filter fallback | high — kills all WebView2 apps machine-wide |
| `gui/devtools/ui01a-native-verify.py` | `taskkill /F /IM cure-gui.exe` + `/IM msedgewebview2.exe` | high — also kills an owner-launched C.U.R.E instance |
| `gui/devtools/ui02-native-verify.py` | same | high |
| `gui/devtools/ui02a-native-verify.py` | same | high |
| `gui/devtools/produce-cure-demo.py` | `taskkill /F /IM cure-gui.exe /T` | medium — kills owner-launched C.U.R.E |
| `testing/vm-stage/run-cure-validation.ps1` + `_parts/c-setup.ps1` | `Get-Process cure-watch \| Stop-Process -Force` (pre-existing instances) | medium — name-based; VM-scoped but still forbidden |
| `testing/fake-overlay/src/main.rs` | comment recommending `taskkill /IM fake-overlay.exe` | low (doc) |

Compliant as found: `ui01-native-captures.py`, `ui01-probe-status.py` (only terminate their own tracked Popen children); `native-e2e.py` shutdown (psutil parentage walk of its own PID); `ui02a` ping reaper (by `ParentProcessId`).

## 3. Corrections

### `testing/native/process_ownership.py` (new, shared)
The one place in the repository with termination primitives:
- **Lease file** per harness (`.process-lease-*.json`, git-ignored): records the exact PID, image name, and OS-reported creation time of every process the harness launched, plus its per-run `WEBVIEW2_USER_DATA_FOLDER` and CDP port.
- **Identity re-verification before any signal:** a previous run's leftover is killed only if the live OS name *and* creation time still match the lease (2 s tolerance). A recycled PID, name disagreement, or exited process is **skipped and reported** — `skipped_stale` entries carry `recorded` vs `live_now` diagnostics.
- **Webview2 children** are located by the run's own user-data folder in their command line — never by image name alone.
- **Port fail-fast:** if the CDP port is still occupied after ownership-scoped cleanup, `ensure_port_free` raises `ProcessSafetyError` with the likely holder's PID/name/command line. No fallback kill exists.
- The helper contains only `taskkill /F /T /PID <n>` — there is no name-based code path in it at all.

### Harnesses rewired
`native-e2e.py`, `native-agent-audit.py` (the `opencode` killer — now also removed its unused psutil import), `ui01a/ui02/ui02a-native-verify.py`, `produce-cure-demo.py`: pre-launch cleanup goes through `pre_launch_cleanup(lease, port)`; launches record their lease; shutdowns clear it. `produce-cure-demo.py`'s webview cleanup now matches its own user-data folder. All previous name-based kills are gone from these files.

### vm-stage
`Get-Process cure-watch | Stop-Process` replaced with **abort with diagnostics** (lists pre-existing PIDs + start times, saves evidence, exits 1): in a clean guest, a pre-existing cure-watch is unexpected and possibly owner-controlled. Its own launched watcher is still stopped by exact PID (`Stop-Process -Id`).

### fake-overlay fixture comment
Recommends PID-scoped closure; name-based suggestion removed.

### Static guard — `core/tests/qa_process_safety.rs` (new, runs in `cargo test --workspace`)
Three tests:
1. `no_name_based_process_killing_in_tooling` — scans `testing/`, `gui/devtools/`, `tools/` (`.py/.ps1/.mjs/.js/.rs`) for: `taskkill /IM`; `Stop-Process` without `-Id`; `Get-Process` piped into `Stop-Process`; psutil `process_iter` referencing a protected image (Codex, OpenCode, Antigravity, Gemini, Aider, Claude, Cursor, Windsurf, Ollama, ZCode, msedgewebview2, cure-gui); any direct `TerminateProcess` in tooling; any kill line referencing a protected image. Fails with file+line+reason. Only `taskkill /F /T /PID <n>` is permitted.
2. `native_harnesses_use_the_ownership_helper` — the five harnesses that launch cure-gui must route cleanup through `process_ownership` / `pre_launch_cleanup`; inline cleanup is what allowed the Codex kill.
3. `ownership_helper_never_kills_by_name` — the helper itself may only use the PID-scoped form and must not contain `Stop-Process`.

(The guard flagged my own explanatory comments quoting the banned patterns during development — reworded; the guard is already doing its job.)

## 4. Agent Radar End Process verification (requirement 5)

Unchanged product surface, now with extracted, unit-tested ownership rules:
- `validate_end_process_target` (pure, `gui/src-tauri/src/main.rs`) is the single gate in front of `end_agent_process_single`: the live process must come from a **fresh backend enumeration** (renderer names alone never count), the recorded image name must equal the live name, and the recorded start time must agree with the live one (2 s tolerance) or the action is refused. Four unit tests cover accept / name-mismatch / stale-time / unreadable-start-time.
- The command then re-proves identity again in core (`verify_pid_identity` anchor), refuses critical-system processes, requires the `END_<pid>` token, uses graceful WM_CLOSE only when the process owns windows, terminates only the selected PID (never children), and reports success only after `GetExitCodeProcess` confirms the exit. Every attempt is journalled (`AgentProcessEnd`) without secrets.
- Existing regression coverage intact: 4 core `end_agent_process_single` tests (wrong token, PID 4 refusal, already-exited, stale time), devtools `agent_radar_flow` (confirm + cancel paths through the modal), native `ui02a` scenario 9–12.

## 5. Codex interruption — read-only evidence (requirement 6)

Examined without opening any session file or database:
- Session store `~\.codex\sessions\2026\10\30\*.jsonl`: newest entry 2026-10-30 16:13 — **nothing written on 31 Oct** (the killed processes had started 07:49/07:50 on 31 Oct and had not flushed new rollout files).
- Store databases (`logs_2.sqlite`, `state_5.sqlite-shm/-wal`, `queue_1.sqlite-shm`): modification stamps on 31 Oct 15:54–16:55 come from the **restarted** processes (new PIDs started 15:54:37 and 16:32:23), not from the killed ones; `queue_1.sqlite-shm` created 15:54:37 = restart time.
- One 0-byte `tmp\arg0-windows\...\.lock` dated 16:32:23 belongs to the restarted process's normal temp pattern; no stale lock from the killed PIDs observed in the store root.
- Two Codex processes are running again (app-server + exec-server). **The owner restarted or Codex relaunched them; I did not.**

**Verified:** store locations, modification stamps, restart state, absence of observed stale locks. **NOT verified (and not claimed):** contents/integrity of any session or database, whether the killed CLI sessions lost unsaved work, whether Codex recovered cleanly. Only Codex's own tooling can verify that; nothing in this report should be read as a data-integrity claim.

## 6. Safety-test results (no owner process touched)

- **Static guard:** 3/3 tests pass; clean repo scan. (It failed during development on three comment lines quoting the banned patterns — reworded; the guard demonstrably catches even prose.)
- **Non-destructive ownership validation** (`testing/native/qa_ownership_check.py`): **8/8 PASS**, executing zero terminations — the only spawned process is a scratch `cmd /c ping -n 15` that exits by itself. Proven: exact-PID identity lookup with OS creation time; a mismatched lease entry (wrong name + wrong creation time) is classified `skipped_stale` with recorded-vs-live diagnostics **and the live process survives the release call**; an already-gone entry is classified gone; webview2 scoping matches nothing outside the recorded folder.
- **Rewired harnesses, real runs:** `native-e2e.py` **37/37 PASS** and `ui01a-native-verify.py` **13/13 PASS** — both now run pre-launch lease cleanup (identity-verified) + port fail-fast. No name-based kill is reachable.
- **Rust gates:** `cargo fmt --check` PASS; `cargo clippy --workspace --all-targets -D warnings` PASS; `cargo test --workspace` 514+4+3 passed / 1 failed — the failure is the known pre-existing environmental `acl::tests::sddl_and_attributes_roundtrip` (needs elevation; unchanged, reported honestly).
- **Devtools:** `evidence-console.py verify` **589 PASS** (mock-only; UI untouched).

## 7. Files changed

- **New:** `testing/native/process_ownership.py`, `testing/native/qa_ownership_check.py`, `core/tests/qa_process_safety.rs`, `STAGE_UI_02B_QA_SAFETY.md`
- **Corrected:** `testing/native/native-e2e.py`, `testing/native/native-agent-audit.py`, `gui/devtools/ui01a-native-verify.py`, `gui/devtools/ui02-native-verify.py`, `gui/devtools/ui02a-native-verify.py`, `gui/devtools/produce-cure-demo.py`, `testing/vm-stage/run-cure-validation.ps1`, `testing/vm-stage/_parts/c-setup.ps1`, `testing/fake-overlay/src/main.rs`
- **Testability extraction:** `gui/src-tauri/src/main.rs` (`validate_end_process_target` + 4 unit tests)
- **Housekeeping:** `.gitignore` (lease files), removed my own stray debug scripts and three leaked synthetic lnk-fixture directories from `core/` (untracked, test-created; the underlying lnk-test cwd leak is reported as a finding, not fixed here)

## 8. Remaining risks

1. **Interactive shell commands are outside the repo's reach.** The guard covers committed tooling; an operator can still type a broad `taskkill`. The lease flow makes such commands unnecessary: a stale harness instance is cleaned by the next run's identity-verified release, so nobody should ever reach for a name-based kill again.
2. **First run after this change on a machine with a crashed legacy leftover:** the lease file may not exist (old runs never wrote one). Cleanup then does nothing and the port check fails with diagnostics naming the holder — manual resolution required by design. If the leftover is a C.U.R.E instance the operator chooses to close, that is their action, not automation.
3. **The lnk tests leak `core/cure-startup-*` fixture dirs into the repo cwd** (pre-existing; I removed three leaked instances). Their fix (OS temp dir) belongs to the lnk test owner; noted here because it surfaced during the QA hygiene pass.
4. `acl::tests::sddl_and_attributes_roundtrip` remains environmentally red (needs elevation; pre-existing across stages).
5. Webview2 cleanup after a hard crash of the *host* run relies on the lease written at launch; if the machine power-cycles before the lease is written, the orphaned webview holds only its temp profile — no user data — and the port check reports it.

**Final executable:** `D:\Projects\CURE\gui\src-tauri\target\release\cure-gui.exe`
