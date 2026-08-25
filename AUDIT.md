# C.U.R.E Project Audit — 2026-08-24

Full pre-scoping audit. Purpose: surface accumulated drift/debt across iterative
development rounds, not to reassure. Every claim below was re-verified against
the working tree on this date (commit `2ba4b27`, clean status).

---

## 1. Feature inventory — what actually exists

### `core` (cure_core) — shared engine
| Module | What it does |
|---|---|
| `model.rs` | `PersistenceEntry` (id/source/name/command/location), `ScoredEntry`, `RiskLevel`, `PersistenceSource` |
| `scanners/startup.rs` | Lists `.bat/.cmd/.ps1/.lnk/etc.` files in a Startup folder (root overridable) |
| `scanners/scheduled_tasks.rs` | Walks task XML tree, extracts first `<Command>` (entity-decoding, UTF-16 aware) |
| `scanners/registry.rs` | Read-only enumeration of HKCU+HKLM Run/RunOnce autoruns (Windows-only, cfg-gated) |
| `risk.rs` | Heuristic scoring: drop-zone +30, trusted-path −20, random-name +25, hidden-PowerShell +25, profile-bump +10, signed −40, invalid-signature +40, known-bad hash forces HighRisk; thresholds HighRisk ≥40, Suspicious ≥15 |
| `signature.rs` | Resolves command→exe path; Authenticode verdict via WinVerifyTrust (+ catalog APIs); non-Windows returns `Unverified` |
| `hash_intel.rs` | SHA-256 of candidate binaries vs compile-time-embedded JSON seed list (3 synthetic demo hashes) |
| `baseline.rs` | Save/load/diff persistence baselines (UTC timestamp, id-set diff) |
| `quarantine.rs` | Move-based quarantine (rename, else copy+length-verify+remove-source; failed copy cleans up dest), JSON records, undo incl. parent-dir recreation |
| `cleanup.rs` | Disk-cleanup scans (temp/browser-cache/recycle-bin/Windows.old/old-installers), direct `delete_candidates` w/ per-item failure capture, `run_dism_cleanup` shell-out |

### `cli` (cure.exe)
Single `main.rs`: `scan` (report + write baseline), `diff` (new entries since
baseline), `quarantine <id>` (moves file/task XML; prints manual reg-delete
instructions for registry entries), `undo <id>`, `cleanup scan`,
`cleanup run [--include-downloads] [--dism]` (confirm-gated deletes),
global `--data-dir/--startup-root/--tasks-root`.

### `watch` (cure-watch.exe)
| Module | What it does |
|---|---|
| `main.rs` | Self-installs to `%APPDATA%\...\Startup`, polls drive letters every 1500 ms, launches GUI (`--data-dir <drive>`) on valid trigger; GUI fallback search beside watcher |
| `drives.rs` | A–Z drive-root existence probe (Windows; empty stub elsewhere) |
| `detector.rs` | Set-diff of previous/current drive sets |
| `trigger.rs` | `.cure-trigger` must contain `CURE-TRIGGER-V1` (trailing-whitespace tolerant, leading-whitespace/case strict) |
| `logger.rs` | Best-effort UTC append log to `%APPDATA%\cure-watch.log` |

### `gui` (Tauri v2 + static frontend)
Backend commands: `run_auto_scan` (scan all sources → score → auto-quarantine
HighRisk non-registry → emit progress events → save baseline),
`quarantine_entry`/`undo_entry`, `open_quarantine_folder`, `view_log`,
`exit_app`, `scan_cleanup`, `run_cleanup`.
Frontend: animated radar/net-map canvas views, live scan feed, results screen
with reason-chip cards, footer actions, **DISK CLEANUP panel** (category toggle
cards, downloads checklist, two-step armed confirm button, failure list).
Dev harness: `index.dev.html` + `mock-tauri.js` (canned backend) +
6 playwright tools in `devtools/`.

### README cross-reference
**README claims that hold:** poll interval "~1.5s" (=1500 ms ✓), self-install
no-admin ✓, state-on-USB via `--data-dir <drive>` ✓, scoring weights ✓,
"quarantine never deletes" ✓ (see §5), registry read-only ✓, CLI usage ✓,
cleanup usage ✓, "hash-intel is a demo seed" ✓ (exactly 3 synthetic entries).

**Drift found:**
1. README says "**60+ tests**" — actual count is **81** (stale number).
2. "What's in the box" does **not mention the GUI Disk-Cleanup feature**
   (shipped in `6187110`) — code does more than README advertises.
3. Scoring description omits the **+10 "runs directly from user profile
   folder" bump** present in `risk.rs`.
4. Minor: TESTING.md exists but isn't linked from README.

---

## 2. Verification status matrix

Legend: ✅ Automated-tested · 🔧 Manually-verified-on-real-hardware · ⚠️ Spec'd-but-unverified

