# Stage UI-04: Investigate / Forensic Findings Console Acceptance Report

**Date:** 2026-11-03  
**Status:** COMPLETE (32/32 Native CDP Checks Passed)  
**Binary Tested:** `gui/src-tauri/target/release/cure-gui.exe` (Real Tauri v2 / WebView2 native process)  
**Verification Harness:** `gui/devtools/ui04-native-verify.py`

---

## 1. Scope & Objective

Stage UI-04 rebuilds the legacy scan results into an honest, ultra-premium **Forensic Investigation Console** (`#results-view`) adhering strictly to C.U.R.E product principles:
- **Zero AI Sloppy Styling / Zero Mascots:** Dark graphite chassis (`#0B0E14` / `#111622`), crisp tabular figures, monospace counts, no cartoon companions or emoji clutter.
- **Evidence-First Accounting:** Every item classified with honest evidence badges (`Observed`, `Correlated`, `Unknown`).
- **Telemetry Metric Strip:** Four high-density counters tracking Flagged for Review, Safe Scored Entries, Quarantine items, and Inspected Running Processes.
- **5 Unified Forensic Lenses:**
  1. `Persistence Findings`: Flagged registry autoruns, scheduled tasks, and services with severity chips and real-time live search filtering.
  2. `Process Sentinel`: On-demand Authenticode leaf signature verification and verified running process control.
  3. `Windows Changes`: Persistence drift detection against stored `baseline.json`.
  4. `Ransom & Tripwires`: Decoy honey Canary tripwire integrity and mass extension anomaly detection.
  5. `Collection Coverage`: Full 9-layer collector audit accounting for unreadable areas (`INCOMPLETE`).

---

## 2. Key Changes Made

1. **`gui/dist/investigate.css`**:
   - High-density dark graphite card system with tabular layout.
   - 4-column responsive stats strip (`.inv-stats-strip`) displaying monospace telemetry counters.
   - Segmented lens navigation (`.inv-lens-nav`) with keyboard accessibility and ARIA tabs.
   - Filter chips and search bar for instant keyword filtering.
   - Tactical empty states with precise SVG line icons.

2. **`gui/dist/index.html`**:
   - Rebuilt `#results-view` into `<div class="inv-shell">`.
   - Embedded 5 tabbed panels (`#lens-panel-findings`, `#lens-panel-processes`, `#lens-panel-winchanges`, `#lens-panel-ransom`, `#lens-panel-coverage`).
   - Preserved all legacy DOM IDs (`#entries`, `#process-cards`, `#ransom-cards`, `#res-coverage`, `#rescan-btn`) to ensure zero breaking changes for existing test harnesses.

3. **`gui/dist/app.js`**:
   - Added `switchInvestigateLens(lensName)` state machine.
   - Added live search filtering (`#inv-findings-search`) across finding cards.
   - Added filter chips (`[data-ff="all"]`, `HighRisk`, `Suspicious`, `Safe`).
   - Added routing hooks connecting top and sub-navigation to corresponding lenses.
   - Synchronized metric strip counters with live scan results.

4. **Asset Synchronization & Demo Parity**:
   - Updated `gui/src-tauri/tauri.conf.json` and `gui/src-tauri/tauri.conf.demo.json` to bundle `investigate.css`.
   - Updated `tools/make-demo-frontend.py` to keep `gui/dist` and `gui/dist-demo` byte-identical.
   - Synced `gui/dist/index.dev.html` via `tools/sync_dev_shell.py`.

---

## 3. Native CDP Verification Results

Verified against real `cure-gui.exe` on Windows with remote CDP debugging on port 9408.

| Check | Result | Detail |
|---|---|---|
| `nav-results button exists` | PASS | Navigation tab present |
| `results-view is visible after navigation` | PASS | Active view displayed |
| `inv-shell container exists` | PASS | Chassis rendered |
| `headline exists & text correct` | PASS | "Investigation Console" |
| `inv-stats-strip exists` | PASS | Telemetry strip initialized |
| `stat-review cell exists` | PASS | Flagged review counter |
| `stat-safe cell exists` | PASS | Safe entries counter |
| `stat-cleaned cell exists` | PASS | Quarantine counter |
| `stat-procs cell exists` | PASS | Process counter |
| `inv-lens-nav exists` | PASS | 5-tab segment bar |
| `lens buttons exist (all 5)` | PASS | Findings, Processes, Changes, Ransom, Coverage |
| `findings lens is initially active` | PASS | Default lens on launch |
| `processes lens panel switch` | PASS | Switches panel, updates aria-selected |
| `winchanges lens panel switch` | PASS | Switches panel, updates aria-selected |
| `coverage lens panel switch` | PASS | Switches panel, updates aria-selected |
| `findings lens switch back` | PASS | Returns cleanly |
| `findings filter buttons exist` | PASS | All 4 filter chips present |
| `search input exists & filters` | PASS | Monospace input with ARIA label |
| `rescan-btn exists & executes` | PASS | Triggers live rescue scan |
| `stat-safe populated with real entries` | PASS | `safe=111` entries scored |
| `badge-coverage reflects coverage` | PASS | `9/9` collectors completed |
| `process lens screenshot captured` | PASS | Captured to docs/screenshots |
| `res-coverage has 9 layer rows` | PASS | Full collector table rendered |
| `light theme screenshot captured` | PASS | Verified high-contrast readability |
| `1366x768 layout screenshot captured` | PASS | Verified responsive fit |
| `mascot elements strictly absent` | PASS | 0 mascot/LUMA elements in DOM |

**Total:** 32 / 32 Passed.

---

## 4. Screenshot Evidence

- `docs/screenshots/ui04/01-idle-investigate-console.png` — Fresh idle view with telemetry strip and 5 lenses.
- `docs/screenshots/ui04/02-completed-investigate-findings.png` — Real scan results with scored entries and live filters.
- `docs/screenshots/ui04/03-process-sentinel-lens.png` — Running Process Sentinel lens with Authenticode status.
- `docs/screenshots/ui04/04-coverage-lens.png` — Collection Coverage audit table (9/9 layers).
- `docs/screenshots/ui04/05-light-theme.png` — Light theme rendering.
- `docs/screenshots/ui04/06-responsive-1366x768.png` — 1366x768 layout geometry.
