# C.U.R.E — Stage V1.3 Final Acceptance Report
**Real Capability & Security Acceptance**

Date: 2026-10-30
Branch: `feat/cure-operator-storage-defense`
Baseline: `bc341b7` (V1.2 "real-world capability acceptance")
Result commit: see git log after this file (committed locally, not pushed)

---

## 1. Current branch and commit

- Started from: `feat/cure-operator-storage-defense` @ `bc341b7` (checked out per plan; the
  working tree had been left on `feat/agent-intelligence-release` @ `02e8cdb`, which is an
  ancestor of `bc341b7` and contains none of the V1.1/V1.2 operator work — user approved
  the switch; nothing was reset or overwritten).
- All V1.3 work is committed locally on the same branch. No push, no PR, no tag.
- PR #6 and all other branches are untouched.

## 2. Verified baseline

Ran on the actual `bc341b7` tree (not accepted from prior reports):

- `core/src/operator/{defense,journal,response}.rs`, `core/src/agent/session.rs`,
  `core/src/storage/rescue.rs` (2,252 lines) — read line by line.
- 12 IPC commands confirmed registered in `gui/src-tauri/src/main.rs`
  (`storage_rescue_finalize/activate/rollback/cleanup_source`, `operator_status`,
  `operator_agent_sessions/graceful_shutdown/terminate`, `operator_defender_scan/contain`,
  `operator_audit_log`, `operator_verify_audit_journal`).
- V1.1 audit report (`STAGE_V1_1_AUDIT_REPORT.md`) present on branch; **no V1.2 report
  exists anywhere** — the V1.2 commit message is the only record of that stage.

## 3. Previous claims: confirmed / disproven

**Confirmed by direct code reading and live runs:**
- Full-content SHA-256 verification: `copy_tree_verified` hashes **every** file at source
  and destination (not just samples) and gates `is_verified` on files+bytes+all hashes.
- Reparse-point/junction protection in discovery, copy, and cleanup (`collect_files_recursive`
  skips reparse points; copy rejects reparse-point sources).
- OneDrive awareness via path + `User Shell Folders` registry scan, elevating to `Caution`.
- 12-agent taxonomy: OpenCode, Codex, Claude Code, Aider, Ollama, Cursor, Cline, Continue,
  Gemini CLI, Copilot, Antigravity, Windsurf (`AgentKind::ALL`, detection.rs:202).
- Defender audit-only separation: `scan_file_with_defender` always passes `-DisableRemediation`;
  remediation requires a separate explicit call with token `CONTAIN_DEFENDER_<filename>`.
- PID-recycle guard, critical-system-process immunity, path boundary guard, 14-process
  kill blocklist — all real and tested.
- Journal hash chaining (SHA-256, prev-hash pointers) + DPAPI anchor; tamper detection tested.
- Honest session taxonomy (Running / Artifacts / Active / Recoverable / Unsupported) and
  "SESSION RECOVERY UNSUPPORTED" labeling for proprietary stores.

**Disproven / corrected:**
- **"Every operator action is recorded in the tamper-evident audit journal" — FALSE at
  runtime.** `init_global_journal` was never called anywhere in the codebase; every
  `audit_log(...)` call was a silent no-op, `operator_audit.jsonl` was never written, and
  the audit UI could only ever show an empty log. (Fixed — D8.)
- **"128-bit nonce replay defense" — materially incomplete.** Consumption never verified
  the nonce had been issued (any caller-supplied 32-hex string was accepted, so the
  activation→cleanup gate was bypassable), and all nonce state died with the process, so
  replayed nonces succeeded after an app restart. (Fixed — D1/D2.)
- V1.1 report text says "32-bit hex nonce (8 characters)"; the code has always generated a
  128-bit (32-hex-char) nonce. Documentation error, not a code error.
- **"584 passing tests" (V1.2 claim): plausible but not independently reproduced.** No test
  was re-run at the bare baseline (rebuilding the baseline tree twice was rejected to spare
  the low-space C: drive). 584 is consistent with 574 workspace + 10 GUI, and the current
  tree — which adds 9 workspace tests — measures **586 workspace + 10 GUI**.

