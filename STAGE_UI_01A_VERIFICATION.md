# STAGE UI-01A — Verification Report

**Branch:** `feat/cure-operator-storage-defense` · **Baseline:** `ead9982` (Stage UI-01)
**Scope:** Overview scan flow, navigation ownership, status consistency.
**Binary used for all native checks:** `gui/src-tauri/target/release/cure-gui.exe` (real Tauri + WebView2 + Rust backend; no mocks).

## Root cause of the forced navigation

`runScan()` in `gui/dist/app.js` unconditionally performed
`showViewInstant(scanView)` when a scan started and
`switchView(currentView(), resultsView)` + `setNav("results-view")` when it
finished — regardless of where the scan was launched from. A scan started
from the Overview therefore dragged the operator into the Rescue view and
then onto the Investigate findings view. There was no concept of a scan
*origin*, and completion navigation was not gated on whether the operator
had navigated elsewhere meanwhile.

A second, latent bug surfaced while testing the fix: `switchView()` bumps
the shared `viewToken` itself, so any post-`await` comparison of
`viewToken` against a token captured before the call is always false. In
practice this meant the completion handler could not reliably tell
"operator navigated" apart from "the transition itself moved the token".

## State-management corrections (single scan lifecycle)

One authoritative lifecycle remains (`scanPhase` + `scanToken`); origin is
a property of the current run, not a competing state:

- `scanOrigin` (`"rescue"` default, `"overview"`) records who started the
  scan; `scanNavToken` records the navigation-ownership baseline at scan
  start; `scanProgressLine` carries the backend's latest collector
  message.
- `_runScan()` is the internal implementation; the public `runScan()`
  refuses to start while a scan is `running` **or** `preparing`, so
  repeated clicks can never stack collections (the overlay-review
  hand-off calls `_runScan` deliberately — it is the same flow).
- `startScanFlow(origin)` is the single scan-start path shared by both
  buttons (`#start-rescue-btn` on Rescue, `#overview-rescue-btn` on
  Overview). Rescue keeps the historical behaviour (switch to scan view,
  results view at the end). Overview-origin scans switch views only when
  the overlay-review step genuinely needs the operator (the review cards
  live in the scan log); with no candidates the operator never leaves the
  Overview.
- Completion navigation is doubly gated: rescue-origin only, and only
  when `viewToken === scanNavToken` (no manual navigation since start)
  **and** `currentView() === resultsView` after the transition (the
  `switchView` token bump cannot false-positive any more). Manual
  navigation always wins; completion only refreshes data in place.
- A failed scan renders a dedicated failure state (`renderOverview(null)`
  path): posture "Operation failed", coverage "—", findings "—", dial
  reset, findings panel hidden. The previous summary stays available in
  Investigate but is never presented as the failed operation's result,
  in-session or after restart.
- Stale callbacks: completion/failure are guarded by `scanToken`; a
  superseded scan (`token !== scanToken`) updates nothing.

## Overview scanning experience (truthful, event-driven)

- Hero shows `Scanning` with a pulsing beacon; the nine-segment dial
  switches to `Layers reporting` and advances **only** when a layer
  actually reports (`scan-progress` `source-state` events).
- Live line under the headline: `Collecting — <collector message from the
  backend> · elapsed <real seconds>` — updated from real progress events
  and the run's own ticker (500 ms). No invented percentages or collector
  names; there is no cancel command for the rescue scan in the backend,
  so no cancel control is offered.
- The activity timeline streams real session events while scanning; the
  rest of the dashboard stays usable; "Review findings" stays available
  for the previous completed scan (labelled as such) and "Start System
  Scan" is disabled until the run finishes.

## Status semantics

- Coverage metric (`ov-coverage-state`) now reports only `COMPLETE` /
  `PARTIAL` / `—`. Collector errors and access denials make coverage
  *partial*; the per-layer list keeps `CHECK FAILED` / `ACCESS DENIED`
  details visible. The word `FAILED` is reserved for the scan operation
  itself (assessment metric + posture + pill say `Operation failed`).
- The vocabulary `Not assessed / Scanning / Assessment complete /
  Partial coverage / Attention required / Operation failed` is rendered
  from the single `sessionStatus()` model, so hero, metrics, pill and
  timeline cannot contradict each other. Zero findings with incomplete
  coverage is still `Partial coverage`, never clean.
- Cancelled state: not applicable — the rescue-scan backend exposes no
  cancellation command, so none is claimed.

## Exact files changed

- `gui/dist/app.js` — scan-origin ownership, live scan line, coverage
  semantics, failure state, duplicate-scan guards.
- `gui/dist/index.dev.html`, `gui/dist-demo/*` — regenerated by
  `gui/devtools/make-dev-shell.py` and `tools/make-demo-frontend.py`.
- `gui/devtools/evidence-console.py` — new `scan_origin_flow` regression
  suite (registered in `verify` mode).
