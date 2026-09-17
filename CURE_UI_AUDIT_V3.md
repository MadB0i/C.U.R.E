# C.U.R.E. UI Audit V3 — Forensic UX Audit (Audit Only, No Implementation)

Status: AUDIT ONLY. Nothing in `gui/dist` (`app.js`, `index.html`,
`style.css`, `mock-tauri.js`), Rust code, tests, or docs was modified to
produce this document. All observations below were verified against the
live interface (Playwright + mock backend at `gui/dist/index.dev.html`,
inkl. forced PARTIAL/CHECK-FAILED states via a `__TAURI__` trap) at
commit `52ab905` + uncommitted V2.2/V2.3 work, viewports 900×700 →
1920×1080, with and without reduced motion.

Companion: `CURE_UI_AUDIT_SUMMARY.md` (top-10s, principles, order).

---

## 1. Executive summary

The UI is a competent dark-forensic console that is honest in its data
model (every number observed traces to a backend response; no fabricated
metrics were found) but uneven in communication. The three findings that
matter most:

1. **The results screen can say "System is clean"** (`app.js:1464`,
   default headline in `index.html:191`) when the backend only
   established "no findings observed" — the single most important
   security-UX violation in the product. Overview's "Protected" is
   borderline-acceptable; "System is clean" is not.
2. **Quarantine is single-click while kill/cleanup are two-step.**
   The most destructive action in the app (file relocation) has the
   weakest confirmation of the three armed actions — an inconsistency
   with real misclick consequences.
3. **Two WCAG AA contrast failures** (`.metric-label`, `.empty-state` at
   2.64:1) plus zero keyboard dismissal for the canary alert overlay and
   an unlabeled process table exclude keyboard/screen-reader users from
   exactly the high-stakes flows.

Everything else is P1 and below. The information architecture is
fundamentally sound (workflow-ordered sidebar, gated nav, evidence-first
cards); the redesign should restructure hierarchy and language, not
invent new paradigms. Backend contracts are frozen and listed in §18.

---

## 2. Current UI inventory

| Layer | File | Size | Role |
|---|---|---|---|
| Structure | `gui/dist/index.html` | 606 lines | 11 views, sidebar, topbar, footbar |
| Logic | `gui/dist/app.js` | 3490 lines | Single IIFE; scan/cleanup/canary/incident flows, canvas radar + map |
| Style | `gui/dist/style.css` | 1720 lines | Token system + components; V2/V2.1/V2.2 appended sections |
| Test double | `gui/dist/mock-tauri.js` | 586 lines | 9 knobs (`ITEM_COUNT`, `SWEEP`, `ALL_SAFE`, `FOOTER_ERRORS`, `CLEANUP_FAILURES`, `CANARY_ALERT`, `OVERLAY_HITS`, …), clearly-labeled synthetic data |
| Dev entry | `gui/dist/index.dev.html` | = index.html + mock script tag + "· Mock" wordmark | Harness target |

Backend surface consumed: 19 Tauri commands (`run_auto_scan`,
`dismiss_overlays`, `kill_high_risk_processes`, `quarantine_entry`,
`undo_entry`, `list_quarantine`, `entry_details`, `reveal_location`,
`open_quarantine_folder`, `view_log`, `exit_app`, `scan_cleanup`,
`run_cleanup`, `start/stop_canary_guard`, `canary_status`,
`start_incident_observation`, `export_report`,
`export_incident_report`) plus `scan-progress`, `incident-progress`,
`canary-alert`, `e2e-done` events. All observed payloads match backend
DTOs (ScoredEntry + `attack`, quarantine records, ObservationResult).

Views (11): Scan Center (landing + live scan), Scan Results, Overview,
Startup Audit, Process Sentinel, Incident, Quarantine, Disk Cleanup,
Event Log, Canary Guard. Nav items (10): same minus Scan Center's
landing/scan split; Results/Audit/Processes gated `disabled` until a
scan completes (verified pre-scan).

Harnesses (16 scripts in `gui/devtools/`): verify, chipcheck,
cleanupcheck, pixelcheck, canarycheck, shots/sweepshots/mascotshots/
rakshakshots/visualshots/mediacapture, layout-check, diag, repro×3.

---

