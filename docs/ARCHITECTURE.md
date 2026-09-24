# C.U.R.E architecture (one page, derived from the code)

## Crates

| Crate | Dir | Role |
|---|---|---|
| `cure_core` | `core/` | Scanners, scoring, quarantine/undo, reports, signature checks. Pure logic + Windows FFI; no UI, no network. |
| `cure_cli` | `cli/` | `cure.exe`: `scan`, `diff`, `quarantine`, `undo`, `report`, `incident`, `cleanup`. Explicit flags only. |
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
```

## Key modules (`core/src`)

- `scanners/{registry,startup,scheduled_tasks,services,wmi,ifeo,appinit,com}.rs` — read-only collectors; failures surface as coverage states, never silent zeros.
- `risk.rs` — verdict precedence: hash IOC > invalid signature > heuristics > valid-signature discount > unsigned/unknown; component-wise path matching.
- `signature.rs` — WinVerifyTrust, whole-chain revocation, cache-only by default (`--online-revocation` opts in); states VALID/INVALID/UNSIGNED/UNKNOWN/UNVERIFIED.
- `quarantine.rs` + `acl.rs` — crash-safe move protocol, orphan reconcile, SDDL restore with fidelity notes.
- `overlay.rs` — pure candidate matcher (topmost + borderless + non-ValidSigned + ≥90% coverage + attributed + not allowlisted). The GUI shows matches; each close is a separate confirmed command (graceful default, explicit force).
- `threat_intel.rs` / `hash_intel.rs` — local-only providers; fixture feed is labeled DEMO and its label travels into finding reasons.
- `report.rs`, `incident.rs`, `process_scan.rs`, `ransom_detect.rs`, `canary.rs`, `lnk.rs`, `baseline.rs`, `elevation.rs`, `entry_details.rs`, `attack.rs`, `model.rs`, `threat_intel.rs`.

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
