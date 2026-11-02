# STAGE UI-03 — Rescue / System X-Ray Reconstruction Acceptance

**Branch:** `feat/cure-operator-storage-defense` · **Baseline:** `e1f69c4` (Stage UI-02B)
**Scope:** Rescue page only (System X-Ray radar, nine-layer telemetry, inline results console, scan lifecycle inside the Rescue workflow, backend coverage reporting). Overview, Agents, Quarantine, Detective, Storage and Monitor were not redesigned.
**Binary used for all native checks:** `gui/src-tauri/target/release/cure-gui.exe` — real Tauri + WebView2 + Rust backend, real collectors, no mocks.

---

## 1. Exact root causes of the bugs found

### Found in the inherited (half-finished) working tree

The stage arrived mid-implementation from a previous agent. An audit found the
work substantially coherent (radar engine, nine-layer matrix, inline results
console, backend coverage rows, mock contract, demo regeneration), with these
defects:

1. **`gui/devtools/ui03-native-verify.py` asserted strings the UI never
   produces.** It waited for `xray-live` values `Radar Standby`,
   `Radar scanning` and `Partial coverage`, and its reduced-motion check
   tested `data-running` (a scan attribute, not a motion attribute). The real
   values are `Radar standby`, `Collecting` and `Coverage incomplete`
   (`xray.js paintReadouts()`), so the harness stalled forever after its
   second screenshot — which is exactly why only 2 of the promised 7
   screenshots existed. **Fix:** the script was rewritten against the real
   state model (`CureSweep.getVerdict()` + rendered DOM), mapped to the 20
   scenarios of the stage brief, and honest about what it cannot trigger
   natively (backend failure paths, native save dialogs).

2. **Dead CSS left behind by the removed markup.** `.feed-card`,
   `.feed-head` and the `.feed-title` group entry in `style.css` styled the
   old Collection-log panel that UI-03 replaced. **Fix:** removed (both
   `dist` and regenerated `dist-demo`).

3. **A failed scan displayed the previous run's duration.**
   `renderRescueResult(null, err)` rendered `Duration` from `scanDurationMs`,
   which is only assigned on success — a failed run quoted the previous
   scan's elapsed time as its own. **Fix:** the catch path now stamps
   `scanDurationMs` from the failed attempt's real start time.

### Found and fixed during this stage's verification

4. **Dual ownership of the radar corner tag (`#xray-live`) and
   `xray-field[data-running]`.** `setPill()` (the topbar session pill) also
   wrote both, so on a fresh launch the radar read `Idle` instead of the
   radar's own `Radar standby`, a storage measurement or a process action
   could leave the radar claiming `Collecting` with no collection running,
   and the pre-scan overlay review could flash `data-running=true` during
   `preparing`. **Fix:** one writer per panel — the pill reports the session,
   `xray.js`'s lifecycle owns the radar (`app.js setPill`, `xray.js
   paintReadouts` gained an explicit `Preparing` state).

5. **The ELAPSED readout truncated after completion.** The completion text
   `finished in 17.3s · 114 entries checked` overflowed its narrow cell
   (`…114 entries …`), and duplicated the adjacent ENTRIES INSPECTED cell.
   **Fix:** the readout shows `finished in 15.6s`; the entry count lives in
   its own cell and in the results console.

### Backend / coverage (Phase 4 investigation)

6. **Root cause of the owner's reported "3/9 layers, partial coverage" scan:**
   only 4 of the 9 collectors produced a coverage row at all (Registry,
   Scheduled tasks, Services, WMI). Startup folders, IFEO/AppInit, COM
   registrations, Running processes and Ransom indicators emitted **no** row,
   so the UI's honest "unreported" logic could not count them as checked.
   Scheduled Tasks additionally collapsed the "directory walkable but every
   task file unreadable" case into a blanket **Access denied** because
   `TaskScanReport::status()` used `files_seen > skipped` — when *all* seen
   files were skipped the branch fell through to AccessDenied even though the
   directory itself had been enumerated. **Fix:** `run_auto_scan` now records
   a coverage row for every one of the nine areas (checked rows for
   Startup/IFEO+AppInit/COM with real entry counts, Running processes with
   the enumerated count, Ransom indicators with folder/finding counts and a
   truthful `not_checked` when no user folders exist), and
   `core/src/scanners/scheduled_tasks.rs` reports `Partial { skipped }`
   whenever task files were seen, so the skip count is preserved. No
   elevation, permission change or security bypass was introduced. On this
   machine a standard-user scan now honestly reports **8/9 layers covered,
   Scheduled tasks Access denied** (see §5).

7. **The earlier `Access denied` claim for Scheduled Tasks is accurate for a
   standard user** — `C:\Windows\System32\Tasks` is listable but task XML
   bodies are ACL-restricted. The collector says so, the UI says so, and the
   verdict refuses an all-clear (§5, screenshot 04).

---

## 2. Files changed

**Backend**
- `core/src/scanners/scheduled_tasks.rs` — Partial status preserves the skip count when task files were seen but unreadable.
- `gui/src-tauri/src/main.rs` — coverage rows for all nine persistence areas (Startup folders, IFEO and AppInit, COM registrations, Running processes, Ransom indicators) emitted both as `scan-progress source-state` events and in `ScanSummary.source_states`; non-Windows stubs kept honest (`Unavailable`); `cargo fmt` applied.

**Frontend (`gui/dist`, mirrored byte-identical into `gui/dist-demo` by `tools/make-demo-frontend.py`)**
- `index.html` — Rescue view rebuilt: header copy + Response badge + compact Tools menu (`details.xr-tools` with Export case file / Defender file scan "audit only · never removes"), scanner workspace (`xr-shell`) with the radar column and the primary controls inside it, telemetry aside (HUD + nine-layer Inspection matrix with expandable "Why" rows), readout strip, inline Assessment console (`#rescue-results`), collapsible Collection log; results view label `SAFE SCORE` → `ENTRIES SCORED SAFE`; companion mount removed from the results view.
- `xray.js` — nine-layer state machine driven only by backend events (`scan-progress`) and reconciled against `summary.source_states` at completion; outcome vocabulary `pending/running/checked/partial/denied/failed/unsupported/unreported`; radar states idle/preparing/scanning/layer-completed/denied/failed/complete/partial; findings beacons, shockwaves, hover/click layer callouts, reduced-motion + pause control; `Preparing` live tag added this stage.
- `app.js` — inline results console (`renderRescueResult`: assessment title, four real stats, backend-worded coverage-gap notes, compact ranked findings or the scope-honest empty state), phase-driven primary controls (`setScanControls`), response badge rendered from the live operator posture, stale-result clearing before every run, failed-scan duration fix, elapsed readout fix, `setPill` ownership fix.
- `xray.css` — full Rescue restyle (workspace grid, matrix rows per outcome, tools menu, results console, light-theme palette).
- `mock-tauri.js` — mock emits the same nine coverage rows so the labelled demo/dev harness exercises the real contract.
- `style.css` — dead `.feed-card`/`.feed-head`/`.feed-title` removed.

