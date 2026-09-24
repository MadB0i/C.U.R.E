> **[HISTORICAL] This is an archived development report from the V4 development cycle (Sep 2026). It contains internal paths and context specific to the author's machine. For current documentation, see docs/validation/REAL-PC-VALIDATION.md, docs/ARCHITECTURE.md, and docs/CHANGELOG.md.**

# C.U.R.E. V4 FINAL REPORT — real quarantine + no Desktop writes + futuristic forensic UI

Base: `eea2cad` (V3). Working tree: 9 modified + 5 new files (see §15).
DO NOT COMMIT (per instruction). Reference image note: no image file was
attached to the conversation or repo — the redesign follows the written
direction (futuristic dark security-console, structured sidebar, command
dashboard, restrained violet/cyan accents, technical panels).

## 1. QUARANTINE ROOT CAUSE

Not a UI bug. `run_auto_scan` (`gui/src-tauri/src/main.rs`, old lines
346–360) **auto-relocated every HighRisk file-backed finding mid-scan
with zero user confirmation**. Consequences observed in real-world testing:

1. Files vanished during the scan — the user never clicked Quarantine.
2. A later manual Quarantine click re-scans (`find_current_entry`) and
   finds nothing → `entry is no longer present in the latest scan` error,
   so manual quarantine *appeared* broken.
3. It violated the app's own contract (no automatic destructive action,
   explicit confirmation) and the CLI's (`NO automatic remediation`).
4. It also wrote `quarantine/records.json` next to the exe (Desktop).

The explicit path itself was already correct: confirm modal →
`quarantine_entry` (fresh-scan lookup, registry/non-file refusal,
move + record) → list refresh → scoped undo. No change needed there.

## 2. QUARANTINE FIX

- Removed the auto-clean branch. ALL HighRisk/Suspicious findings land
  in `suspicious_for_review`; relocation happens only via explicit
  per-item confirm → `quarantine_entry`. `high_risk_cleaned` stays as an
  always-empty field so the `ScanSummary` shape is stable.
- `QUARANTINED` stat tile now counts **this session's confirmed manual
  quarantines** (`sessionQuarantined`, +1 on invoke success, −1 on scoped
  undo success, floored at 0). Removed the dead `quarantined
  automatically` headline branch (could never trigger).
- Real errors still surface (`Quarantine failed`, `Undo failed`); registry
  / service / WMI / IFEO / AppInit / COM stay manual-guidance only.
- `mock-tauri.js` (dev-only) mirrors the fixed backend: no auto-clean,
  no `cleaning` stage; file-backed HighRisk → review queue.
- Proven by `gui/devtools/quarantine-regression.mjs` (16/16, real CLI
  binary in a temp sandbox): detect → scan does NOT move → quarantine →
  bytes identical → record listed → double-quarantine idempotent →
  undo → bytes identical → record cleared.

## 3. BASELINE JSON ROOT CAUSE

`resolve_data_dir()` defaulted to **the folder holding the executable**.
The shipped `cure-gui.exe` sits on the Desktop → every scan wrote
`baseline.json`, `quarantine/`, `records.json`, exports and
`overlay-dismissal.log` onto the Desktop. Classification: **F —
accidental production write of app state to an unowned location**
(repo-root `baseline.json` / `quarantine/` were the same bug from dev
runs; both removed as stale ignored artifacts). CLI was never affected
the same way (explicit `--data-dir`, console-printed path).

## 4. BASELINE PATH FIX

- `resolve_data_dir()` default → `%LOCALAPPDATA%\CURE` (app-owned;
  `temp\CURE` fallback if the var is missing). `--data-dir` override
  kept for portable/USB workflows and tests. Single choke point fixes
  baseline, quarantine records, exports, and the overlay log at once.
- New Rust unit test `data_dir_tests::…_never_exe_adjacent` (gui
  workspace) pins the default; `gui/devtools/baseline-guard.mjs`
  proves a real scan leaves Desktop/home byte-identical and artifacts
  land only in `--data-dir`, plus a static guard on the default.
- Note: stale `baseline.json`/`quarantine/` already on a Desktop from
  the old build are inert leftovers — safe to delete; items quarantined
  under the old location are not auto-migrated (no silent file moves).