- `gui/devtools/ui01a-native-verify.py` — new native scenario suite.
- `testing/native/screenshots-ui01a/native/*.png` — native captures.
- `STAGE_UI_01A_VERIFICATION.md` — this document.

No Rust, workflow, version, or other-page files were touched.

## Regression tests added (mock shell, deterministic)

`scan_origin_flow` in `gui/devtools/evidence-console.py` covers:

1. Overview-origin scan keeps Overview visible, hero reports `Scanning`,
   live elapsed line, dial in `Layers reporting` mode.
2. Exactly one `run_auto_scan` invocation; five re-clicks during the scan
   start nothing (button disabled + lifecycle guard).
3. Completion leaves the operator on Overview; Investigate is never
   force-opened; the scan button re-arms; "Review findings" is offered;
   posture uses the honest vocabulary; the scanning line is replaced by
   the scan's scope summary.
4. Rescue-origin scan still lands on the findings view (title becomes
   `Investigate`).
5. Manual navigation to Agents mid-scan survives completion (title stays
   `AI Agent Radar`).
6. Completion race on the rescue path: navigating at the moment the scan
   finishes keeps the operator on Agents.
7. Failed overview scan: posture `Operation failed`, coverage/findings
   `—`, findings panel hidden, no navigation.

Suite results: **verify 547 checks PASS · layout 184 PASS · pixel 112
PASS** (`python gui/devtools/evidence-console.py verify|layout|pixel`).

## Native test scenarios and results

`python gui/devtools/ui01a-native-verify.py` (real binary, real scans):

| # | Scenario | Result |
|---|----------|--------|
| 1 | Fresh launch → Overview idle (`Not assessed`, no header summary) | PASS |
| 2 | Overview → Start System Scan → stays on Overview, live collector line + elapsed, dial reporting, screenshot during scan | PASS |
| 3 | Overview scan completes → Overview remains, review button re-armed, data updated | PASS |
| 4 | Rescue → Scan Again → stays in Rescue workflow → results view, title `Investigate` | PASS |
| 5 | Overview scan → navigate to Agents mid-scan → completion respected (`AI Agent Radar`) | PASS |
| 6 | Overview scan → navigate to Rescue mid-scan → completion respected (scan view) | PASS |
| 7 | Rapid repeated clicks → single collection (button disabled during run; also asserted via mock command counting in the devtools suite) | PASS |
| 8 | Partial coverage (real machine: 8/9 layers, access denied) → posture `Partial coverage`, coverage `PARTIAL`, never `FAILED`, never clean | PASS |
| 9 | Scan error → truthful failure state (mock suite: `Operation failed`, coverage `—`, no stale findings; native backend cannot be forced to fail without fault injection) | PASS (devtools) |
| 10 | Restart → neutral session (`Not assessed`), no stale results, previous durable records intact | PASS |
| 11 | Live PC clock advances across ticks (`10:53:23 → 10:53:25 am`) | PASS |
| 12 | Mascot fully absent (`window.Luma`/`window.companion` undefined, zero luma nodes) | PASS |

**13/13 native scenario checks passed.** The full `native-e2e.py` suite
(37 checks: navigation, real storage scan, detective board, removal
receipt, mascot removal, records hygiene) also passes on this binary.

## Rust / frontend gates

- `cargo fmt --check` — PASS
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS
- `cargo test --workspace` — 509/510 PASS; the single failure
  (`acl::tests::sddl_and_attributes_roundtrip`, "security descriptor not
  restored: Access is denied") is pre-existing and environmental: the
  UI-01A diff contains **no Rust changes**, so this result is identical
  on the baseline commit. Needs an elevated/dev-environment run to
  investigate; out of scope here.

## Screenshots (real binary)

- `testing/native/screenshots-ui01a/native/01-native-overview-scanning.png`
  — Overview during a live scan: `Scanning` hero, 4/9 layers reporting,
  `Collecting — Risk-scoring 115 persistence entries · elapsed 0.3s`,
  live activity timeline, metrics `In progress`, scan button disabled.
- `testing/native/screenshots-ui01a/native/02-native-rescue-results.png`
  — Rescue-origin scan landing on the findings view as before.
- `testing/native/screenshots-ui01a/native/03-native-overview-after-own-scan.png`
  — Overview after its own scan completed: `Partial coverage` hero,
  `PARTIAL` coverage metric, access-denied detail preserved, operator
  never moved.

## Remaining limitations

- The rescue scan has no backend cancellation command; the Overview
  therefore offers no stop control (per spec: only if genuinely
  supported).
- Scenario 9 (native failure injection) is verified on the mock shell
  only; the native backend cannot be made to fail without modifying it.
- The coverage summary list still shows "not assessed yet" rows while a
  scan runs (per-layer live states arrive at completion); the dial and
  collector line carry the live progress.
- `acl::tests::sddl_and_attributes_roundtrip` remains environmentally
  red (pre-existing; see above).