**Tooling / tests**
- `gui/devtools/ui03-native-verify.py` — rewritten (§1.1).
- `gui/devtools/evidence-console.py` — rescue-flow selectors updated for the new workspace; recorder scrub interaction made atomic (§4.3).
- `gui/devtools/ui01a-native-verify.py` — scenario 4 updated to the UI-03 contract (completion stays in Rescue; Investigate reached via the explicit "Open in Investigate" action) and its wait now keyed to the real completion signal.
- `tools/make-demo-frontend.py` — demo banner also injected into the Rescue view (it now reports results inline).

**Evidence**
- `testing/native/screenshots-ui03/native/*` + `docs/screenshots/ui03/*` — nine native captures (below).
- `testing/native/screenshots-ui01a/native/*`, `testing/native/screenshots-ui02a/native/*` — regenerated by the green regression runs.

---

## 3. Visual improvements

- The scanner is one centred instrument: circular holographic radar (canvas) with outcome-coloured orbital tracks, an activity sweep only while the backend reports, travelling bead on the active collector, evidence diamonds for flagged entries, and a hub readout that states `8 / 9 PARTIAL`, `INCOMPLETE · NO FINDINGS`, findings count — never a health score.
- Primary control lives inside the scanner workspace: `Start System Scan` → `Scanning…` (disabled) → `Scan Again`; `Review findings (N)` appears only when findings exist; `Retry scan` only after failure. Secondary actions moved into a compact Tools menu.
- The telemetry panel is a nine-row matrix with one outcome word per collector, real inspected counts, amber `Access denied`, expandable per-layer reasons, and a footer that states the counting rule.
- Results are an inline Assessment console under the radar (title, 4 stats, backend-worded gap notes, ranked findings or the honest empty state) instead of a forced view change; the oversized storage recommendation never appears here.
- Dark and light themes both restyle the canvas palette; no mascot or companion widget anywhere in the flow (asserted in DOM).

Before/after: the inherited idle state is captured in git history
(`01-idle-scanner.png` at the previous agent's run vs. the current set in
`docs/screenshots/ui03/`); the structural "before" is the pre-stage layout in
`git show e1f69c4:gui/dist/index.html`.

---

## 4. Verification results

### 4.1 Native acceptance (`gui/devtools/ui03-native-verify.py`, real binary) — **23/23 PASS**

Real scan evidence from the definitive run (standard user, this machine):
**115 entries collected, 8/9 layers covered, 15.6 s, 0 findings for review,
Scheduled tasks = Access denied.** All four coverage readouts (matrix header,
readout strip, HUD, results stat) showed `8 / 9`; the radar hub showed
`8 / 9 PARTIAL`; the verdict read `No findings observed · coverage
incomplete` with the explicit "not evidence that the machine is clean"
wording. Mid-scan navigation to Overview and back was respected; the scan
started exactly once; screenshots captured per scenario.