## 3. Screens observed (audit captures)

Captured to a temp dir (not committed) at 1600×900 unless noted:
landing, results, audit-drawer-open, processes, incident (verdict +
timeline), cleanup, canary-alert overlay, all-safe results,
footer-error, coverage PARTIAL/FAILED states, full-drawer (VALID
signature + publisher + 2 task actions), plus results at
1280×720 / 1366×768 / 1920×1080 / 900×700. Key per-screen notes:

- **Landing**: mascot hero + 2 cards (Rescue Scan / Disk Cleanup). No
  scope statement (what will be checked is unstated until the scan runs).
- **Scan running**: radar canvas + live feed (24 lines for 8 items +
  sweep) + stage pill. Elapsed time nowhere shown.
- **Results**: headline + 3 stat cards + review/cleaned/process/ransom
  panels + canary toggle + map rail. Dense but ordered.
- **Audit drawer (open)**: 10 field rows (Name → Evidence) + lazy
  Signature/Publisher + task Action rows. Longest panel in the app.
- **Incident**: verdict badge + methodology note + timeline (7 rows in
  mock) + 1 correlation + 1 transient window + export. Coherent flow.
- **Cleanup**: 4 category cards + 3 download rows + armed button with
  byte amount ("Really free 37.3 GB?").
- **All-safe**: "System is clean" + "Scan complete — all clear" +
  "Protected" posture. (See §7.)
- **Coverage mixed states**: PARTIAL renders "12 files (some task files
  unreadable) — 8 skipped" (warn dot); CHECK FAILED renders bare
  "error" (bad dot, thin detail — P1).

---

## 4. Visual system audit

### Layout
Single-column stack (max 880px) + fixed 380px map rail on results;
sidebar 216px collapsing to a 60px icon rail under 980px (verified at
900px: labels hidden, icons remain). No page-level horizontal scroll at
any tested viewport; panels scroll internally (feed, card lists capped
at 260px, tables in `.table-wrap`). Rhythm is consistent (28px stack
gaps, 14–16px panel padding), but results-view carries 6 stacked panels
+ rail, which reads as a wall on 720p heights — the primary density
problem. One measured defect: `.sr-only` "Action" table header
overflows its cell by 43px at 1280px (the visually-hidden pattern is
correct CSS; the overflow comes from its table context — fix scoping,
not the pattern).

### Typography
Inter UI + system mono stack. Headings: 18px semibold view titles,
15px panel titles, 13px section labels, 11px uppercase eyebrow labels
(1px letter-spacing). Body 13–14px, secondary 12px. Hierarchy is
legible but flat in long panels (audit drawer, incident timeline): many
rows share one weight/size, so scanning depends on dots and chips
rather than type scale. Mono is correctly reserved for paths, hashes,
commands, durations.

### Color
Dark blue-grey surfaces, muted borders, single lavender accent for
primary/focus, severity scale bad(red)/warn(amber)/ok(green)/info(blue)
reused for dots, badges, chips, stat values. Discipline is good (no
competing hues), but severity color is the ONLY channel in several
places (audit dots, coverage dots, footbar dot) — red/green-only
distinction with no shape or text redundancy (§12). Map region fills
reuse the severity scale at low opacity, which is decorative rather than
informative (region PR shown as bad-red on a mixed list — §7).

### Iconography and imagery
No icon font; status is carried by 8px dots, text badges, and two
canvas visualizations (scan radar, results map). Mascot wordmark on
landing only. Restraint is appropriate for a forensic tool; no change
recommended except adding non-color status redundancy (§17).

### Motion
Two keyframe animations (pulse on scan-play/status dot; fadeUp on view
enter) + spinner; view transitions 220–260ms ease-out. `prefers-
reduced-motion` disables pulse/fadeUp/spinner (verified forced +
media). Motion is sparing and never conveys state alone — keep this
policy; add a crossfade-free instant path for alert overlays (§17).

### Density
Comfortable on landing/canary; heavy on results (6 panels + rail),
audit drawer (33 rows incl. lazy fields), processes (3-column rows +
evidence). Long content scrolls inside panels correctly. The density
problem is ordering, not spacing: remediation actions sit below
informational panels (review panel after stat cards; quarantine buried
at nav position 7, §5).

