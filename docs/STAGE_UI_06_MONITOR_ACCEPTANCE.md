# Stage UI-06 — Monitor Workstation, Canary Tripwire & Login Observation

**Date:** 2026-11-05
**Status:** COMPLETE (57/57 Native CDP Checks Passed)
**Binary Tested:** `gui/src-tauri/target/release/cure-gui.exe` (Real Tauri v2 / WebView2 native process)
**Verification Harness:** `gui/devtools/ui06-native-verify.py`

---

## 1. Scope & Objective

Complete the workstation rebuild for the last three non-workstation views and repair the
two reachability regressions left behind by Stage UI-04:

1. **Monitor Workstation** (`#view-eventlog`) — session telemetry with live counters,
   kind filters and a tactical empty state.
2. **Canary Tripwire** (`#view-canary`) — guard status hero, honest experimental
   disclosures (wording preserved verbatim), session alert feed.
3. **Login Observation chamber** (`#view-incident`) — restored as a reachable view and
   rebuilt in the workstation grammar; all render-path IDs preserved.
4. **Windows Change Radar** — moved from the removed dead view into the Investigate
   "Windows changes" lens, replacing a static stub that dishonestly claimed
   "No Drift Detected" before any comparison existed.
5. **Dead-section removal** — `#view-audit`, `#view-processes`, `#view-winchanges`
   (unreachable duplicates of Investigate lenses since UI-04) deleted together with
   their dead nav branches, dead audit render path and dead filter bindings.

## 2. Key Changes Made

- `gui/dist/monitor.css` (new): `m-` workstation grammar — shell, header, telemetry
  strip, rail, feed cards/tags, guard hero, disclosures, verdict row, timeline restyle,
  light theme, responsive breakpoints. Empty feeds collapse (`:has(> .m-feed:empty)`)
  so no hollow pill renders above the tactical empty state.
- `gui/dist/index.html`: three sections rebuilt; dead sections removed; Windows Change
  Radar markup moved into `#lens-panel-winchanges`; ransom lens gained a cross-link
  card into the observation chamber; process lens regained its All/High risk/Suspicious
  filter chips (lost in UI-04); `monitor.css` linked.
- `gui/dist/app.js`: feed filter state + live counter updates (`updateMonitorStats`),
  new row grammar (`buildEventRow`), canary rows restyled, guard-note state text,
  `navTo`/`VIEW_TITLES`/`setNav` cleanup, dead audit render path removed
  (`renderAudit`/`paintAuditList`/`buildAuditCard`), `paintProcessRows` now also
  filters the visible lens cards.
- Sync surfaces: `tauri.conf.json` + `tauri.conf.demo.json` asset allowlists,
  `tools/make-demo-frontend.py` copy list, `gui/dist-demo/` regenerated,
  `gui/dist/index.dev.html` regenerated via the guarded `make-dev-shell.py`.

## 3. Native CDP Verification Results (57/57 PASS)

| # | Check | Result |
|---|---|---|
| 1 | nav-eventlog button exists | PASS |
| 2 | view-eventlog is visible after navigation | PASS |
| 3 | m-shell container exists | PASS |
| 4 | headline text is correct ("Monitor Workstation") | PASS |
| 5 | telemetry stats strip exists | PASS |
| 6–9 | stat cells #m-stat-events / canary / actions / last exist | PASS ×4 |
| 10 | events counter starts at 0 | PASS |
| 11 | last-event counter starts em-dash | PASS |
| 12 | monitor rail has 3 destinations | PASS |
| 13 | rail routes are eventlog, canary, incident | PASS |
| 14 | session-events rail tab is current | PASS |
| 15 | feed empty state visible on fresh launch | PASS |
| 16 | 4 feed filter chips exist | PASS |
| 17 | feed list is empty on fresh launch | PASS |
| 18 | clear-view button exists | PASS |
| 19 | view-incident is visible via monitor rail | PASS |
| 20 | observation headline is correct ("Login Observation") | PASS |
| 21 | 4 observation durations offered | PASS |
| 22 | start-observation button exists | PASS |
| 23 | incident results render after **real 15 s observation** | PASS |
| 24 | verdict badge is populated (STRONG CORRELATION, real) | PASS |
| 25 | verdict text is honest vocabulary | PASS |
| 26 | scope coverage rows rendered | PASS |
| 27 | timeline rows rendered | PASS |
| 28–29 | export TXT / export JSON buttons exist | PASS ×2 |
| 30 | inspect drawer opens over observed evidence | PASS |
| 31 | session counter updated by the observation | PASS |
| 32 | actions counter updated by the observation | PASS |
| 33 | feed rows rendered | PASS |
| 34 | canary filter shows zero rows (honest empty) | PASS |
| 35 | action filter shows the run's action rows | PASS |
| 36 | action rows carry the Action tag | PASS |
| 37 | all filter restores full feed | PASS |
| 38 | view-canary is visible via monitor rail | PASS |
| 39 | canary headline is correct ("Canary Tripwire") | PASS |
| 40 | guard state badge starts OFF | PASS |
| 41 | guard toggle visible | PASS |
| 42 | experimental disclosure keeps honest wording | PASS |
| 43 | canary alerts feed present | PASS |
| 44 | winchanges lens panel visible | PASS |
| 45 | radar controls live inside the lens | PASS |
| 46 | **real capture** records a trusted snapshot (111 supported entries) | PASS |
| 47 | **real compare** renders the diff summary | PASS |
| 48 | differences stay review leads, not verdicts | PASS |
| 49 | ransom lens cross-link exists | PASS |
| 50 | cross-link lands on the observation chamber | PASS |
| 51–56 | zero emojis + zero mascot elements in all three views | PASS ×6 |
| 57 | telemetry counters use monospace tabular figures | PASS |

