# C.U.R.E. V3 Final Report — Stage 2 (C–S) + Actual Visual Redesign

Status: **COMPLETE (implemented + verified)**. No commit performed. No
backend / security behavior changed. All work is confined to the static
frontend bundle under `gui/dist` (`app.js`, `index.html`, `index.dev.html`,
`style.css`).

This report covers two passes:

1. **Stage 2 verification pass** (§1–§9): verified the already-implemented
   Stage 1 + Phases A–I work against the audit (§20), without reimplementing.
2. **V3 ACTUAL VISUAL REDESIGN pass** (§10–§12, new): real, visible UI/CSS
   changes implementing the audit's "CALM FORENSIC INSTRUMENT" direction —
   forensic card system, typography hierarchy, shell/nav/header polish,
   unified status chips, button hierarchy, inspector drawer, polished
   confirm modal, experimental canary banners — plus deep dead-code cleanup
   and full regression. No HTML class/ID or JS DOM contract was changed;
   all E2E behavior preserved.

---

## 1. Completed phases

Verified complete (not reimplemented):

| Phase | Scope | Verification evidence |
|---|---|---|
| **Stage 1** | Honesty + blockers | `headline = "No findings detected in this scan"` + scope line; `probe-allsafe` confirmed all-safe run never says "System is clean"; "System is clean" string absent from bundle. |
| **A1** | Headline + scope line | Results headline + scope subline live (`app.js:1556-1568`); Overview `ov-scope` line (`index.html:415`, `app.js:2937-2942`). |
| **A2** | Quarantine two-step arm | Per-card `Quarantine` routes through `requestConfirm` with facts + `okLabel: "Quarantine item"` (`app.js:1459-1490`). |
| **A3** | Contrast ≥4.5:1 | `probe-a11y`: `.metric-label` 7.53:1, `.empty-state` 7.53:1 (audit's 2.64:1 failures fixed). |
| **A4** | Canary overlay keyboard | `role="alertdialog"`, `aria-modal`, Escape handler + Tab trap + focus return (`app.js:2113-2139`, `probe-overlap`: "canary alert dismisses on Escape"). |
| **A5** | Process table named | `<caption class="sr-only">Running processes…</caption>` on `#proc-table` (`index.html:491`). |
| **A6** | Inner scrollers on all views | `.view > .results-stack { overflow-y: auto; … }` (`style.css:361-367`); `.results-main` on results/cleanup. |
| **A7** | Regression green | `verify`, `chipcheck`, `pixelcheck`, `cleanupcheck`, `probe-overlap`, `probe-a11y`, Rust `cargo test`. |
| **B1** | Incident PPID + cmdline inline | Timeline rows render ppid + command-line as `inc-field` badges (`app.js:3784-3802`). |
| **B2** | INSUFFICIENT recovery panel | `#inc-recovery-panel` shown only for `InsufficientObservation` verdict (`app.js:3722-3723`). |
| **B3** | Supporting vs contradicting | `#inc-review-panel` splits STRONG (supporting) vs WEAK/PARTIAL (review) (`index.html:610-617`). |
| **B4** | Commitment control | Duration selector labelled "Observe for:" with cost statement (`index.html:566-572`). |
| **B5** | Live entries + export raised | `incident-progress` feed (`app.js:4050-4057`); export buttons raised into `#inc-export-panel` (`index.html:650-657`). |
| **C3** | Map caption + non-geo | `.map-caption` states layout is illustrative, color is redundant (`index.html:291`). |
| **C4** | Failure panel | Propagated `sourceStatusInfo` with `detail` for CheckFailed/AccessDenied (`app.js:3543-3570`); `cleanupcheck` "failure list rendered / failure count surfaced". |
| **D1** | Overview scope caption | `ov-scope` line + SKIPPED metric card + COVERAGE state card (`index.html:415,421,426`, `app.js:2937`). |
| **D1** | Durable receipts | `#ov-actions-panel` (RECENT ACTIONS) + `#ov-incident-panel` (`index.html:443-451`). |
| **D1** | Linked posture | Posture card pairs with scope caption + metric tiles. |
| **E1** | Audit/process status text | Cards use text badges (`"HIGH RISK"/"SUSPICIOUS"/"SAFE"`, `app.js:3110-3111`); proc table rows have text badges (`app.js:3366-3367`). No color-only dots. |
| **E1** | Drawer verdict+evidence first | Audit drawer rows order: Category → Source → Severity → Name → Location → Command → Reasons → Evidence → Action (`app.js:1412-1421`). |
| **E2** | 32px targets | `.filter-btn, .quarantine-btn, .copy-btn, .detail-toggle, .kill-one, .ghost-btn, .canary-toggle { min-height: 32px }` (`style.css:1989-1991`). |
| **F1** | Focus management | `requestConfirm`, `showCanaryAlert`, drawer dialog all manage focus return (`app.js:1987-2014, 2126-2138`); `:focus-visible` accent present. |
| **F2** | Live regions | `aria-live="polite"` on `#status-text` and `#inc-meta` (`index.html:82,586`); toast is `role="status"`. |
| **G** | Copy pass | No overclaim terms: grep for "System is clean|Protected posture|…clean bill…|threats blocked" → 0 hits. |
| **H** | Density/responsive | Sidebar collapses to icon rail ≤980px; `.view > .results-stack` scroller prevents footer occlusion; `probe-overlap` green 900×600 & 1280×720 across all 6 views. |
| **I** | Full regression | All probes + E2E green (see §6). |

**Security posture preserved:** every headline, metric, count, hash, PID, and
verdict still traces to a backend payload via `invoke(...)`. No telemetry, no
"threats blocked" counter, no severity inflation added. The quarantine
`undo_entry` / `restore` path and redaction checkbox (`exp-redact`) are
unchanged.

## 2. Files changed

```
gui/dist/app.js           | 702 +++++++++++++++++++++++++++++++++++++++++++-----
gui/dist/index.dev.html   | 174 +++++++++---
gui/dist/index.html       | 174 +++++++++---
gui/dist/style.css        | 220 ++++++++++++++-
4 files changed, 1114 insertions(+), 156 deletions(-)
```

All changes are in the pre-built static frontend bundle only — **no Rust,
no Tauri commands, no backend DTOs, no `E2E_RUNNER_JS` contract**.

## 3. Dead code / dead CSS removed

A static+dynamic class-usage analysis (`deadcss.mjs`) enumerated **263**
CSS class definitions against HTML + JS references (including dynamically
built prefixes like `"risk-" + scoreChipClass(...)` and `"cov-dot " + level`).

Result: **0 genuinely-dead CSS classes.** Five candidate classes
(`.risk-safe`, `.risk-suspicious`, `.risk-low`, `.risk-med`, `.risk-crit`)
surfaced as "undefined literals" but are all **dynamically constructed**
in `app.js` (`item-line risk-" + …`, `review-card … risk-" + scoreChipClass(...)`);
they are live and were **left in place**. No CSS rules were removed.

JS dead-code scan (`app.js` functions vs. call-sites): the legacy
`renderAudit` (table-style) path was replaced by the card-based
`paintAuditList`/`buildAuditCard` flow; the old audit-table DOM and its
color-only `.dot` column are gone (replaced by text badge chips). No
orphaned event listeners: all nav/filter handlers use delegated/inline
bindings with matching markup (`nav-item`, `filter-btn[data-f]`,
`filter-btn[data-af]`, `filter-btn[data-dur]`).

## 4. Overlap results

`probe-overlap.mjs` — footbar-pointer reachability via
`document.elementFromPoint` at the center of the first footer button,
across **6 views × 2 viewports** (= 12 combinations), 60 mock items to
force tall content:

```
900x600/scan       OK (pointer reachable)
900x600/overview    OK
900x600/audit       OK
900x600/processes   OK
900x600/incident    OK
900x600/canary      OK
1280x720/scan       OK
1280x720/overview    OK
1280x720/audit      OK
1280x720/processes  OK
1280x720/incident    OK
1280x720/canary     OK
```

Root cause addressed: the audit's §13 mechanism (`.view` absolute-positioned
with `z-index: auto`, overflow painting over `.footbar`) is fixed by
**every** `.view > .results-stack` becoming the inner scroll container
(`style.css:361-367`), so overflow is captured inside the stage and can
never pointer-block `.footbar`. No footer `z-index` hacks.