The 20 brief scenarios map to the checks printed by the harness (header of
`ui03-native-verify.py`); the two that cannot be triggered natively are
stated there: backend-failure reporting is covered by the mock frontend suite
(truthful-failure run asserts no stale results are presented), and the
evidence export dialog flow is covered by the deterministic mock suite — the
native check verifies the Tools entry and the Defender modal's audit-only
default (`Run Read-Only Scan (-DisableRemediation)`, containment gated behind
a typed confirmation token).

### 4.2 Rust gates

| Gate | Result |
| --- | --- |
| `cargo fmt --check` (root + gui crate) | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| GUI crate clippy + tests | PASS (14 tests; + overlay desktop fixture 1 PASS run locally) |
| `cargo test --workspace` | **599 passed, 0 failed** — includes `qa_process_safety` 3/3 and `acl::tests::sddl_and_attributes_roundtrip` **PASS this session** (historically environment-red needing elevation; green here) |
| Ignored by design | `live_probes` (3, manual host probes), GUI overlay fixture (run manually above) |

### 4.3 Frontend + regression suites

| Suite | Result |
| --- | --- |
| `evidence-console.py verify` (mock frontend, includes the truthful-failure run, mascot absence, accessibility/axe) | **589 checks PASS** |
| `ui01a-native-verify.py` (scan-origin ownership, Overview live updates) | **14/14 PASS** |
| `ui02a-native-verify.py` (Agent Radar, Flight Recorder, ownership-gated process control) | **26/26 PASS** |

Two harness flakes were root-caused and fixed rather than papered over:
the recorder scrub `fill()` race (Playwright splits value-set and event
dispatch; a recorder status render between them read back the reset slider —
now one atomic task; assertions unchanged), and the UI-01A wait that resolved
against the previous scan's inline results (now keyed to the real completion
signal — this reproduced the "third scan" illusion and the mid-scan
"Scanning" reading; the product was correct in both cases).

---

## 5. Real collector coverage (native run, standard user)

| Layer | Outcome | Detail |
| --- | --- | --- |
| Registry autoruns | Checked | 10 values |
| Startup folders | Checked | 5 entries |
| Scheduled tasks | **Access denied** | task directory unreadable (elevation may help) |
| Auto-start services | Checked | 94 services |
| WMI subscriptions | Checked | 3 entries |
| IFEO and AppInit | Checked | 0 entries |
| COM registrations | Checked | 2 entries |
| Running processes | Checked | enumerated live |
| Ransom indicators | Checked | user folders scanned, 0 findings |

Zero findings + one denied layer ⇒ verdict `No findings observed · coverage
incomplete`, empty state "No findings were observed within the inspected
scope. At least one area could not be read, so this is not evidence that the
machine is clean." — no all-clear is ever implied.

---

## 6. Screenshots (native, `docs/screenshots/ui03/`)

| File | Scenario |
| --- | --- |
| `01-idle-scanner.png` | Honest idle state, `Start System Scan` inside the workspace |
| `02-active-scanning.png` | Sweep active, 6/9 layers, per-layer callout, denied already amber |
| `03-midscan-overview.png` | Mid-scan manual navigation respected; Overview reports the live operation |
| `04-completed-scanner.png` | Completed radar `8 / 9 PARTIAL`, consistent readouts |
| `05-results-console.png` / `05b-results-console-detail.png` | Inline Assessment console |
| `06-light-theme.png` | Light theme |
| `07-1366x768.png` | 1366×768, no overflow/overlap/clipping |
| `08-final-completed-1600.png` | Final 1600×900 completed state |

All captures were reviewed by the agent; the two visible defects found in
review (truncated ELAPSED readout, tools menu left open in the 1366 shot)
were fixed and re-captured.

---

## 7. Remaining limitations (honest)

1. **Scheduled Tasks needs elevation to read task XML bodies.** The scan
   stays read-only and unprivileged; the layer reports Access denied with the
   reason. No elevation path was added (out of scope by design).
2. **The Investigate coverage strip duplicates the denied reason text**
   (`ACCESS DENIED — access denied (elevation may help) · task directory
   unreadable (elevation may help)`). Cosmetic, pre-existing on the
   Investigate page, which this stage must not redesign.
3. **Startup folders / IFEO / COM report `Checked` with a count** — those
   collectors return plain vectors and cannot distinguish "empty" from
   "unreadable" on exotic ACLs. Practical risk is low (user-readable
   locations), noted for a future collector-report hardening.
4. **`live_probes` (3) remain `#[ignore]`** — they are manual host probes by
   design; the GUI overlay fixture requires an interactive desktop and was
   run manually here.
5. **A second simultaneous scan is refused by design** (no cancellation
   support in the backend yet), so no Stop/Cancel control is shown while
   scanning — the button is disabled and the phase readouts carry the state.

---

## 8. Delivery

- **Executable:** `D:\Projects\CURE\gui\src-tauri\target\release\cure-gui.exe`
- **Branch:** `feat/cure-operator-storage-defense`
- **Baseline commit:** `e1f69c4`
- **Delivery commit:** this commit (Stage UI-03) — see `git log -1` on the branch.
- Not pushed, not merged, not installed, not published.