## 5. UI FILES CHANGED

- `gui/dist/style.css` — full V4 reskin (all 247 JS/HTML-referenced
  classes covered, 0 undefined/unused tokens, no duplicate selectors
  outside media/reduced-motion overrides).
- `gui/dist/index.html` + `index.dev.html` — identical structural adds:
  4 nav-group labels, `#session-meta` topbar slot (dev/prod still differ
  by exactly the 2 pre-existing mock lines).
- `gui/dist/app.js` — session-meta renderer (real scan data only),
  session quarantine counter, auto-clean fallout removal (below).
- `gui/dist/mock-tauri.js` — behavior mirror (above).
- `gui/src-tauri/src/main.rs` — the two backend fixes + data-dir test.
- Devtools: `quarantine-regression.mjs`, `baseline-guard.mjs`,
  `v4-probe.mjs`, `v4-shots.mjs` (new); `verify/cleanupcheck/diag.mjs`
  assertions re-pointed from the removed auto-clean signals to live
  ones (`__cureVisitActive`, `__cureResolvedCount`).

## 6. FUTURISTIC DESIGN CHANGES

Graphite + technical grid backdrop, violet primary / cyan secondary /
amber-experimental / red-destructive-only / green-verified-only.
Grouped sidebar (SCAN/INVESTIGATE/MANAGE/MONITOR) with illuminated
active spine; topbar with mono screen title + live session context
(`LAST SCAN … · N CHECKED · N TO REVIEW`) + status pill; Overview as a
12-column command grid (8 real metric tiles + coverage/report/actions/
incident panels); framed scan console with restrained sweep; evidence
cards (SEVERITY→FINDING→TARGET→EVIDENCE→ACTION); timeline spine on
incident events; dense mono process table; premium quarantine rows;
explicit cleanup pipeline; technical event log; canary stays
unmistakably EXPERIMENTAL. No fake metrics, maps, scores, or relations.

## 7. DEAD CODE REMOVED

- JS: `dispatchMascot()` (fired only on auto-clean), `ping()` (never
  called, even before), `glitchPillText`+timer, `lastItemNode`,
  `appendStageLine` (sole caller was the cleaning branch), both
  `stage === "cleaning"` warn-line conditions.
- CSS: `.warn-line`, `#status-text.glitch`, `glitchJitter` keyframes,
  stale `var(--shadow)` reference, 4 unused tokens.
- Harnesses updated (not deleted): mascot-count waits → visit/resolved
  signals; stale `per-cleaning beats` comment; diag `pings` → resolved.
- Nothing removed without a traced-zero-caller check; dynamic families
  (`risk-*`, `cov-dot`, `src-icon`, `proc-killed`) retained.

## 8. DEAD CODE RETAINED + WHY

- `high_risk_cleaned` (always empty): keeps `ScanSummary` shape stable
  for frontend/mock consumers; honest zero, not a fake.
- `cleaned-cards` panel + `fillCards(..., true)`: renders empty/hidden;
  removing the DOM would break `getElementById` call sites.
- `q-undo` class, `pings` render loop, `mascot`/`visit.fight` fields:
  unstyled hooks / empty-loop harmless; draw path untouched for safety.
- Mock's 1 seeded quarantine record: intentional list-content fixture.
- CLI exe-adjacent default: documented, console-printed, explicit —
  not the unexpected-write bug (GUI was).

## 9. ACCESSIBILITY

Keyboard nav, `:focus-visible`, modal trap + Escape + focus restore
(5/5 viewports), `alertdialog`, nav/table names, 32px targets,
selectable evidence (61 nodes), reduced-motion kills ALL ambient loops
(sweep, rings, bobs, pulses, reveals→instant). New styling did not
reduce any V3 a11y property (v4-probe asserts each).

## 10. CONTRAST (measured, ≥4.5:1)

metric 8.86 · empty 8.86 · chip-red 7.92 · chip-amber 10.13 · subline
8.86 · rc-name 16.64 · session-meta 6.32 · nav-group 5.95 · q-paths 8.86.

## 11. RESPONSIVE (900×700, 1280×720, 1366×768, 1600×900, 1920×1080)

