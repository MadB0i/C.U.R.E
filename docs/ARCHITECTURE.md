# C.U.R.E architecture (one page, derived from the code)

## Crates

| Crate | Dir | Role |
|---|---|---|
| `cure_core` | `core/` | Scanners, scoring, quarantine/undo, reports, signature checks. Pure logic + Windows FFI; no UI, no network. |
| `cure_cli` | `cli/` | `cure.exe`: `scan`, `diff`, `quarantine`, `undo`, `report`, `incident`, `cleanup`, `storage`. Explicit flags only. |
| `cure_watch` | `watch/` | `cure-watch.exe`: USB polling, token-pairing, pinned-GUI launch, consent, self-install/`--uninstall`, canary guard. |
| `cure_dirwatch` | `winwatch/` | Shared directory watching + canary decoy planting (used by watcher and GUI). |
| `cure_gui` | `gui/src-tauri` | Tauri v2 desktop UI (separate workspace/lockfile). Thin IPC layer over `cure_core`. |
| fixtures | `testing/fake-overlay` | Inert fullscreen test window (own workspace). |

## Data flow

```
scan:   scanners::* (registry, startup ×2, tasks, services, WMI, IFEO,
                     AppInit, COM) + process/ransom probes
          → risk::{score_entry, score_service, score_process}
          → ScoredEntry { entry, score, risk, reasons[], attack }
          → baseline.json (scan) / report files (report/incident)

quarantine: fresh-scan id → is_file_backed gate → confirm → Pending record
          (atomic save) → MoveFileExW → Committed → records.json
undo:     scoped roots check → integrity (size+sha256) → move back →
          SDDL/attrs/timestamps restore (notes on gaps)

agent:    detection::supported_processes (exact executable names)
          → recorder::baseline (path, size, mtime only)
          → watcher + recorder::reconcile (created/modified/removed/renamed, bounded)
          → evidence ladder (observed / correlated / unknown; attributed unused)
          → AgentCaseFile (allowlisted counts and labels)

storage:  catalog::roots (known-safe locations + user-selected)
          → tiers::classify (SafetyTier + the clauses that produced it)
          → walk::walk_root (no link following, cancellable, bounded)
          → duplicates::find_duplicates (size → 64 KiB → full SHA-256)
          → StorageScan { files, folders, summaries, coverage,
                          large_files, duplicates, abandoned }
          → build_plan (selection → items + refused, with consequences)
          → execute_plan (re-validate every target, then remove)
          → CleanupReceipt { verified bytes, estimate, refusals, failures }
```

The storage engine is deliberately split the same way the persistence
engine is: **measure**, then **plan**, then **execute**, with no phase
skipping another. `cure storage …` in the CLI only ever reaches the first
two; the third exists solely behind the desktop preview flow.

## Key modules (`core/src`)

- `scanners/{registry,startup,scheduled_tasks,services,wmi,ifeo,appinit,com}.rs` — read-only collectors; failures surface as coverage states, never silent zeros.
- `risk.rs` — verdict precedence: hash IOC > invalid signature > heuristics > valid-signature discount > unsigned/unknown; component-wise path matching.
- `signature.rs` — WinVerifyTrust, whole-chain revocation, cache-only by default (`--online-revocation` opts in); states VALID/INVALID/UNSIGNED/UNKNOWN/UNVERIFIED.
- `quarantine.rs` + `acl.rs` — crash-safe move protocol, orphan reconcile, SDDL restore with fidelity notes.
- `overlay.rs` — pure candidate matcher (topmost + borderless + non-ValidSigned + ≥90% coverage + attributed + not allowlisted). The GUI shows matches; each close is a separate confirmed command (graceful default, explicit force).
- `threat_intel.rs` / `hash_intel.rs` — local-only providers; fixture feed is labeled DEMO and its label travels into finding reasons.
- `agent/detection.rs` — exact-name detection for the OpenCode and Codex CLI executable names (`opencode.exe`, `opencode-cli.exe`, `opencode-tui.exe`, `codex.exe`, `codex-cli.exe`, `codex-tui.exe`) via ToolHelp; never infers activity from presence.
- `agent/recorder.rs` — metadata-only workspace baseline, bounded event reconciliation (50,000 baseline entries, 5,000 events), explicit coverage gaps and session persistence.
- `agent/windows.rs` — trusted snapshot and before/after comparison of the existing read-only `PersistenceEntry` list; added/removed/changed entries are review leads only.
- `agent/casefile.rs` — allowlisted agent case model with no path, file name, prompt or command-line field.
- `report.rs`, `incident.rs`, `process_scan.rs`, `ransom_detect.rs`, `canary.rs`, `lnk.rs`, `baseline.rs`, `elevation.rs`, `entry_details.rs`, `attack.rs`, `model.rs`, `threat_intel.rs`.