| Feature | Status | Evidence / honest notes |
|---|---|---|
| Startup-folder scanning | ✅ | unit tests (incl. missing-dir case) |
| Scheduled-task XML parsing | ✅ | unit tests incl. UTF-16/entities |
| Registry autorun reading | ✅ | `scan_completes_without_panicking`; live-reads real HKCU/HKLM in every test run on this machine |
| Risk scoring | ✅ | extensive boundary/unit tests |
| Known-bad hash matching | ✅ | unit tests with fixture hashes |
| Authenticode verdict (WinVerifyTrust) | ✅🔧 | unit tests hit **real** MS binaries on this machine — effectively hardware-verified every CI/test run |
| Baseline save/diff | ✅ | unit tests |
| Quarantine + undo | ✅ | unit tests: move/metadata/undo/parent-recreate/double-quarantine; move implemented as verified copy-then-remove |
| Watcher: poll/diff logic | ✅ | detector/trigger/drives units |
| Watcher: self-install + launch chain | 🔧⚠️ | **live-fire PASS** on host (2026-08, subst-drives + fake-overlay, z-order/pixel/log proof) — but that simulated drive arrival via subst; **a real removable-device arrival event inside a VM is still unproven** |
| GUI scan → results render | ✅🔧 | playwright harnesses (verify/chipcheck/pixelcheck/shots) + real launches on this machine |
| GUI network graph/map | ✅ | pixelcheck + map-count assertions in dev harness; real-data rendering visually confirmed by operator in past rounds |
| Footer buttons (quarantine folder / view log / exit) | ⚠️ | error/success paths asserted **only against the mock backend**; never explicitly exercised against the real Tauri backend |
| Disk cleanup: core scans/deletes | ✅🔧 | 17 unit tests + **sandboxed end-to-end delete run on real filesystem** (env-isolated roots) + real-machine scan (41.7 GB found) |
| Disk cleanup: GUI ↔ real Tauri backend | ⚠️ | **panel verified only via mock-tauri** (28/28 cleanupcheck assertions). The real `invoke("scan_cleanup"/"run_cleanup")` plumbing has never been driven end-to-end in the packaged app. Backend compiles; commands registered; plumbing unexercised. |
| DISM component-store cleanup | ⚠️ | `run_dism_cleanup` has **never been executed at all** — not manually, not in a test. Only arg-list construction is unit-tested. |
| Real-VM validation (GUNDA) | ⚠️ | **Incomplete.** Snapshot taken, staging shipped, INSTRUCTIONS delivered; guest phases (Setup → PostReboot → OverlayAndUSB) never reported complete; VM later found aborted. WebView2 presence inside guest unknown (host couldn't download bootstrapper). |
| Physical USB passthrough | ⚠️ | VBoxUSBMon/VBoxUSB drivers were missing; hand-registered from INF specs; host rebooted; **attach itself still pending** (was interrupted by audit request). |

---

## 3. Git/build hygiene

- `git status`: **clean** — nothing uncommitted (HEAD `2ba4b27`).
- `cargo build --release --workspace`: succeeds, **0 warnings, 0 errors**.
- `cargo test --workspace`: **81 passed, 0 failed** (core 67 [incl. 17 cleanup],
  watch 14).
- GUI `cargo build --release` (src-tauri; tauri-cli not installed — equivalent
  build): **0 warnings**, artifact `target/release/cure-gui.exe` produced.
- Note: `gui` is workspace-excluded (own lockfile/target) — intentional, but
  means its deps aren't pinned by the root `Cargo.lock`.

---

## 4. Design consistency

- `index.html` vs `index.dev.html`: diff is **exactly the expected 3 lines**
  (title suffix, wordmark "· Mock", `<script src="mock-tauri.js">`). Zero
  structural drift. ✓
- Old neon-teal/hex-grid skin: **no artifacts found** (grepped neon/teal/
  hexagon/hex-grid/#0ff/#00e5/cyber/glow/scanline across css/js/html).
  Two cosmetic survivors, both *current*-design with legacy *names*:
  - `.tint-teal` / `.chip.teal` classes now color with `var(--safe)` (green);
    name says teal. Rename candidate only.
  - `glow` identifiers in `app.js` are violet canvas radial gradients — current
    design, just a generic variable name. Not debt.
- Design tokens (`:root`, 21 custom properties): all **referenced** in
  stylesheet (min 2 uses each) — no dead variables. Current palette:
  bg `#0a0a0b`, panel `#131315`/`#17171a`, text `#ededef`/muted/faint greys,
  accent violet `#7c6cf0` (+dim/border rgba), semantic safe `#4fae7d`,
  caution `#d1a13f`, danger `#e1594f`, radius 10px, mono/ui font
  stacks. (Original re-skin spec text predates this audit's context window;
  verified internal consistency and full token utilization instead.)

---

## 5. Safety invariants — re-confirmed

1. **Quarantine never deletes**: implementation prefers `fs::rename`; on
   cross-volume fallback it copies, **verifies byte-length equality**, removes
   the source only after success, and cleans the destination on mismatch
   (`quarantine.rs:66-77`). Undo restores identical bytes. ✓
2. **Cleanup is a separate safety model, correctly isolated**: `disk_cleanup`
   is imported only by the two explicit cleanup entry points (`cli`
   `CleanupAction::*`, gui `scan_cleanup`/`run_cleanup`). Malware-quarantine
   paths never call it; cleanup never calls quarantine. ✓
3. **Cleanup cannot traverse outside its five target classes**: CLI accepts no
   paths at all; GUI `run_cleanup` filters freshly-rescanned candidates by
   category key and matches download paths by **exact string equality against
   the scan result** — arbitrary/user-supplied paths can never reach
   `delete_candidates`. Scan roots are env-derived standard locations only;
   browser scan touches only `...\User Data\<profile>\Cache`; Documents/
   Desktop are unreachable by construction. ✓
4. **Registry untouched**: `scanners/registry.rs` contains zero write/delete/
   create calls (grep-verified); quarantine of registry entries prints manual
   instructions instead of acting. ✓

---

## 6. Known gaps — canonical consolidated list

1. **Trigger authenticity is a shared-secret string**, not crypto
   (Ed25519 drive-signing planned).
2. **Registry entries are never auto-remediated** — flagged with manual
   instructions only.
3. **Signature checking is verdict-only** — no publisher/certificate details.
4. **Hash intel = 3 synthetic demo hashes**; no feed-update mechanism (idea on
   record: refresh JSON at build time or load from USB).
5. **Persistence coverage gaps**: services, WMI event subscriptions, IFEO,
   COM hijacks.
6. **Parser limits**: Startup `.lnk` targets unresolved; scheduled-task XML
   parses only the first `<Command>`.
7. **Real-VM validation chain incomplete** (biggest open item):
   Setup/PostReboot/OverlayAndUSB phases unverified in GUNDA; WebView2-in-guest
   unknown; host network blocked bootstrapper download.
8. **Physical USB passthrough attach still pending** post driver-fix/reboot.
9. **DISM path never executed anywhere** (design intent: manually-verified-only).
10. **GUI cleanup panel untested against the real Tauri backend** (mock-only so far).
11. **Footer buttons untested against real backend** (mock-only).
12. Cleanup scope limits (by design, but list them): Recycle Bin = system drive
    only; caches = Chrome/Edge only; locked files reported-not-forced;
    GUI downloads-age fixed at 30 days; Windows.old delete may need elevation/
    ACL takeover — never attempted against a real one (this machine HAS a
    30.6 GB Windows.old — natural first real test, needs admin console).
13. Watcher self-install **never updates an already-installed older copy**
    (skips if `%APPDATA%` Startup file exists) — stale-binary hazard.
14. Detection is letter-polling (1.5 s), not `WM_DEVICECHANGE` — fine in
    practice, noted as deliberate simplicity.
15. Session-level constraint (process, not product): visual QA relies on
    playwright DOM/pixel assertions because screenshots can't be viewed in
    these sessions.
16. `gui/dev-screenshots/*.png` get byte-churned by every harness run and are
    tracked — recurring commit noise.

---

## 7. Dead ends / abandoned code

| Item | Verdict |
|---|---|
| `baseline::save_baseline` — pure alias of `save`, GUI-only legacy name | **Consolidate** (keep one name) |
| `.tint-teal`/`.chip.teal` class names carrying old-palette vocabulary | Cosmetic rename to `safe`/`positive` |
| `testing/vm-stage/_parts/*.ps1` vs assembled `run-cure-validation.ps1` | Differs by **4 blank lines only** (join artifacts) — regenerate once to kill drift risk, or delete parts |
| `gui/devtools/diag.mjs` | One-off timing debug tool; harmless, fold into `verify.mjs` or keep as dev util |
| `design-ref/` (original skin references) | Historical; keep out of any release packaging |
| `testing/fake-overlay/` | **Not dead** — intentional inert fixture documented in TESTING.md; keep |
| Mock knobs | All four (`ITEM_COUNT`, `ALL_SAFE`, `FOOTER_ERRORS`, `CLEANUP_FAILURES`) consumed by harnesses — none orphaned |

No orphaned source files, no commented-out code blocks, no unused Rust
dependencies observed in any crate manifest.

---

## Bottom line

Engineering hygiene is strong (clean builds, 81 green tests, tight safety
separation, honest in-code caveats). The project's weak axis is **end-to-end
verification of the newest layer**: GUI-cleanup-over-real-backend, DISM,
the real-VM self-install chain, and physical USB passthrough are all coded,
plausibly correct, and unproven. §6 items 7–11 are where new work should start.