hOverflow=0, footer quarantine button pointer-reachable, icon-rail
sidebar ≤980px, overview grid collapses to 1 column ≤1120px,
results/map stack vertically on narrow windows. 0 errors.

## 12. OVERLAP

Scroll ownership preserved (`.view > .results-stack` single scroller);
modal/drawer above content (`z 9000/9999`); footbar toast
pointer-transparent; no clipped cards/tables/timeline in any viewport.

## 13. SECURITY REGRESSION

Backend changes are ONLY the two fixes (§2, §4). Preserved:
deterministic detection, explicit destructive confirmation, scoped
undo (+roots), PID-reuse protection, correlation, CLI matching,
redaction, canary EXPERIMENTAL. Still absent: telemetry, screenshots,
keylogging, credential extraction, injection, network threat lookup,
automatic remediation. Added-line scan for network/telemetry: 0 hits.
`git diff -- '*.rs'` outside `gui/src-tauri/src/main.rs`: empty.

## 14. TEST RESULTS

```
cargo test --workspace (root)        267 passed, 0 failed (224+7+36)
cargo test --workspace (gui)           2 passed, 0 failed (overlay + data-dir)
TOTAL                                269 passed, 0 failed
cargo clippy (both workspaces, -D warnings)   clean
cargo check --workspace --all-targets --all-features   0 errors
cargo build --release (root + gui)    0 errors (fresh cure-gui.exe 20:48)
verify / chipcheck / pixelcheck / cleanupcheck / canarycheck   ALL PASS
quarantine-regression.mjs (new)       16/16 PASS
baseline-guard.mjs (new)              6/6 PASS
v4-probe.mjs (new: 5 viewports + a11y)   ALL PASS
v4-shots.mjs (all-views smoke)        ALL PASS (8 real metric tiles, 4/4 panels)
session-counter E2E (mock)            0→1→0, list 2→1 (seed retained), 0 errors
```
Count note: 268→269 is the ADDED data-dir unit test, not a deletion.
One transient `verify` failure during the run was a dropped Internet
route (pre-existing Google-Fonts CDN link fails offline); re-ran green
with connectivity. Font stack degrades to system fonts offline.

## 15. BEFORE/AFTER SCREENSHOT SUMMARY

V3 baseline: `%USERPROFILE%\AppData\Local\Temp\opencode\v3-shots\`.
V4: `%USERPROFILE%\AppData\Local\Temp\opencode\v4-shots\`
(`v4-overview/results/modal/incident` @1280×720, 1366×768, 1920×1080).
Visible delta: grouped glowing sidebar, session-context topbar, metric
command grid, severity-spine evidence cards, gridded scan-map rail,
timeline spine, mono technical tables — same C.U.R.E., futuristic
instrument feel, zero invented data (mock harness data only in dev).

```
git status --short
 M gui/devtools/cleanupcheck.mjs
 M gui/devtools/diag.mjs
 M gui/devtools/verify.mjs
 M gui/dist/app.js
 M gui/dist/index.dev.html
 M gui/dist/index.html
 M gui/dist/mock-tauri.js
 M gui/dist/style.css
 M gui/src-tauri/src/main.rs
?? CURE_V4_FINAL_REPORT.md
?? gui/devtools/baseline-guard.mjs
?? gui/devtools/quarantine-regression.mjs
?? gui/devtools/v4-probe.mjs
?? gui/devtools/v4-shots.mjs

git diff --stat
 gui/devtools/cleanupcheck.mjs |    4 +-
 gui/devtools/diag.mjs         |    2 +-
 gui/devtools/verify.mjs       |    9 +-
 gui/dist/app.js               |  101 +-
 gui/dist/index.dev.html       |    5 +
 gui/dist/index.html           |    5 +
 gui/dist/mock-tauri.js        |   21 +-
 gui/dist/style.css            | 2921 +++++++++++++++++++++--------------------
 gui/src-tauri/src/main.rs     |   75 +-

git diff --check
(clean)
```

Desktop: rebuilt `cure-gui.exe` (V4 UI + both fixes) copied to
`%USERPROFILE%\OneDrive\Desktop\cure-gui.exe`; no baseline/quarantine
JSON on Desktop. NOT COMMITTED — awaiting instruction.
