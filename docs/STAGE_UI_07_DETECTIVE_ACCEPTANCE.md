# Stage UI-07 — Detective Mode Polish

**Date:** 2026-11-05
**Status:** COMPLETE (24/24 Native CDP Checks Passed)
**Binary Tested:** `gui/src-tauri/target/release/cure-gui.exe` (Real Tauri v2 / WebView2 native process)
**Verification Harness:** `gui/devtools/ui07-native-verify.py`

---

## 1. Scope & Objective

Bring Detective Mode to the same workstation grammar as the other views without
touching the deliberate case-board design language (its `dt-` band hue system and
board internals stay exactly as designed):

1. **Case telemetry strip** — four monospace cells (Measured total / Review
   candidates / Files measured / Discoveries) fed from the same investigation the
   board renders, so the strip and the map can never contradict each other.
2. **Mascot removal** — the leftover `operator-mount`/`data-companion="detective"`
   slot is gone from the view (the last one inside a view body).
3. **Honest idle state** — before any measurement every cell shows an em-dash,
   never a zero, and the subline states "Nothing is measured until you start an
   investigation."

## 2. Key Changes Made

- `gui/dist/index.html`: `dt-stats-strip` added between the head and the
  investigation path; mascot mount removed.
- `gui/dist/detective.css`: strip styles using the shared tokens; `.dt-operator-slot`
  rule removed (dead); light-theme + responsive (960/640) rules.
- `gui/dist/detective.js`: `renderStats(inv)` — one source of truth read by both the
  strip and the board; `None found` (not zero) when no bytes are reviewable.
- Sync surfaces: `gui/dist-demo/` regenerated, `index.dev.html` regenerated.

## 3. Native CDP Verification Results (24/24 PASS)

| # | Check | Result |
|---|---|---|
| 1 | nav-detective button exists | PASS |
| 2 | view-detective is visible after navigation | PASS |
| 3 | headline text is correct | PASS |
| 4 | idle subline stays honest | PASS |
| 5 | case telemetry strip exists | PASS |
| 6–13 | 4 stat cells exist, each showing an honest em-dash before any measurement | PASS ×8 |
| 14 | mascot elements absent from detective view | PASS |
| 15 | no emojis present in detective view | PASS |
| 16 | start-investigation button exists | PASS |
| 17 | **real investigation completes** (read-only quick scan from the board) | PASS |
| 18 | measured-total stat populated from the real scan (13.32 GB) | PASS |
| 19 | files-measured stat is a number (28083) | PASS |
| 20 | discoveries stat is a number (40) | PASS |
| 21 | candidates stat populated or honest "None found" | PASS |
| 22 | map core total matches the strip | PASS |
| 23 | board rendered discovery list or honest empty | PASS |
| 24 | strip figures use monospace tabular numerals | PASS |

The investigation (check 17) is the **real read-only engine** — nothing under
`storage` removes a file, and the harness terminates no process.

## 4. Screenshot Evidence

Captured by the harness to `testing/native/screenshots-ui07/native/` and copied to
`docs/screenshots/ui07/`:

| File | Shows |
|---|---|
| `01-detective-idle.png` | Idle board, honest em-dashes, dark |
| `02-detective-measured.png` | Real measurement: strip, map, ranked discoveries |
| `03-detective-light-theme.png` | Light theme with populated detail panel |
| `04-detective-1366x768.png` | Responsive, smallest supported window |
| `05-detective-1920x1080.png` | Full-size layout |

## 5. Gates

- Root workspace: `cargo fmt --check` clean, clippy `-D warnings` clean, 598 tests pass.
- GUI crate: `cargo fmt --check` clean, clippy `-D warnings` clean, 14 tests pass.
- `gui/dist` ↔ `gui/dist-demo` shared assets byte-identical; `index.dev.html` = index + exactly two substitutions.
- Zero AI-sloppy styling: no emojis, no mascots, monospace tabular counters; the four
  band hues still mean exactly one thing each on the board.