## Storage engine (`core/src/storage/`)

| Module | Role |
|---|---|
| `types.rs` | The whole vocabulary: `SafetyTier`, `StorageClass`, `FileRecord`, `FolderRecord`, `ScanCoverage`, `DuplicateSet`, `AbandonedAppRecord`, `CleanupPlan`, `CleanupReceipt`. All serialisable DTOs. |
| `tiers.rs` | `classify(path, class, ctx) -> Classification { tier, reasons }`. Provenance supplies a **floor**; only path context can raise it. Component-wise token matching, so `C:\Data` never swallows `C:\Database` and `Program FilesX` is never inside `Program Files`. |
| `walk.rs` | The single traversal in the engine. Iterative (no recursion), never follows links, records every unreadable entry, checks a shared cancel flag at each entry boundary, and reports its depth/entry/time limits instead of silently truncating. |
| `fsx.rs` | Platform facts `std::fs` will not give: allocated size (`GetCompressedFileSizeW`), attributes, reparse points, hard-link counts (`GetFileInformationByHandle`), volume free space. Every one degrades to "unknown", never to a guess. |
| `catalog.rs` | The known-safe location list, resolved from the environment at scan time. Shared vendor list with `tiers` so a browser cannot be half-recognised. Reconciles overlapping roots. |
| `duplicates.rs` | Three-stage verification. Nothing is reported that has not been read in full and hashed. |
| `abandoned.rs` | Dormant application data, always tiered `unknown`, always with a caveat. Reads the uninstall registry to say whether an installed program claims the folder. |
| `mod.rs` | `scan`, `build_plan`, `execute_plan`. `execute_plan` re-validates each target live and refuses anything that moved, changed size, became a link, left its authorised root, or changed tier. |
| `reclaim.rs` | The **union** of the removable files. The category view and the duplicate view overlap, so the headline is computed from one identity table: each path contributes its bytes once, and the duplicate figure is reported as a subset of it. This is why the headline cannot over-promise. |
| `safedelete.rs` | Race-safe removal. The file is opened, its identity on the volume is compared with the identity recorded when the plan was approved, and the *verified handle* is marked for deletion. A file that was swapped, or a parent that became a junction, is refused rather than removed. |
| `detective.rs` | Turns a completed `StorageScan` into an investigation: category, location, verified-duplicate and dormant-application leads, each with where/why/origin/risk/action/consequence. It reads the scan only; it does not touch the filesystem. Significance needs both an absolute floor and a share of the measured total. |
| `casefile.rs` | Builds the allowlisted case-file model: category labels, band labels and byte counts only. The type has no field for a path, user name, machine name or file name, and the operator note is withheld if it looks like a path. Recovery is called verified only when a receipt from the same scan observed a free-space gain. |

### The tier rule, in one paragraph

`safe_to_review` means *this class of file is regenerable*, and even then
every individual file is listed before anything is removed.
`review_carefully` means real user data: selectable, but one at a time, never
as a bulk action. `protected` means Windows, drivers, recovery, restore
points or installed applications — the engine never proposes removing it.
`unknown` means the engine could not classify the path with confidence, which
is not the same as *known to be important* and is therefore treated as
off-limits. Uncertainty escalates; it never resolves downward into a delete.