## 4. Defects found (all fixed, with regression tests)

| # | Severity | Defect | Root cause |
|---|----------|--------|------------|
| D1 | Critical (security) | Source-cleanup accepted any caller-minted 32-hex nonce; the activation gate was bypassable without ever activating | `consume_nonce_strict` checked only the consumed-set, never `is_nonce_issued` |
| D2 | High (security) | Nonce replay protection died with the process; consumed nonces could be replayed after restart | Nonce state lived in in-process statics only |
| D3 | Critical (data safety) | Cleanup hard-deleted the source with no re-verification that the destination still matched — a drifted/incomplete copy could not stop deletion | Verification happened once at copy time; deletion used a stale receipt |
| D4 | High (safety) | `ActivationReceipt.previous_path` was always empty; a failed validation left the system pointing at the new (unproven) location | Previous registry/env values never captured; no rollback on failed activation |
| D5 | High (reliability) | Receipts existed only in frontend JS state; an interrupted/crashed migration left nothing to reconcile after restart | No persistence layer for lifecycle records |
| D6 | Medium | Lock probe only sampled 20 top-level files and flagged read-only files as "locked" (write-open probe) | Wrong probe primitive; not recursive |
| D7 | Medium (honesty) | "ACTIVE SESSION" was reported whenever a process ran and its store dir was non-empty — days-old artifacts counted as an active conversation; no resource usage shown | No recency measurement; no memory query |
| D8 | High (honesty/security) | The entire hash-chained audit journal was dead code | `init_global_journal` never called |
| D9 | Medium (detection accuracy) | Genuine, canonically-installed unsigned tools (observed live: `Antigravity.exe`, which carries a stray embedded cert blob) were graded `SUSPECT MISMATCH` | Publisher evidence was used even when the signature verdict was `Unsigned` |
| D10 | Medium (safety UX) | A parity-refused cleanup burned the single-use nonce, forcing re-activation for a legitimate retry | `consume_nonce_strict` ran before the verification gates |

## 5. Root causes and actual fixes

All in `core/src/storage/rescue.rs` unless noted:

- **D1+D2**: Nonce logic rebuilt as an instantiable `NonceLedger` (issued/consumed, ordered,
  capped at 8,192 entries) behind the existing global API. `set_nonce_store_path(path)`
  loads prior state from `<data_dir>/operator_nonces.json`; every issue/consume persists
  atomically (temp+rename). `recover_nonce_issuances_from_journal()` re-imports activation
  nonces from the audit journal at startup so an activation done before a restart still
  unlocks its cleanup. Consumption now requires issuance.
- **D3**: New `verify_copy_parity_for_cleanup` — final gate immediately before
  `fs::remove_dir_all`: identical relative file sets (case-insensitive), identical sizes,
  and **SHA-256 of every file pair**; refuses on reparse-point asymmetry. A stale verified
  receipt can no longer authorize deletion of original data.
- **D4**: `activate_candidate_relocation` captures the previous Known Folder / env value
  before writing (new `previous_configuration` receipt field) and, when validation fails,
  automatically restores it — including removing a variable it created when none existed
  (new `remove_user_environment_variable`); incomplete rollbacks are journaled honestly
  (`RollbackIncomplete`) and flagged in the receipt (`auto_rollback_performed`).
- **D5**: `MigrationTransactionRecord` persistence (`<data_dir>/migration-receipts/`,
  atomic writes, filename-safe ids, traversal-proof), `load_transaction_records`, and a
  read-only `reconcile_transaction_record` that compares each record against the live
  filesystem/registry (e.g. "copy no longer exists — do NOT delete the source"). GUI
  persists records at every lifecycle step and exposes them via new IPC command
  `storage_rescue_receipts`.
- **D6**: Probe now uses `fsx::is_in_use` (share-mode-0 open; only kernel sharing/lock
  violations count) over a bounded recursive walk (depth 8, ≤500 files, never descends
  reparse points). Read-only files are no longer false positives.