The observation (check 23–30) ran against the **real backend**: 242 processes and
12 windows observed, 47 correlations computed, investigation id recorded. The Windows
Change Radar round-trip (checks 46–47) performed a **real** capture + compare; it only
writes to the harness's temp data dir.

**Process safety:** the harness never terminates any process and never starts the
canary guard (the real guard plants decoys in user folders; that path is exercised by
the interactive suite against the labelled mock shell). The only process touched is
the harness-owned `cure-gui.exe` via the lease-file ownership protocol.

## 4. Screenshot Evidence

Captured by the harness to `testing/native/screenshots-ui06/native/` and copied to
`docs/screenshots/ui06/`:

| File | Shows |
|---|---|
| `01-login-observation-idle.png` | Observation chamber, dark, idle, honest limits |
| `02-login-observation-results.png` | Real 15 s observation: verdict, timeline, STRONG correlation cards |
| `03-monitor-session-feed.png` | Monitor Workstation with live counters + action rows |
| `04-canary-tripwire.png` | Canary guard hero + disclosures + empty alert state |
| `05-monitor-light-theme.png` | Light theme |
| `06-monitor-1366x768.png` | Responsive layout, smallest supported window |
| `07-monitor-1920x1080.png` | Full-size layout |

## 5. Regression Repairs Included

| Regression | Repair |
|---|---|
| Login observation unreachable since UI-04 (navTo intercepted `view-incident` into a lens without observation controls) | `navTo` interception removed; view restored; reachable via Monitor rail + ransom-lens cross-link; verified end-to-end natively |
| Windows Change Radar controls stranded in dead `#view-winchanges` | Moved into `#lens-panel-winchanges` with real capture/compare verified |
| Process lens lost its filter chips in UI-04 while JS state remained | Chips restored; `paintProcessRows` filters visible cards |
| Winchanges lens stub claimed "No Drift Detected" before any comparison | Replaced by the real radar with honest "No trusted persistence snapshot is stored yet" |
| Dead duplicate sections + dead audit render path | Removed (sections, nav branches, `renderAudit`/`paintAuditList`/`buildAuditCard`, dead bindings) |

## 6. Gates

- Root workspace: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, **598 tests pass / 0 fail**.
- GUI crate: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean, **14 tests pass / 0 fail**.
- `gui/dist` ↔ `gui/dist-demo`: shared assets byte-identical (`app.js`, `monitor.css` verified via `git diff --no-index`).
- `index.dev.html` = `index.html` + exactly the two labelled substitutions (regenerated by `gui/devtools/make-dev-shell.py`).
- `python tools/audit_views.py`: all 10 views found. `python tools/audit_ui_cleanliness.py`: zero emoji/mascot matches in the three rebuilt views (pre-existing matches in older-stage views are tracked for the UI-08 sweep).
- Zero AI-sloppy styling: no emojis, no mascots, monospace tabular counters, dark graphite `#0B0E14`-ramp surfaces, violet used only for identity.