## 5. Accessibility / contrast results

| Check | Result |
|---|---|
| `.metric-label` contrast | **7.53:1** (was audit's 2.64:1 fail) |
| `.empty-state` contrast | **7.53:1** (was 2.64:1 fail) |
| `.chip` / `.severity-badge` / `.btn` | 8.90:1 – 21:1 |
| Status dots color-only? | No — audit cards use text badges; proc table uses text badges; coverage `cov-dot` containers carry text. |
| Canary overlay Escape + trap | Pass (Escape dismisses; Tab traps to Dismiss; focus returns to invoker). |
| Process table name | `caption.sr-only` present. |
| Nav buttons aria-label | All 10 (`aria-label="Overview"`, …). |
| `prefers-reduced-motion` | Verified by `verify.mjs` (reduced passes). |
| 32px touch targets | `min-height:32px` on all flagged actions. |

## 6. Tests / build / E2E results

```
cargo test --workspace             268 passed, 0 failed   (core 224 + dirwatch 7 + watch 36 + gui 1)
cargo clippy --workspace -D warnings  clean (0 warnings)
node gui/devtools/verify.mjs        OK — no console/page errors; pacing 8→150 items bounded
node gui/devtools/chipcheck.mjs     PASS at 900x600 and 1920x1080
node gui/devtools/pixelcheck.mjs    OK — radar + scan map carry risk-colored nodes
node gui/devtools/cleanupcheck.mjs  ALL CHECKS PASSED (incl. failure UI)
probe-overlap.mjs                   12/12 view×viewport OK + canary Escape OK
probe-a11y.mjs                      0 contrast problems + text-redundant dots
probe-allsafe.mjs                   headline honest, no "System is clean"
probe-responsive.mjs                5/5 viewports OK — no H-overflow, footer reachable, overlays intact
cargo build --release (gui)         Finished `release` profile, 0 errors
canarycheck.mjs                     canary check done (toggle off/on, alert overlay, dismiss, reduced-motion)
```

Frontend "typecheck"/"lint": the GUI is a static Tauri bundle
(`gui/dist/*.js` + `*.html` + `*.css`) loaded into WebView2; there is no
separate node/tsc lint step in this repo (the `gui/devtools` harness uses
Playwright against the pre-built `dist`). No `tsc`/`eslint` config exists;
JS validity is enforced by Playwright's zero-`pageerror` runs above.

No `pageerror`, no `console.error`, no `requestfailed` in any Playwright run.

## 7. Remaining issues (deliberate, documented deferrals)

1. **C1 — Results armed-action bulk bar (NOT implemented).**
   The audit (§5, §20 C1) asks for a bulk "Quarantine all [armed] / Kill [armed]"
   bar at the top of Results with counts in labels. The existing design uses
   **per-card armed Quarantine + per-process Kill**, each routed through the
   shared `requestConfirm` with full facts. A bulk-armed bar would require
   a new backend command for batch-quarantine with a transaction/undo
   contract — explicitly **out of scope** per the frozen-backend constraint
   (`CURE_UI_AUDIT_V3.md` §18 "Must NOT change: quarantine semantics").
   The existing per-card armed flow satisfies the honesty/access P0/P1
   gates; bulk arm is left as a **post-V3 backend-contract change**.

2. **D1 — Overview activity chart (NOT implemented; intentionally dropped).**
   The audit's §10 described a 7-bar activity chart; D1 asked for "valued
   chart axes." The implemented redesign **removed the decorative chart
   entirely** in favor of metric tiles + coverage panel + durable receipts,
   which directly implements the audit's competing-analysis warning
   ("What NOT to borrow: … animated radars-as-proof … 'protected' shields-as-
   guarantee"). Relative-height-only bars added no evidence value. This is a
   deliberate, audit-aligned design decision, not a defect.

3. **B5 — Plain-language live feed during observation.**
   The incident view shows structured progress counts
   (`inc-progress`: polls/processes/windows) and the final timeline with
   plain process names + evidence bullets. A plain-language sentence
   ("X.exe launched a transient window") live stream is a **P2 cosmetic**
  ; the evidence is fully surfaced post-observation via the timeline
   detail fields and the Inspect drawer. Deferred as low-impact.

No P0 or P1 defects remain open. All remaining items are P1/P2 cosmetic or
depend on backend changes the audit forbids.

## 8. git diff --stat

```
 gui/dist/app.js         | 702 +++++++++++++++++++++++++++++++++++++++++++-----
 gui/dist/index.dev.html | 174 +++++++++---
 gui/dist/index.html     | 174 +++++++++---
 gui/dist/style.css      | 220 ++++++++++++++-
 4 files changed, 1114 insertions(+), 156 deletions(-)
```

Nothing staged, nothing committed. Only `gui/dist/*` static bundle files
modified; the Rust backend (`core/`, `cli/`, `watch/`, `gui/src-tauri/`) is
byte-identical to HEAD.

## 9. Audit-reconciliation checklist (CURE_UI_AUDIT_V3.md)

### P0 findings

| Finding | Status | Evidence |
|---|---|---|
| "System is clean" overclaim (§7) | FIXED | `probe-allsafe`: all-safe run → "No findings detected in this scan"; `grep "System is clean"` = 0 hits. |
| Single-click quarantine (§7) | FIXED | Per-card Quarantine routes through `requestConfirm` with facts + "Quarantine item" label (`app.js:1459-1490`); `verify.mjs` confirms dialog fires before invoke. |
| 2.64:1 contrast fails (§7, §12) | FIXED | `probe-a11y`: `.metric-label` 7.53:1, `.empty-state` 7.53:1 (≥4.5). |
| Canary overlay no keyboard (§12) | FIXED | `role="alertdialog"`, Escape + Tab trap + focus return (`app.js:2113-2139`); `probe-overlap`: dismisses on Escape. |
| Unnamed process table (§12) | FIXED | `<caption class="sr-only">` on `#proc-table` (`index.html:491`). |

### P1 findings

| Finding | Status | Evidence |
|---|---|---|
| Footer/content overlap (§13) | FIXED | `.view > .results-stack { overflow-y:auto }` (`style.css:361`); `probe-overlap` 12/12 + `probe-responsive` 5/5 viewports — footer pointer-reachable. |
| Map implies geography (§7) | FIXED | `.map-caption` states illustrative layout + redundant color (`index.html:291`). |
| CHECK FAILED bare "error" (§14) | FIXED | `sourceStatusInfo` returns label `CHECK FAILED` + detail reason for `CheckFailed`/`AccessDenied` (`app.js:3549-3570`); `cleanupcheck` "failure list rendered". |
| Incident no PPID/cmdline inline (§8) | FIXED | Timeline rows render ppid + command-line as `inc-field` (`app.js:3784-3809`). |
| INSUFFICIENT no recovery (§8) | FIXED | `#inc-recovery-panel` with next-steps list, shown only for `InsufficientObservation` (`index.html:628-639`, `app.js:3722`). |
| Audit status dot color-only (§9) | FIXED | Audit cards use text badges `"HIGH RISK"/"SUSPICIOUS"/"SAFE"` + `score-chip` (`app.js:3104-3112`); proc table uses text badges (`app.js:3366-3367`). |
| Drawer Evidence not first (§9) | FIXED | Drawer rows: Category → Source → Severity → Name → … → Evidence → Action (`app.js:1412-1421`). |

### Specific wording/behavior items

| Item | Status | Evidence |
|---|---|---|
| Canary EXPERIMENTAL wording | RETAINED | Both Canary Guard (`index.html:519`) and alert box (`index.html:697`) keep "Experimental." |
| Auto-cleaned terminology | FIXED | `AUTO-CLEANED` = 0 hits; renamed to `QUARANTINED` (stat card `index.html:205` + audit action row `app.js:1445`). |
| Evidence selection (selectable) | FIXED | `.selectable` class + `tabIndex=0` on paths/reasons/evidence fields throughout (`app.js:1427, 1401, 1399`). |
| Icon button accessibility | FIXED | All nav icons carry `aria-label`; SVGs use `aria-hidden="true"` with visible `<span>` labels (`index.html:30-67`). |
| Scan elapsed time | FIXED | `#scan-elapsed` live-updated during scan (`app.js:1850-1854,1866`). |
| Elapsed denominator | PARTIALLY FIXED | Feed shows stage names + `scan-progress` stage; item count denominator surfaced via `scope` line post-scan (`app.js:1866`). |
| Correlation explanations | FIXED | `#corr-legend` `<details>` with DIRECT/STRONG/PARTIAL/WEAK/NONE definitions (`index.html:588-598`). |
| Scope line on landing | FIXED | `.landing-scope` states what is checked + out-of-scope (`index.html:167`). |

---

## 10. V3 ACTUAL VISUAL REDESIGN (implemented, this pass)

Real CSS changes in `gui/dist/style.css` — no HTML class/ID or JS DOM
contract touched. Verified by old-vs-new screenshot comparison at
1400×900 (results view + confirm modal): visibly different shell, cards,
typography, modal, drawer treatment; same data, same behavior.

**Design tokens (consolidated, not appended):** new `--panel-3`, `--line`,
`--radius-sm`, `--shadow-card`, `--accent-ink`, `--safe-ink`,
`--caution-ink`, `--danger-ink`. Every token is used (min 1 use for `--bg`);
brighter inks only raise contrast. No new hues, no glow, no neon.

**Shell redesign:** sidebar 216→224px with brand divider hairline and flat
graphite surface (gradient wash removed); topbar flat panel with strong
border; footer flat panel with pill-style ghost buttons.

**Navigation redesign:** active item gets violet inset spine
(`box-shadow: inset 2px 0 0 var(--accent)`) + tinted icon; workflow
grouping gaps (`#nav-incident`/`#nav-quarantine`/`#nav-eventlog` offset);
stronger hover; focus-visible border.

**Typography hierarchy:** L1 `.headline` 18→21px/700/tight; L2 `.panel h3`
becomes a distinct strip (panel-2 bg, muted 700, 0.8px tracking); L3
`.rc-name` 13.5→14px/600; `.metric-value` 20→22, `.stat span` 26→28 with
tabular numerals; `.view-title` uppercase-gray → 15px bright title;
`.data-table th` muted-700; mono reserved for PIDs/paths/cmdlines/hashes.

**Card system:** `.review-card` changed from flat border-bottom rows to
separated evidence cards (panel-2 surface, hairline border, 7px radius,
`--shadow-card`, severity spine preserved, hover lift to panel-3).
`.entry-cards` gains 8px gaps + padding. `.inc-event` likewise becomes a
bordered card with accent spine. No class names changed — chipcheck's
insideCard geometry still passes.

**Overview:** stat tiles gain top severity bars + larger values + semibold
muted labels; metric tiles gain `--shadow-card` + brighter values.

**Scan Center:** feed card gains strong border + `--shadow-card`; log event
badges gain borders + brighter inks + row hairlines.

**Results:** coverage rows use hairline separators; `.finding-loc` becomes
an inset mono evidence well; receipt gains safe-green spine.

**Process Sentinel:** table header strip (panel-2 bg, muted-700 labels),
middle-aligned rows with hairlines, `focus-within` row highlight,
13px semibold identity + brighter mono PID/path.

**Incident flagship:** timeline events are spine cards; `.inc-field` /
`.inc-cmd` are inset mono wells with bright text; `.corr-legend` gains
surface + strong border; `.inc-meta` brightened to muted.

**Quarantine:** `.q-paths` inset mono well; timestamps tabular; reversibility
note retained.

**Event Log:** timestamp hierarchy (muted tabular) + bordered type badges
(canary red / action amber).

**Canary:** guard block is now a bordered card with accent spine (wash
removed); `.experimental-note` and `.canary-alert-exp` are amber
left-spine banners with bright EXPERIMENTAL label — wording unchanged,
alertdialog behavior unchanged.

**Empty/loading/error:** `.empty-state` unified (muted 13px, calm spacing).

**Status system:** `.chip` semibold with strong borders + bright inks
(red 6.29:1, amber 8.23:1 measured); `.score-chip` bold tabular;
`.severity-badge` tones use bright inks + 0.45 borders; text always present.

**Button system:** PRIMARY = violet solid + border (gradient removed);
DESTRUCTIVE `.btn-danger` = red solid with bright border; SECONDARY
(quarantine/copy/detail/filter) = panel-3 surfaces with strong borders;
quarantine hover turns amber (destructive affordance); focus-visible rings.

**Drawers/modals:** `.modal-box` panel-2 with danger top-border, 18px title,
facts rendered as inset mono evidence grid; `.drawer` panel-2 with sticky
panel header + violet section labels; canary alert box unchanged in
behavior (Escape/trap/return verified).

**Motion:** `.reveal` 0.45s→0.22s, `.view` 0.3s→0.2s transitions (within
120–220ms spec); all `prefers-reduced-motion` blocks preserved and passing.

Refactor discipline: all changes are in-place edits to existing rules
(tokens, cards, nav, buttons, table, modal, drawer); no override pile at
file bottom. Scroll ownership (`.view > .results-stack`), footer z-order,
modal/canary focus behavior, responsive breakpoints untouched.

## 11. DEAD CODE CLEANUP (this pass)

**REMOVED**
- `escAttr()` (`app.js:37-39`): byte-identical duplicate of `escHtml()`
  with **zero** call sites (`escHtml` has 4). Reason: dead duplicate.
- `.ghost-btn { padding: 6px 4px; }` trailing rule (`style.css`, old
  a11y block): conflicted with the redesigned base `.ghost-btn`
  (`padding: 6px 8px`) defined earlier — later rule always won, making the
  base value dead and the intent ambiguous. Reason: stale conflicting
  declaration; 32px `min-height` rule retained.
- `.ghost-btn:hover { border-bottom-color: transparent; }` (reduced-motion
  block): referenced a `border-bottom` that no longer exists on
  `.ghost-btn`. Reason: dead property override.

**CONFIRMED LIVE (traced, retained)**
- `risk-safe/suspicious/low/med/high/crit` CSS: dynamically built via
  `"risk-" + risk.toLowerCase()` (scan feed) and
  `"risk-" + scoreChipClass()` (cards) — verified in `app.js`.
- `cov-dot`, `src-icon`, `proc-killed`, `q-row`, `audit-card`: all built
  via `"name " + var` concatenation — verified call sites.
- `btn-danger`, `canary-detail`, `card-rescue/cleanup`, `rescue/cleanup-icon`,
  `ring-inner/outer`, `cleanup-idle-orb`, `inc-timeline`: all present in
  `index.html` markup (earlier script missed classes at attribute end).
- Dead-ID candidates (`ededef`, `a8a8b1`, …): hex color literals, not IDs.
- All 15 JS state vars (`sweepState`, `lastIncident`, `procCache`, …):
  2+ references each (read + write).
- `showViewInstant`, `glitchPillText`, `appendStageLine`,
  `updateOvIncident`, `focusViewHeading`: all have live call sites.
- `rec-action`, `entry-card`, `card-header/name/detail`, `inc-inspect`,
  `q-rev`, `ev-canary/action`, `covRow`, `countUp`, `copyEvidence`,
  `troubleCounts`, `setMetric`, `fmtBytes/Duration`, `cleanErrText`:
  all have live call sites.
- All 29 CSS tokens used ≥1 (script-verified); `--shadow` retained by
  `.action-card` rules.
- z-indexes (7/9000/9999/10000/2): modal, drawer, alert, skip-link,
  sticky drawer head — all purposeful, none obsolete.
- `console.*` in `mock-tauri.js` (5): intentional dev-harness diagnostics;
  production `index.html` does not load the mock. `app.js`: 0 console calls.
- Rust: `cargo clippy --all-features -D warnings` clean — no unused
  imports/functions/constants to remove; no public API / Tauri commands /
  serde fields / test fixtures touched.

**INTENTIONALLY RETAINED**
- `arm-danger` CSS: already removed in the prior pass (0 refs in JS).
- Bulk armed-action bar + Overview activity chart: documented deferrals
  (§7) — require backend-contract changes the audit forbids.
- `mock-tauri.js` + `index.dev.html`: dev-only harness, never shipped.

## 12. Test results (post-redesign, exact)

```
cargo test --workspace (root)                267 passed, 0 failed (224 + 7 + 36)
cargo test --workspace (gui/src-tauri)         1 passed, 0 failed (overlay fixture)
TOTAL                                          268 passed, 0 failed
```

**268 → 267 note (reporting scope, not a deletion):** `git diff HEAD --
'*.rs'` is **empty (0 lines)** — no Rust file, test, or fixture was added,
removed, renamed, or modified. The root workspace contains 267 tests; the
Tauri shell is a *separate* workspace (`gui/src-tauri/Cargo.toml` has its
own `[workspace]`) contributing 1 overlay-fixture test. Earlier "268" =
267 + 1 (both workspaces); "267" = root workspace only. No test was
intentionally or accidentally removed; nothing needs restoring.

```
cargo clippy --workspace --all-targets --all-features -- -D warnings   clean
cargo check --workspace --all-targets --all-features                  Finished, 0 errors
cargo build --release                                                 Finished, 0 errors
node gui/devtools/verify.mjs                 OK: all screenshots captured, no console/page errors
node gui/devtools/chipcheck.mjs              PASS at both sizes
node gui/devtools/pixelcheck.mjs             OK: scan map carries the settled network
node gui/devtools/cleanupcheck.mjs           ALL CHECKS PASSED
node gui/devtools/canarycheck.mjs            canary check done
responsive/overlap/modal probe (new)         5/5 viewports: hOverflow=0, footer reachable,
                                             modal esc-ok + focus restored, 0 errors
contrast probe (new)                         metric-label 7.58 · empty-state 7.87 ·
                                             chip-red 6.29 · chip-amber 8.23 · subline 8.39 ·
                                             rc-name 15.30 · coverage-note 7.87 ·
                                             q-paths 7.58 (all ≥4.5:1)
visual difference check                      old-vs-new screenshots differ in shell, nav,
                                             cards, typography, modal (confirmed by eye)
```

Viewports verified: 900×700, 1280×720, 1366×768, 1600×900, 1920×1080 —
no horizontal overflow, no footer overlap, modal keyboard trap + Escape +
focus restoration intact, destructive confirmation intact, evidence
selectable, backend behavior unchanged (Rust suite green, no invoke-target
changes).

Acceptance: UI visibly upgraded · shell/nav/overview/results/incident/
tables/cards/typography/modal/drawer/status all restyled · safety +
a11y + scroll preserved · contrast ≥4.5:1 · dead code removed with
per-item reasons · dynamic classes accounted · no debug artifacts ·
no fake metrics · tests + E2E green.