---

## 5. Information-architecture audit

Sidebar order: Scan, Results, Overview, Audit, Processes, Incident,
Quarantine, Cleanup, Events, Canary — i.e. workflow order for the FIRST
scan, not for the RETURNING user (quarantine/restore, event review, and
canary status are daily destinations buried at positions 7–10).
Gating (`disabled` until scan) is correct and verified pre-scan, but
gated items give no reason ("run a scan to unlock" appears only as
title text). Landing offers Rescue Scan / Disk Cleanup with no scope
statement of what a scan checks — the user's first decision is
uninformed (§11). Cross-links are sparse: results review cards link to
audit drawer (good); nothing links results → quarantine, incident →
processes, or overview → event log. Mental model is "ten tools", not
"one investigation"; the redesign should frame Scan → Review → Decide
→ Verify as one path (§17).

## 6. Component inventory

Panels (`.panel`/`.audit-panel`), stat cards (`.stat-card`),
chips/badges (`.chip`, 9 severities + publisher/signature/task variants),
buttons (`.btn` primary/ghost/danger + armed two-step), dots
(`.dot` 4 states), tables (audit/process/quarantine/eventlog, sticky
headers, `.table-wrap` scrollers), drawer (audit detail, lazy
signature/publisher), timeline (`.log`), progress stages
(`.stage-pill`, checklist), toggle (canary switch), toast
(`#footbar-msg`, 2800ms + `role="status"`), overlay (`#overlay`,
spinner). Reuse is high; variants are disciplined. Gaps: no shared
confirm-dialog component (three bespoke armed patterns, §7), no
empty-state component (ad-hoc `.empty-state` strings, one failing
contrast), no legend component (dots/badges unexplained anywhere),
title-attribute tooltips on chips/name cells as the ONLY definition
source (hover-only, §12).

## 7. Trust and honesty audit (highest weight)

- **"System is clean" overclaim (P0).** Default headline
  (`index.html:191`) and `setHeadline` fallback (`app.js:1464`) render
  "System is clean" for ANY zero-finding scan — including partial scans
  and scans where nothing was observable. A scanner cannot establish
  cleanliness; it establishes "no findings observed". Overview's
  "Protected" posture is borderline-acceptable (it describes tool
  state); "System is clean" describes the machine and must go. Required:
  "No threats found" + always-visible scan scope line (items checked,
  coverage note, timestamp).