- **D7**: `SessionRecoveryInfo.latest_activity_unix` added; a session is "ACTIVE SESSION
  (RECENT ARTIFACT ACTIVITY)" only when its newest artifact was modified within 30 minutes,
  otherwise "PROCESS RUNNING (SESSION ARTIFACTS PRESENT BUT NOT RECENTLY ACTIVE)".
  `DetectedAgentProcess.memory_usage_bytes` (working set) added via
  `GetProcessMemoryInfo` — feature-gated on the existing `windows` dependency
  (`Win32_System_ProcessStatus`); **no new dependency**.
- **D8**: GUI startup initializes the global journal at `<data_dir>/operator_audit.jsonl`
  and the nonce store (production builds only; the demo build remains engine-free).
- **D9**: In `verify_process_identity`, publisher-based verdicts (expected publisher,
  runtime-conflict, unexpected-publisher) are applied **only** when the signature verdict
  is `ValidSigned`/`ValidRevocationUnknown`; an `Unsigned`/`Unknown` verdict can never
  produce `SuspectMismatch`. Verified live: real Antigravity processes grade `Verified`
  via canonical install path; Codex stays `Verified` (OpenAI signature); unsigned OpenCode
  CLI stays honestly `Unverified`.
- **D10**: The nonce is consumed only after all verification gates pass, immediately before
  deletion — "consumed" now always means "a deletion actually ran".

## 6. Test commands and results (all run on this machine)

| Command | Result |
|---|---|
| `cargo fmt --check` (workspace and `gui/src-tauri`) | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (0 warnings) |
| `cargo clippy --all-targets -- -D warnings` (gui/src-tauri) | PASS (0 warnings) |
| `cargo test --workspace` | **586 passed, 0 failed** (5 cli + 510 core lib + 3 rescue E2E + 10 dirwatch + 58 watch; 3 ignored = live host probes) |
| `cargo test` (gui/src-tauri) | 10 passed, 1 ignored (desktop overlay test, by design) |
| `python testing/native/native-e2e.py --shots D:\CURE_V13_ACCEPTANCE\native-shots` | **36/36 PASS** (real release binary over WebView2 CDP) |
| `python testing/native/native-agent-audit.py` | **45/45 PASS** (real IPC: radar, flight recorder, secret masking of spawned `opencode.exe` with fake key, case-file export) |

New regression coverage added this stage: 9 unit tests (nonce fabrication, restart
persistence, journal recovery, atomic persistence, two parity-gate refusals, record
persistence/round-trip, record-id traversal rejection, unsigned-never-mismatch) + 3
disposable-volume E2E tests + 2 `#[ignore]` live probes.

## 7. Real native functional evidence