## Watcher (`watch/src`)

Poll loop (`drives` + `detector`, 1.5 s) → `.cure-trigger` token check
(`pairing::decide_launch`, pure) → spawn pinned
`%LOCALAPPDATA%\CURE\cure-gui.exe --data-dir <drive>` only. Pairing is
trust-on-first-use from removable media (`ensure_pairing`) or explicit
`cure-watch pair E:`; removal is `cure-watch --uninstall` (same constants,
verify pass, `clean`/`leftovers`).

## GUI (`gui/src-tauri/src/main.rs` + `gui/dist`)

Every mutating IPC command re-resolves fresh scan data and ignores
frontend-supplied paths; destructive ones confirm per item. `dist/` is the
shipped, fully-offline frontend (CI fails on remote URLs); `index.dev.html`
+ `mock-tauri.js` is the browser-runnable mock used for docs captures.

### Agent commands

`agent_radar`, `flight_start`, `flight_status`, `flight_stop`,
`windows_change_capture`, `windows_change_compare`, `agent_case_file`.

`flight_start` canonicalises the chosen workspace, refuses a second active
session and captures a metadata-only baseline. The watcher correlates file
events with a sampled supported-process set; `flight_stop` performs a final
reconciliation before saving the bounded session. A change is never promoted
to attribution: the strongest supported claim is **correlated**. Windows
comparison reuses the existing read-only persistence scan and stores a
snapshot under the application data directory. The agent case file is built
from an allowlist, not from the session object.

### Storage commands

`storage_available_roots`, `storage_scan`, `storage_cancel`,
`storage_preview`, `storage_execute`, `storage_reveal`,
`storage_case_file`, `storage_save_case_file`.

`storage_scan` is the only one that runs long work; it refuses to start a
second concurrent scan and relays `storage-progress` events rather than
having the UI invent progress. `storage_preview` builds the plan and
**the backend keeps it**; `storage_execute` asks for that plan by its
`plan_id` and the exact confirmation word. The caller never supplies a
selection, so it cannot ask for something other than what was shown, and
the engine recomputes the plan's digest and refuses a plan whose contents
no longer match. A mismatch means
nothing runs. The stored scan is dropped afterwards, because its numbers no
longer describe the filesystem. `storage_reveal` selects a path in Explorer
and never executes anything from it.

### Frontend

| File | Role |
|---|---|
| `app.js` | Shell, navigation, persistence results, quarantine, monitor, settings. |
| `xray.js` / `xray.css` | System X-Ray: the nine persistence layers as an instrument, driven by real coverage states. |
| `storage-workstation.js` / `.css` | Storage Workstation: scan lifecycle, squarified treemap, file table, duplicate comparison, queue, inspector, preview, receipt. It can adopt the scan the Detective board already ran instead of measuring twice. |
| `detective.js` / `.css` | Detective Mode: the storage map, ranked discoveries, lead evidence and the case-file controls. It reads the scan's `investigation` field; opening the view never starts a scan. |
| `agent-radar.js` / `.css` | Agent Radar and Flight Recorder: detected processes, workspace session, timeline, evidence, Windows change leads and the agent case file. |
| `casefile.js` | Canvas renderer for the case-file PNG. It draws only the fields returned by `CureCaseFile.allowlist`, and `audit` exposes that exact allowlist for the privacy test. |
| `luma.js` / `luma.css` | The companion rig, her twelve states, two poses, and the single global motion switch. |
| `style.css` | Graphite depth ramp, violet identity accent, semantic colours, type and spacing scales. |

All are hand-written, framework-free and dependency-free. The build step is
Tauri embedding an explicit allowlist of the shipped files, and
`mock-tauri.js` is deliberately *not* on the production list. The public demo
is built from the same frontend into `dist-demo/`, with its own Tauri config
and the deterministic mock; it has no save-case-file handler.