- **Armed-action inconsistency (P0).** Kill uses two-step arm ("Arm
  process termination" → armed "Terminate N processes"); cleanup uses
  arm + byte-amount confirm ("Really free 37.3 GB?"); quarantine
  executes on SINGLE click with no confirm (`app.js` quarantine handler
  has no arm state). File relocation is irreversible from the user's
  perspective (restore exists but is buried at nav 7) yet has the
  weakest guard. Required: two-step arm on quarantine matching kill.
- **Contrast failures (P0, WCAG AA).** `.metric-label` and
  `.empty-state` measure 2.64:1 (7:1 claim in prior summary was
  re-measured; both fail 4.5:1 regardless). Fix to ≥4.5:1.
- **Map implies geography (P1).** Region chips (PR/MY/US…) render as a
  choropleth where one bad region paints a country red; the map has no
  caption stating it aggregates signer/region metadata, not infection
  location. Required: caption + legend, or demote to list.
- **Coverage honesty is good but uneven (P1).** PARTIAL renders counts
  + skipped list (honest); CHECK FAILED renders bare "error" with thin
  detail — the failure state needs the same evidentiary care as the
  success state. Footbar error text ("3 errors") currently needs hover
  for source breakdown; the top scan-blocker should be inline.
- **No fabricated data found.** Every sampled number (counts,
  durations, sizes, PIDs, hashes, verdicts) traced to a mock/backend
  payload with matching provenance; mock file is labeled synthetic.
  Publisher "Unknown" is stated, never invented; zero-states say "none
  observed", not "none exist". This is the product's strongest trust
  asset — the redesign must not add marketing metrics (e.g. "threats
  blocked counter") that would break it.

## 8. Incident screen deep audit

Flow (verified end-to-end): duration select (1/2/5 min) → Observe →
progress bar + live entries → verdict badge + methodology note →
timeline + correlation + transient windows → export. Strengths: single
action, honest methodology line ("heuristic, not proof"), correlation
levels labeled DIRECT/STRONG/PARTIAL/WEAK/NONE with one-line
definitions, INSUFFICIENT verdict exists (not forced binary).
Weaknesses (P1): (a) timeline rows show process names + PIDs but NOT
  parent/command-line inline — the two fields that distinguish
  legitimate admin tools from LOLBins require opening another view;
  (b) INSUFFICIENT renders a one-line why with no recovery guidance
  (re-run longer? narrower scope? which evidence was missing?);
  (c) correlation panel lists one primary link; supporting/contradicting
  signals are not juxtaposed, so the user cannot weigh the verdict;
  (d) duration selector reuses `.filter-btn` styling, reading as a
  filter rather than an observation commitment — relabel as an explicit
  choice ("Observe for…") with cost stated; (e) live entries during
  observation are raw event lines with no plain-language framing for
  non-experts; (f) export buttons sit at the panel bottom, discovered
  late. No overclaim found in verdict copy; DIRECT definition
  ("same PID + time window") is appropriately narrow.

## 9. Tasks and autostart UX deep audit

Audit view: filter chips (All/Suspicious/Tasks/Services/Drivers) +
table (Name/Location/Status dots/Action) + drawer with 10 field rows
(Name, Location, Publisher, Status, Risk, Scheduled, Last run, Evidence,
Signature-lazy, Publisher-lazy) + per-task Action rows (Disable/Quarantine
for tasks). Strengths: evidence row states HOW each entry was found;
lazy signature/publisher are labeled as loaded-on-demand (honest
latency); task actions are scoped per-row (no global "disable all").
Weaknesses (P1): (a) Status dot is color-only (no text) in table rows —
  screen-reader and color-blind users get nothing at the list level;
  (b) task vs service vs driver distinction lives in one filter chip row
  with no explanation of differing risk (a scheduled task and a driver
  are not equally privileged); (c) Disable vs Quarantine consequences
  are unstated at the decision point (reversible? reboot needed? —
  unstated); (d) 33-row drawer buries Evidence below metadata — invert:
  verdict + evidence first, provenance second (§17); (e) remediation
  feedback is toast-only (2800ms) with undo living in quarantine view.

## 10. Overview and dashboard audit

Posture card (Protected/ неизвестность states) + 5 metric tiles
(Threats/Scanned/Suspicious/Unsigned/Protected or Failed variants) +
coverage panel + recent-activity chart (7 bars) + top-risk list +
risk-analysis panel + run buttons. Strengths: single-glance status;
failed-scan variant exists (metric tiles + coverage marked Failed —
verified); activity chart is real scan history, not decoration.
Weaknesses (P1): (a) "Protected" overstates (means "tools report no
findings", not "machine is safe" — pair with scope line per §7);
  (b) chart bars have no axis/values (relative heights only —
  decorative); (c) top-risk list duplicates results content without
  linking to it; (d) run buttons duplicate sidebar Scan entry (three
  paths to one action). Keep the posture card pattern; fix its caption.

## 11. Copy and microcopy pass

Voice is terse forensic ("Quarantined", "Skipped", "Unsigned") —
appropriate. Issues found (P2 unless noted): landing has no scope line
(what will be checked); scan progress speaks in stages, never elapsed
time or item counts ("Scanning…" with no denominator); "Suspicious"
vs "Unsigned" vs "Review" are never defined inline (hover-only titles);
quarantine/restore uses "Restore" (implies healing — prefer "Release
to original location" or keep "Undo" consistent with toast "Undone");
cleanup "Really free 37.3 GB?" is the best confirm string in the app —
propagate its pattern; INSUFFICIENT verdict has no next-step sentence;
canary toggle states ("Armed"/"Off") are clear; footbar error summary
needs the top blocker inline (§7). No blame language, no fear copy —
both correct for the genre; do not add urgency patterns.

## 12. Accessibility audit

- **Contrast (P0):** `.metric-label`, `.empty-state` at 2.64:1 — fail
  AA. All other sampled pairs pass (body text, chips, buttons, dots
  with text).
- **Keyboard (P0):** canary alert overlay has no Escape handler and no
  focus trap — zero `keydown`/`Escape`/`.focus` handlers exist in
  `app.js` (verified by search); the overlay's only exit is pointer
  click. All other flows are Tab-reachable (verified tab order:
  skip-link → sidebar → view controls; footer buttons reachable by Tab
  even when pointer-blocked). Fix: Escape + initial focus + return
  focus, trap while open.
- **Screen reader (P0/P1):** process table has no accessible name
  (`proc-table` unnamed — P0 for the highest-risk list); audit/process
  status dots expose no text alternative (color-only, P1); timeline and
  feed use generic lists with no live-region policy (scan feed
  announces nothing; `role="status"` exists only on the toast — P1:
  add polite live region for stage changes, NOT per-line); canvas radar
  and map have no text fallback (P2: adjacent data already exists for
  map; radar needs an aria-described summary of stage + counts).
- **Focus (P1):** `:focus-visible` 2px accent verified present; drawer
  open/close does not move focus into/out of the drawer (P1); view
  switches do not move focus to the view title (P1 — SPA-correct
  pattern missing).
- **Motion (pass):** reduced-motion path verified; policy to keep.
- **Touch targets (P2):** row action buttons and chips are small
  (~24–28px); raise to 32px minimum in the flagged-actions pass.

## 13. Responsive audit

900×700: sidebar collapses to 60px icon rail (labels hidden, tooltips
via title — pass with note: icon-only rail needs aria-labels
verified present on nav buttons); content stacks; no page h-scroll.
1280×720: all views usable; results rail wraps below content (correct).
1600–1920: centered 880px column + rail; generous margins, no stretch
defects. Landscape-only assumption holds (no mobile layout — acceptable
for a desktop agent console; do not add one). Remaining defect (P1):
views without inner scrollers (all six V2 views + incident + scan;
only results/cleanup have `.results-main{overflow-y:auto}`) overflow
`.stage` (overflow visible) once content exceeds stage height, and
because `.view` is absolutely positioned with z-index auto, the
overflowing content paints above and pointer-blocks `.footbar` buttons
— reproduced deterministically on incident (viewScrollH 1299 vs client
803; button center hit-tests to `#inc-review-panel`) and observed on
overview; keyboard Tab still reaches the buttons (DOM order
unaffected), so breakage is pointer-only. Fix: give every view an inner
scroll container (extend `.results-main` pattern), not footer z-index
hacks.

## 14. States matrix

| State | Observed | Verdict |
|---|---|---|
| Pre-scan (fresh launch) | gated nav disabled, landing intact | pass |
| Scanning (live) | radar + feed + stage pill, no elapsed/denominator | P2: add counts |
| Zero findings | "System is clean" + "all clear" + Protected | **P0: overclaim (§7)** |
| Findings present | review cards + stats + drawer | pass; reorder actions up (P1) |
| Partial coverage | counts + skipped list, warn dot | pass; add legend (P2) |
| Check failed | bare "error", thin detail | P1: full evidentiary failure panel |
| Quarantine empty/non-empty | table + per-row undo, folder open | pass; confirm parity (P0, §7) |
| Cleanup no-data/pre-scan | "Run a scan first" guard | pass |
| Cleanup failure | FAILED badge + byte shortfall + per-source errors (verified) | pass (best error UI in app — propagate) |
| Canary armed/alert/off | toggle + status card; alert overlay blocks dismiss (pointer-only, no Escape) | P0 keyboard (§12) |
| Footer errors | "N errors" hover breakdown | P1: top blocker inline (§7) |
| Toast | 2800ms auto-dismiss + role=status | P2: lengthen armed-action toasts to 6s with Undo |
| Incident insufficient | one-line why, no recovery | P1 (§8) |
| Long lists (150 items) | renders, scrolls internally | pass; add list virtualization note (P3) |

## 15. Evidence strictness audit

Sampled claims → sources: headline counts = `summary` payload; risk
badges = `entry.risk` + `attack` mapping; quarantine rows = backend
records; incident verdict = ObservationResult fields; canary status =
`canary_status` polling. No counters, no telemetry, no "blocked" stats,
no severity inflation (HIGH only where backend says so). Mock data is
labeled synthetic in its own file and never ships. Uncertainty is
stated ("Unknown" publisher, "none observed" zero-states, heuristic
disclaimer). Verdict: the evidentiary chain is intact — the redesign's
hard constraint is to preserve it exactly (see §18 "must not change").

## 16. Competitive analysis (lenses, not clones)

- **Windows Security center:** scope-first language ("No actions
  needed" + what was checked) — borrow the scope-line pattern, not the
  chrome. CURE is already more evidence-rich per finding.
- **Malwarebytes scan report:** one armed decision per screen with the
  consequence in the button ("Quarantine selected (3)") — borrow for
  quarantine parity (§7); CURE's per-row undo is already better.
- **Process Explorer / Autoruns:** parent-PID + command-line inline in
  every row — borrow for incident timeline and process rows (§8);
  CURE's evidence-field pattern is the right vehicle.
- **GlassWire timeline:** plain-language event framing ("X connected to
  Y for the first time") — borrow tone for incident live entries (§8).
- **Little Snitch rules:** reversible-everything with visible undo —
  borrow persistence: every armed action leaves a durable receipt with
  Undo, not a 2800ms toast (§17).
What NOT to borrow: threat-score dials, animated radars-as-proof,
"protected" shields-as-guarantee, upsell-adjacent "advanced issues"
counts. All four would break §15.

## 17. Redesign direction

### Principles (binding)
1. State what was checked, every time a conclusion is shown.
2. Every armed action: consequence in the control, receipt after.
3. Uncertainty gets UI (INSUFFICIENT/PARTIAL/Unknown are first-class).
4. Evidence before provenance (verdict → why → raw).
5. No color-only meaning; no hover-only meaning; no toast-only undo.

### IA proposal
Sidebar reordered by frequency: Scan, Results, Quarantine, Incident,
Overview, Events, Audit, Processes, Cleanup, Canary — with section
labels (Decide / Understand / Maintain). Gated items name their unlock
("Results — run a scan"). Every armed view links its receipt view
(results ⇄ quarantine; incident → processes).

### Key wireframes (text)
- **Results (revised):** scope line ("Checked 8 entries · 2 skipped ·
  14:02:11") under a "No threats found" headline; armed-action bar
  FIRST (Quarantine all [armed], Kill [armed], counts in labels);
  findings; details; map demoted to captioned figure or list.
- **Audit drawer (revised):** verdict + Evidence field pinned top;
  metadata collapsed below; Disable/Quarantine buttons carry
  consequence ("Disable (reversible, needs restart)").
- **Incident (revised):** commitment control ("Observe for 2 min —
  passively records…") → live plain-language entries → verdict +
  inline PPID/command-line timeline → supporting vs contradicting
  signals → INSUFFICIENT recovery panel → export.
- **Overview (revised):** posture card with scope caption ("Based on
  scan 14:02 · 8 entries · 2 skipped") + valued chart (axes) + linked
  top-risk rows.

### Component changes
New: confirm-arm (shared two-step), scope-line, receipt (durable undo),
legend (dots/badges), live-region policy, failure-panel (propagate
cleanup's), recovery-panel (INSUFFICIENT). Changed: headline copy,
quarantine arm, toast durations (6s armed), focus management (drawer,
views, overlay), inner scrollers on all views, 32px targets on
flagged actions. Removed: "System is clean" string, hover-only
definitions, map-as-proof framing.

### Motion
Keep reduced-motion policy; overlay appears instantly (no fade);
armed-state change gets a 150ms background shift only; no new
animation. Focus moves are instant, never scrolled-smooth (vestibular).

### Rollout
Phase-gated per §20; accessibility and honesty fixes ship before any
visual restyle; each phase ends with the probe that caught the defect
re-run green.

## 18. Open questions

Resolved (verified this audit): backend payload shapes for all 19
commands; gating behavior pre-scan; toast duration (2800ms); overlay
keyboard absence (zero handlers); contrast values (2.64:1); footer
overlap mechanism (absolute-view overflow, pointer-only); reduced-motion
coverage; 150-item rendering.
Deferred to implementation: exact scope-line wording (needs backend
field check for skipped-reason strings); chart axis values (history
payload has counts — confirm); icon-rail aria-label spot-check.
Must NOT change (backend frozen): scoring, ATT&CK mapping, quarantine
semantics, canary/watcher behavior, redaction, no-network posture,
event payloads, E2E `E2E_RUNNER_JS` contract.

## 19. Appendix

Captures: temp dir only (landing, results, audit-drawer,
processes, incident, cleanup, canary-overlay, all-safe, footer-error,
PARTIAL/FAILED, full-drawer, 4 viewports) — uncommitted by choice.
Probes: `probe-full` (12-step journey), `probe-a11y` (contrast/overflow/
tab/focus/aria), `probe-resp` (5 viewports), `probe-geom`/`probe-geom2`
(overlap mechanism + deterministic repro), `probe-labels`,
`probe-full15`, `probe-fw` (firewall wording — N/A, no such UI found),
mock trap for PARTIAL/FAILED via `__TAURI__.invoke` override.
Key files+lines: headline `index.html:191`, `app.js:1464`;
quarantine single-click handler `app.js` (no arm state; cf. kill arm,
cleanup `Really free` confirm); toast `app.js:1819` (2800ms);
overlay markup `index.html` (no dialog role); proc table
`index.html` (unnamed); views/scrollers `index.html:165,298`,
`style.css` (`.results-main` only on results/cleanup);
`VERDICT_WHY` map `app.js` (one-liners, no recovery).
Glossary: sweep-listener = overlay-drop watcher; canary = tripwire
file guard; armed = two-step confirm state; scope line = checked-what
caption; receipt = durable undo record.

## 20. Implementation checklist

**PHASE A — P0 honesty + blockers (ship first, no restyle)**
- [ ] A1 Replace "System is clean" everywhere (default headline,
  `setHeadline` fallback) with "No threats found" + scope line (items
  checked · skipped · timestamp) on results + overview captions.
- [ ] A2 Quarantine two-step arm matching kill (armed label carries
  count + consequence); keep per-row Undo; lengthen armed toasts to 6s.
- [ ] A3 Contrast: `.metric-label`, `.empty-state` to ≥4.5:1.
- [ ] A4 Canary overlay: `role="alertdialog"`, Escape dismissal, focus
  trap + initial/return focus (add app's first `keydown` handling).
- [ ] A5 Name the process table (`aria-label`/`aria-labelledby`).
- [ ] A6 Inner scroll container on all six V2 views + incident + scan
  (extend `.results-main` pattern); re-run `probe-geom2` green.
- [ ] A7 Regression: full journey + a11y + overlap probes green.

**PHASE B — Incident flagship**
- [ ] B1 PPID + command-line inline on timeline rows.
- [ ] B2 INSUFFICIENT recovery panel (what was missing, re-run longer/
  narrower, what changes the verdict).
- [ ] B3 Supporting vs contradicting signals juxtaposed with verdict.
- [ ] B4 Duration selector as commitment control with cost stated.
- [ ] B5 Plain-language live entries; export raised above the fold.

**PHASE C — Results + quarantine receipts**
- [ ] C1 Armed-action bar first with counts in labels.
- [ ] C2 Durable receipts with Undo (replace toast-only undo).
- [ ] C3 Map caption + legend or demotion to list.
- [ ] C4 Failure-panel pattern propagated (check-failed = cleanup-grade).

**PHASE D — Overview honesty**
- [ ] D1 Posture scope caption; valued chart axes; linked top-risk rows.

**PHASE E — Audit/processes/task lists**
- [ ] E1 Text redundancy for status dots; drawer verdict+evidence first;
  Disable/Quarantine consequences inline; 32px targets.

**PHASE F — Keyboard + SR pass** (focus management, live regions,
legends, canvas fallbacks) · **G — Copy pass** (§11) · **H — Density +
responsive polish** · **I — Full regression** (all probes + E2E) ·
**J — Final audit + docs.**

## 21. Final verdict

The interface is evidence-honest and structurally sound, but it
overstates its conclusions ("System is clean"), guards its most
destructive action the least (single-click quarantine), and locks
assistive-technology users out of high-stakes flows (contrast, overlay,
unnamed table). Fix honesty and access first (§20 Phase A); then make
Incident the flagship (§20 Phase B); restyle last. No backend changes
are required for any of it.