1. **Full migration lifecycle on a disposable D: root** (`core/tests/storage_rescue_e2e.rs`,
   fixtures under `D:\CURE_V13_ACCEPTANCE\`, removed after each test):
   copy → full SHA-256 verify (4/4 files) → tampered destination **refuses** cleanup
   ("SHA-256 mismatch") → restored copy → approved cleanup deletes source → nonce replay
   refused → source untouched on every refusal. Junction test: `mklink /J` inside source —
   copy skips it (`reparse_points_safely_skipped ≥ 1`), junction target untouched, cleanup
   refused. Lock test: an open handle is detected as in-use; released handle passes.
2. **Fresh-child-process config proof**: scratch value written to `HKCU\Environment`
   (the exact key `set_user_environment_variable` writes) was read back by an independent
   fresh process (`reg query HKCU\Environment /v CURE_TEST_V13_VAR` → `probe-ok`) and then
   removed (verified empty). No real `CARGO_HOME`/`GRADLE_USER_HOME`/`PIP_CACHE_DIR` or
   Known Folder was ever touched.
3. **Live agent discovery** (`cargo test --test live_probes -- --ignored --nocapture`):
   real processes found with PID, exe path, **memory usage**, and graded identity —
   Codex CLI ×2 `Verified`, OpenCode `Unverified`, Antigravity ×6 `Verified` (after D9
   fix; before the fix they were falsely `SuspectMismatch`). Offline session stores:
   Codex 1 session (2026-10-02), Claude Code 9 (2026-10-28), Antigravity brain 4
   (2026-10-30, today), Aider none; Ollama/Cursor/Windsurf/Cline/Continue/Gemini/Copilot
   honestly reported `UNSUPPORTED (PROPRIETARY)` — directories existing does not make
   sessions recoverable.
4. **Defender audit-only scan against the real host engine**: `MpCmdRun.exe` resolved at
   `C:\ProgramData\...\Platform\CurrentVersion\MpCmdRun.exe`; benign fixture scanned with
   `-DisableRemediation` → `CleanNoThreats`, exit 0, file byte-identical afterwards.
   Parser unit tests cover threat/audit/remediation-failed/error/skipped outcomes.
5. **Native E2E** (real release exe + CDP): 36/36 including storage cleanup receipt
   honesty ("does not present an estimate as verified"), cancellation, and the
   data-dir isolation policy. Agent audit: 45/45 including secret-masking of a spawned
   fake `opencode.exe` with an `sk-ant-…` argument (never leaked, never Verified).

## 8. Security findings and residual risks

Fixed this stage: D1–D4, D8, D10 (see §4/§5) plus IPC surface extension `storage_rescue_receipts`
(read-only; file names are validated against traversal; records live under the app data dir).

Residual risks (documented, not hidden):
- **DPAPI journal anchor is not tamper-proofing**: an attacker executing as the same user
  can re-seal a forged anchor. The code's own threat-model comment says exactly this;
  the guarantee is detection of casual tampering/truncation, not cryptographic immunity.
- **TOCTOU window** between the parity check and `remove_dir_all` in cleanup is small but
  nonzero (single-user desktop context; lock probe mitigates the common cases).
- **Cleanup parity hashing cost**: full re-hash of the source at deletion time is
  deliberate (safety over speed); very large trees pay it.
- **`WM_SETTINGCHANGE`**: running processes (and some shells) only pick up new user env
  vars after restart/re-login; activation receipts say what was written but C.U.R.E cannot
  force live processes to re-read the environment.
- **`storage_rescue_receipts` is backend-only** — the frontend does not render the
  receipts view yet (no cosmetic work in this stage, per scope).
- **Pre-existing (not introduced, not fixed)**: `gui/dist-demo/storage-workstation.css`
  diverges from `gui/dist` at `bc341b7` — a byte-identity rule deviation that predates
  this stage; left untouched per "no unrelated changes".
- **Defender `contain` relies on filename-scoped token** `CONTAIN_DEFENDER_<filename>`
  plus an exact user-chosen path; Defender performs the actual quarantine. Reasonable,
  but the trust anchor is the local UI, as with all local-only tooling.

## 9. Remaining incomplete capabilities

- Registry *Known Folder* activation and env-var activation have only been exercised
  against scratch values and workspace (advice-only) candidates; a real
  Downloads/CARGO_HOME relocation was **not** performed on the owner's machine (prohibited
  by the data-safety rules).
- OneDrive *cloud* behavior after relocation (de-sync semantics) is detected and warned
  about, not tested against a live OneDrive sync.
- Windsurf session store is detected (`AppData\Roaming\Windsurf\User\workspaceStorage`,
  observed present on this host) but honestly reported unsupported (proprietary SQLite).
- Frontend UI for receipts/reconciliation; CLI surface for the rescue lifecycle.
- The 3 live probes are `#[ignore]`d and are not CI evidence; they document host-specific
  checks only.

## 10. Honest release verdict

| Feature | Verdict |
|---|---|
| Storage discovery + preflight (space, headroom, same-volume, OneDrive) | **PASS** |
| Copy + verification (count/bytes/full SHA-256, reparse-safe, cancellable) | **PASS** |
| Activation (registry/env writes, read-back validation, previous-value capture, auto-rollback on failure) | **PASS** (scratch-value + workspace candidates; real-folder relocation NOT VERIFIED by policy) |
| Source cleanup (nonce, locks, parity gate, journaling) | **PASS** on disposable data; real developer data: **PARTIAL** (mechanism proven, live relocation not exercised) |
| Interrupted-migration recovery (persisted records + reconcile + nonce durability) | **PASS** (mechanism; no real crash was staged mid-migration) |
| Rollback durability across restart | **PASS** (ledger + journal recovery + reconcile; restart simulated in tests) |
| Agent process detection (12 kinds, identity grading, memory usage) | **PASS** (validated live) |
| Agent session discovery / recovery honesty | **PASS** for what it claims; recovery itself is **NOT VERIFIED beyond detection** (recovery of proprietary formats is explicitly unsupported) |
| Defender audit-only scan | **PASS** (live, benign fixture) |
| Defender remediation (contain) | **PARTIAL** — token gating and parser unit-tested; no real detection/remediation exercised (no malware used, per rules) |
| Threat response (process containment, quarantine) | **PASS** (PID-race guards, blocklist, quarantine two-phase commit; unchanged from V1.2, re-tested) |
| Audit journal | **PASS** (now actually initialized and persisting; tamper-evident, not tamper-proof) |
| Tauri IPC / frontend | **PASS** for existing flows (36/36 native E2E); new receipts command is backend-only |

### Explicit safety decisions

| Decision | Verdict & conditions |
|---|---|
| Safe to scan files | **YES** — read-only walks, no link-following, honest INCOMPLETE coverage |
| Safe to migrate disposable data | **YES** — proven end-to-end on D: fixtures |
| Safe to migrate real developer data | **YES with review** — copy is verified and source-preserving; Caution-tier candidates require the operator to follow the reconfiguration advice; tool behavior after relocation must be confirmed by the operator (a copy probe alone is never proof) |
| Safe to activate relocated configurations | **CONDITIONAL** — previous values are now captured and failed validation auto-restores, but a *successful* validation is still a filesystem+registry probe, not proof the tool works; test the tool in a fresh process before cleanup |
| Safe to clean original source data | **YES after the parity gate** — deletion now requires a byte-level SHA-256-verified faithful copy *at deletion time* plus a valid unused nonce; refused attempts preserve the source and do not burn the nonce |
| Safe to detect AI agents | **YES** — read-only enumeration; identity grading now free of the unsigned-binary false-mismatch defect; no process is killed without explicit token |
| Safe to recover AI sessions | **NO for proprietary formats** (honestly labeled unsupported); read-only inspection of JSON/markdown stores is safe; C.U.R.E performs no destructive session operation |
| Safe to run Defender scans | **YES** — `-DisableRemediation` always; cannot modify Defender settings; exit codes are never treated as "clean" without the parsed outcome |
| Safe to remediate Defender threats | **YES with explicit approval** — separate token-gated command; Defender does the quarantine; post-state is checked and reported honestly; unverified filename-only suspicion is never actionable |

**Overall**: the migration lifecycle's destructive path is now gated by real evidence
(issuance-bound nonces, durable replay protection, deletion-time parity), the audit
journal actually exists, and agent detection gained live-validated accuracy. The product
is honest about what it cannot do. Recommended next: surface the receipts/reconciliation
view in the frontend and exercise one real (operator-approved) CARGO_HOME relocation in a
controlled session.

## What is NOT verified (blunt list)

- Any operation against the owner's real user folders, caches, or registry Known Folders
  (prohibited by this stage's own safety rules).
- A real application crash *between* copy and cleanup (recovery is test-simulated, not
  field-observed).
- Defender remediation against a real detection; behavior of Defender's own quarantine
  folder; event-log correlation.
- OneDrive de-sync consequences; elevated/kernel-adjacent remediation; third-party AV.
- The bare `bc341b7` baseline test count (rebuild rejected to conserve the low-space C:
  drive; current counts are the evidence).
- GUI rendering of the new receipts data (backend-only in this stage).
