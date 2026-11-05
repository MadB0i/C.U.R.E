# Detection Engine v2 — deeper malware scanning + repo-wide broken/overlap sweep

**Date:** 2026-11-06
**Status:** COMPLETE (605 workspace tests + 14 GUI tests green; UI-04 32/32 and UI-09 19/19 native against the rebuilt binary)
**Scope:** "Make the virus/persistence scanning — the stuff Windows skips — more advanced, and fix everything broken, erroring, or duplicated."

---

## 1. Detection upgrades (what Defender skips, now scored)

### 1.1 Scheduled-task arguments are finally scored (the biggest gap)
Task XML `Arguments` (and second+ actions) were parsed for display only, so
`Command: powershell.exe` + `Arguments: -enc AAAA…` scored **0**. Now
`PersistenceEntry` carries scorer-visible `arguments` (`#[serde(default)]`,
old baselines stay readable) while the stable id is still computed from
`source + name + command` only — no baseline/quarantine identity churn
(`scorer_command()` feeds the heuristics). Evidence: `risk::tests::
scheduled_task_arguments_are_scored`, `scheduled_task_second_action_is_scored`,
`normal_signed_task_arguments_stay_safe` (guard against over-firing), and
`scheduled_tasks::tests::scan_report_attaches_argument_material_without_churning_ids`.

### 1.2 LOLBIN list widened + script-cradle content heuristics
- LOLBINs now include `certutil.exe`, `bitsadmin.exe`, `msiexec.exe`,
  `installutil.exe` — a signed `certutil -urlcache http://…` used to score 0.
- New program-independent **cradle tokens** (`IEX`, `invoke-expression`,
  `DownloadString`, `DownloadFile`, `FromBase64String`, `javascript:`,
  `vbscript:`) fire on command *content* even for signed non-LOLBIN helpers,
  and suppress the trusted-location/Valid-signature discounts (encoded
  PowerShell stays owned by the existing sneaky-PowerShell heuristic — no
  double counting; explicitly tested).
- Network-share persistence (`\\host\share\…`) from a plain program now
  scores +15 Suspicious (URLs stay LOLBIN-gated so a benign homepage link
  stays Safe).

### 1.3 Second-tier drop zones
`ProgramData` and `AppData\Roaming` are the classic landing spots that also
host legitimate updaters — they now add a +15 Suspicious floor that a valid
signature can still talk down to Safe (both directions tested).

### 1.4 Shadow-copy-wipe tripwire revived (was dead code everywhere)
`shadow_wipe_reason` requires a command line; **both** real pollers passed an
empty one, so the wipe tripwire could never fire in production. Command lines
are unavailable to Toolhelp snapshots, so a name-only companion check now
fires for the two tools with no benign mid-session use — `vssadmin.exe`,
`bcdedit.exe` — worded honestly as correlated evidence ("command line not
captured — verify…"). `wmic`/`diskshadow`/`wbadmin` deliberately stay
command-line-only (benign admin uses would be mislabeled).
`watch/src/canary.rs` + `gui/src-tauri/src/main.rs` pollers wired; 2 new tests.

### 1.5 New scanner: Winlogon & Active Setup (boot/logon layer)
`core/src/scanners/bootlogon.rs` reads `Winlogon` `Shell`/`Userinit`/
`Taskman`/`AppSetup` (both hives) and Active Setup `StubPath` per component
(both hives) — classic persistence Defender never surfaces. Entries are
detection-only (`RegistryRun` source, real key path in `location`), access
accounting honest (skipped keys counted). Wired into `collect_all`, the CLI
(`cure scan` prints an `arg:` line and a "Winlogon & Active Setup" coverage
row) and the GUI (10th collector stage + coverage row). The X-Ray radar and
the Overview dial now render **ten** layers (tick arc and "ALL N LAYERS
CLEAN" derived from the data, not hardcoded).

## 2. Broken / overlapping things found and fixed

| Finding | Fix |
|---|---|
| `core/src/threat_intel.rs` + `seed_iocs.json`: a whole second, incompatible IOC store with zero callers (overlaps `hash_intel`) | Deleted (module + orphaned seed file + lib.rs reference) |
| `style.css` selectors for views deleted in UI-06 (`#view-audit`, `#view-processes`, `#view-incident .results-stack`) | Removed, comment records why |
| `app.js enablePostScanNav` referenced sidebar items that no longer exist | Simplified to the live `nav-results` entry |
| Investigate coverage strip duplicated the denied reason (Stage UI-03 documented cosmetic bug): "ACCESS DENIED — access denied (elevation may help) · access denied (elevation may help)" | Row detail now appended only when it adds information the status detail lacks |
| `move-bot.js` grabbed a `doneUndo` element it never uses | Removed |
| Hardcoded 9s in X-Ray tick arc / "ALL 9 LAYERS CLEAN" / Overview dial vs the real 10-layer engine | Derived from the layer data; stale "nine" copy updated |
| README claimed "nine-layer … Winlogon" while Winlogon was never scanned | README now states the ten surfaces truthfully |
| Dead `merge()` helper written then superseded in `bootlogon.rs` | Removed before commit |

Known, deliberately not "fixed": CLI/GUI use hand-rolled collect loops
instead of `scanners::collect_all` (they carry root overrides / progress
emits; a unifying refactor is follow-up work, not a defect); `formatBytes`
exists as small module-local helpers in two IIFEs; services in trusted
locations still skip hash-IOC lookups (perf tradeoff, documented in code).

## 3. Verification

- `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings`: clean (both workspaces).
- Root workspace tests: **605 passed / 0 failed** (incl. 8 new risk tests, 2 new canary tests, 4 new bootlogon tests, 1 new scheduled-tasks test).
- GUI crate: clippy clean, **14 tests / 0 failed**.
- `ui04-native-verify.py` **32/32** against the rebuilt binary — coverage badge reads **10/10 collectors**, findings lens populated (safe=123), themes + 1366×768 verified.
- `ui09-native-verify.py` **19/19** (human titles intact, Move Bot unaffected).
- `dist` ↔ `dist-demo` byte-identical; dev shell exact; `audit_views` 10/10; `audit_ui_cleanliness` 0 emoji lines.

## 4. What is NOT verified

- Live malware behavior: the new heuristics are tested against synthetic
  commands and the hash list remains fixture data (documented in-code).
- The shadow-wipe name-only tripwire is verified by unit tests and code
  path review; triggering it natively would require actually starting
  `vssadmin.exe` on the operator machine — not done.
- Scheduled-task argument scoring is exercised by unit tests; the native
  scan on this machine happened to carry no hostile task, so no live
  HighRisk from the new path was observed end-to-end.
- `evidence-console.py` interactive suite not re-run this session.

## 5. Risks / follow-ups

- `ProgramData`/`Roaming` soft zone can raise review noise on machines with
  many unsigned auto-starters there — by design (Suspicious, reviewable), and
  a valid signature still clears it.
- Task `arguments` appear in JSON exports (sanitized by default like the rest
  of the entry) — command lines were already exported; no new secret class.
- Follow-up: unify the three collect pipelines; wire `certutil`-style hashing
  for trusted-location services if perf budget allows.
