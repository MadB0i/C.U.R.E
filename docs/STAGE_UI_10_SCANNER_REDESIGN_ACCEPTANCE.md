# Stage UI-10 — Scanner Redesign (final visual pass)

**Date:** 2026-11-06
**Status:** COMPLETE (16/16 Native CDP Checks Passed, incl. a REAL end-to-end scan)
**Binary Tested:** `gui/src-tauri/target/release/cure-gui.exe` (Real Tauri v2 / WebView2 native process)
**Verification Harness:** `gui/devtools/ui10-native-verify.py`

---

## 1. The design decision (my choice, as asked)

The scanner still wore the pre-workstation identity: a cyan "holographic"
radar, an amber-free palette mix, and a fully wired **Live Telemetry HUD
that was invisible** (`display: none` on a block the render path updates
every tick — dead UI). The final pass makes the scanner speak the same
grammar as every other workstation:

- **Violet is C.U.R.E acting** — the sweep beam, the active layer, the
  running pulse, the field border, the core readout chrome.
- **Info blue reports** — checked layers (canvas tone, matrix dot, and the
  new outcome pill all use the app's `#38bdf8` info tone).
- **Amber/red stay strictly semantic** (partial/denied/failed).
- The dead HUD is now the side panel's header card: current collector,
  entries inspected, layers coverage, flagged, unreadable — real figures,
  live.
- Layer rows became instrument rows: name, tabular count, and an outcome
  **pill** (CHECKED / RUNNING / PARTIAL / NOT STARTED…) tinted per outcome.
- Stale copy fixed: "Holographic radar · 9 persistence vectors" →
  "Persistence radar · 10 inspection vectors"; HUD/matrix start at "0 / 10".

## 2. Files

- `gui/dist/index.html` — scanner copy + HUD/matrix counts.
- `gui/dist/xray.css` — HUD un-hidden as a workstation card; violet field
  chrome; layer-row + outcome-pill redesign; light-theme counterparts.
- `gui/dist/xray.js` — palette unification (violet sweep/glow, info-blue
  checked tone, semantic amber/red aligned to tokens), shockwave colors.
- Sync surfaces regenerated (`dist-demo`, `index.dev.html`).

## 3. Native verification (16/16 PASS — includes a REAL scan)

| # | Check | Result |
|---|---|---|
| 1 | Radar field visible; tag says "10 inspection vectors"; "Holographic" gone | PASS |
| 2 | Live-telemetry HUD visible (was dead UI) with the four live metrics | PASS ×2 |
| 3 | Matrix renders 10 instrument rows incl. the new Winlogon & Active Setup | PASS ×2 |
| 4 | **REAL scan runs end to end** (running state → complete) | PASS ×2 |
| 5 | Complete readout honest ("1 FINDING DETECTED" on this machine) | PASS |
| 6 | HUD coverage reports the real collectors: **10 / 10** | PASS |
| 7 | Every layer row carries an outcome (10 checked / 0 partial here) with the info-blue pill | PASS ×2 |
| 8 | Overview dial draws 10 segments | PASS |
| 9 | Zero emojis, zero mascots in the scanner view | PASS ×2 |
| 10 | Dark + light + 1366×768 + 1920×1080 screenshots | PASS |

The scan was read-only; no process was terminated by the harness.

## 4. Screenshot Evidence

`testing/native/screenshots-ui10/native/` + `docs/screenshots/ui10/`:
`01-scanner-idle.png`, `02-scanner-running.png` (live sweep + "8 / 10"
hub readout + NOT STARTED pills), `03-scanner-complete.png` (10/10, REVIEW
badge), `04-scanner-light-theme.png`, `05-scanner-1366x768.png`,
`06-scanner-1920x1080.png`.

## 5. Gates

- `node --check` on all touched JS; `audit_ui_cleanliness` 0 emoji lines;
  `audit_views` 10/10; `dist` ↔ `dist-demo` byte-identical; dev shell exact.
- Rust untouched in this stage (frontend only); release exe rebuilt with the
  new frontend before verification.
