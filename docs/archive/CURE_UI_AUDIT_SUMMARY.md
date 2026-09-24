> **[HISTORICAL] This is an archived development report from the V3 audit cycle (Aug–Sep 2026). It contains internal context specific to the author's machine. For current documentation, see docs/validation/REAL-PC-VALIDATION.md, docs/ARCHITECTURE.md, and docs/CHANGELOG.md.**

# C.U.R.E. UI Audit V3 — Summary

Full audit: `CURE_UI_AUDIT_V3.md` (21 sections; audit only, zero files
modified). Verified at commit `52ab905` + V2.2/V2.3 work via Playwright
+ mock backend, viewports 900×700 → 1920×1080, reduced-motion on/off.

## Top 10 strengths
1. Evidentiary chain intact — every sampled number traces to a backend
   payload; no fabricated metrics anywhere.
2. Uncertainty is stated ("Unknown" publisher, "none observed",
   heuristic disclaimer, INSUFFICIENT verdict exists).
3. Cleanup failure UI is exemplary (FAILED badge + shortfall + per-source
   errors) — the pattern to propagate.
4. Two-step arms on kill and cleanup with consequence in the label.
5. Workflow-ordered sidebar with verified pre-scan gating.
6. Lazy signature/publisher labeled as on-demand (honest latency).
7. Reduced-motion path verified working; motion never carries state.
8. Per-row quarantine Undo exists (needs durability, not invention).
9. Incident correlation levels narrowly defined (DIRECT = same PID +
   window — appropriately cautious).
10. No blame language, no fear copy, no dark patterns.

## Top 10 issues (ordered)
1. (P0) "System is clean" overclaim — `index.html:191`, `app.js:1464`.
2. (P0) Single-click quarantine vs two-step kill/cleanup.
3. (P0) Contrast 2.64:1 on `.metric-label`, `.empty-state`.
4. (P0) Canary overlay: no Escape, no focus trap (zero `keydown`
   handlers in `app.js`); unnamed process table.
5. (P1) Tall views overflow `.stage` and pointer-block footer buttons
   (deterministic repro: incident scrollH 1299 vs 803; only
   results/cleanup have inner scrollers).
6. (P1) Map implies geography without caption/legend.
7. (P1) CHECK FAILED state is a bare "error" vs PARTIAL's full detail.
8. (P1) Incident timeline lacks inline PPID/command-line;
   INSUFFICIENT has no recovery guidance.
9. (P1) Status dots are color-only; definitions hover-only; focus not
   managed (drawer/views); toast-only undo (2800ms).
10. (P2) No scope line on landing/results; scan progress has no
    denominator; chart has no values; sidebar ordered for first scan,
    not daily use.

## Design principles (binding)
1. State what was checked, every time a conclusion is shown.
2. Every armed action: consequence in the control, receipt after.
3. Uncertainty gets UI (INSUFFICIENT / PARTIAL / Unknown first-class).
4. Evidence before provenance (verdict → why → raw).
5. No color-only meaning; no hover-only meaning; no toast-only undo.

## Implementation order
**A** — P0 honesty + blockers (headline+scope, quarantine arm,
contrast, overlay keyboard, table name, view scrollers) → **B** —
Incident flagship (PPID/cmdline, recovery panel, signals, commitment
control) → **C** — Results + receipts → **D** — Overview honesty →
**E** — Audit/process lists → **F** — keyboard/SR → **G** — copy →
**H** — density/responsive → **I** — regression → **J** — final audit.
Honesty and access ship before any visual restyle. Backend frozen:
scoring, ATT&CK, quarantine semantics, canary/watcher, redaction,
no-network, payloads, E2E contract.
