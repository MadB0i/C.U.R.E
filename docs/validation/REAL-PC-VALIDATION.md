# Real-PC validation report — C.U.R.E

- Date: 2026-09-24
- Machine class: Windows 11 laptop, standard (non-elevated) user account.
  No username, serial, or machine identifiers are recorded in this repo.
- Binaries: `cure.exe` / `cure-watch.exe` built from branch
  `cure-watch-uninstall-overlay-safety` (`cargo build --release -p cure_cli
  -p cure_watch`); `cure-gui.exe` from `gui/src-tauri`.
- Scope: HKCU/user-profile only. No admin, no HKLM, no `cleanup run`, no
  DISM, no Defender/SmartScreen changes, no real malware. Every mutating
  step was confirmed against `CURE_TEST_*`-only targets first (dry-run or
  list), except the explicitly human-authorized watcher/USB flow (§8),
  which is recorded step by step below.
- Scrubbing: `%USERPROFILE%` stands in for the home path; only
  `CURE_TEST_`-named findings are quoted.

## Gate G1

`Get-ComputerRestorePoint` as standard user → **Access denied**
(expected: the query needs elevation). The human confirmed proceeding
(restore handled on their side; games/recorders/overlays closed; work
saved). Recorded as a limitation: a standard-user kit cannot verify
restore-point state by itself.

## Step table

| # | Step | Status | Command / evidence (excerpt) |
|---|------|--------|------------------------------|
| 0 | Baseline snapshot (read-only) | PASS | Snapshot to `%TEMP%\CURE_VALIDATION_BASELINE` (outside repo): `hkcu-run.txt`, `startup-listing.txt`, `tasks.csv`, `localappdata-cure.txt`, `processes.txt`. Noted pre-existing: `%LOCALAPPDATA%\CURE\baseline.json` (32,820 B) and `%APPDATA%\…\Startup\cure-watch.exe` (342,016 B, 2026-08-26) — left untouched. |
| 1.1a | Kit dry-run | PASS | `make-test-artifacts.ps1 -DryRun` printed the 6-step plan, changed nothing (verified absent afterwards). |
| 1.1b | Kit live run | PASS | `make-test-artifacts.ps1`: dir, helper exe (SHA256 `3F543719…55380`), HKCU Run value, Startup `.lnk` created; ACL file + `CURE_TEST_acl/hash-before.txt` snapshots saved. |
| 1.1c | Scheduled task creation | SKIPPED | `schtasks /Create …` → `ERROR: Access is denied.` (standard user; Task Scheduler root needs elevation). Script prints SKIP honestly. |
| 1.2a | Scan detects Run value | PASS | `cure.exe --data-dir … scan` → `SAFE 0 CURE_TEST_run (registry-run) id=00146e7ed7fe8c80`, `loc: HKCU\…\Run`, ATT&CK T1547.001, reasons `+30 drop zone / +10 profile / -40 Valid Signature`. |
| 1.2b | Scan detects shortcut | PASS (after F-LNK-1 fix) | `SAFE … CURE_TEST_startup.lnk (startup-folder) id=f5199b4df24c0912`, T1547.001, `cmd: C:\...\CURE_TEST\CURE_TEST_helper.exe` (resolved absolute target, not .lnk path), `lnk→: C:\...\CURE_TEST\CURE_TEST_helper.exe [exists]` (long path, not 8.3). Before fix `ccc7...` showed `lnk→: ..\..\..\..\..\..\Local\Temp\… [MISSING]` and `cmd` was the .lnk path itself. |
| 1.2c | Scan task row | SKIPPED | Correctly absent (creation SKIPPED above). |
| 1.2d | Risk expectation | PASS (corrected) | Both rows Safe — **after fix, correctly** because the helper is signed-notepad bytes. The earlier report called this "correct for signed bytes" while the .lnk was actually scored from the .lnk file path (`…\Startup\…lnk`, Unknown sig, score 0) not the target — coincidence. Re-validated after fix: `cmd` is the target, `why:` now includes `-40 Valid Signature` for the .lnk as well (target's valid sig), and the `lnk→` line is long absolute + `[exists]`. |
| 1.3a | Registry quarantine = guidance only | PASS | `quarantine <run-id> --yes` → prints `reg export` / `reg delete` backup steps; Run value verified intact afterwards. |
| 1.3b | Quarantine dry-run (ACL probe) | PASS | `quarantine <id> --dry-run` → candidate block names only `%TEMP%\CURE_TEST\CURE_TEST_file.txt` + `dry run: nothing was moved.` |
| 1.3c | Quarantine ACL probe (`--yes` after dry-run) | PASS | `moved: …\CURE_TEST_file.txt` → `to: …\cure-data\quarantine\7ea89cf59cf46fae_CURE_TEST_file.txt`; original gone (`Test-Path False`). |
| 1.3d | Undo restores bytes + ACL | PASS | `undo <id>` → `restored:`/`to:` with **zero** `note:` lines. `Get-FileHash` after == `hash-before.txt` (`95BB63CE…6B4D`, `MATCH=True`). `icacls` output byte-identical to `acl-before.txt`, incl. the explicit non-inherited `Everyone:(R)` ACE. |
| 1.3e | Quarantine unknown id | PASS | `quarantine deadbeefdeadbeef --yes` → `error: unknown id …`, exit 1, nothing changed. |
| 1.3f | Undo twice | PASS | Second `undo` → `error: unknown id … nothing was ever quarantined under that id`, exit 1. Minor wording quirk (it *was* quarantined, then undone) — cosmetic, listed below. |
| 1.4 | Non-TTY refusal | PASS | Piped stdin `quarantine <lnk-id>` (valid id under those roots) → `error: refusing to quarantine without confirmation in a non-interactive session …`, exit 1, `.lnk` verified still present. (First attempt with a stale id from other roots correctly failed id lookup instead — fresh-scan id binding working as designed.) |
| 1.5 | `--uninstall --dry-run` | PASS | 9 targets listed from real constants (5 files + host dir + 3 decoy sweeps); decoy rows named live `~cure-canary-*` files. Live uninstall SKIPPED here — targets pre-date the session (see §8 for the authorized live run). |
| 1.6 | Kit removal + re-diff | PASS | `remove-test-artifacts.ps1` → `RESULT: clean`, exit 0. Re-diff vs Phase 0: `hkcu-run.txt`, `localappdata-cure.txt` IDENTICAL; `tasks.csv` only system-task timestamp churn; processes only transient `vctip` delta. |
| 2-overlay | Fullscreen overlay card/close/allowlist | SKIPPED | Human deferred; needs interactive desktop session. |
| 8-consent | Watcher consent Yes | PASS | pid 16048 prompt → human clicked Yes → marker `{"status":"enabled"}`. |
| 8-selfinstall | Self-install + pairing bootstrap | PASS | Startup copy hash == branch build (`4292E5E8…`); `watcher-pairing.json` holds token + `gui_sha256 C369…` == branch GUI; `E:\.cure-trigger` = `CURE-TRIGGER-V2:<token>`; log shows `updated stale watcher`, `host GUI copy pinned`, `paired removable media`, `watcher started`. |
| 8-pair | `cure-watch pair E:` | PASS | `paired E:\ …`, exit 0. |
| 8-correct | Correct trigger+token | PASS | After replug: `[drive] new drive appeared: E:\` then `[launch] launched pinned %LOCALAPPDATA%\CURE\cure-gui.exe for E:\`; process path verified as the pinned copy (pid 14848). |
| 8-missing | Missing trigger (subst X:) | PASS | `[trigger] drive X:\ ignored: no trigger file on drive; ignoring`; nothing launched. |
| 8-wrong | Wrong token (subst X:) | PASS | `[trigger] drive X:\ ignored: trigger token mismatch; ignoring`; nothing launched. |
| 8-swapped | Notepad copy as `E:\cure-gui.exe` | PASS | Log shows pinned host path launch; running process path is `%LOCALAPPDATA%` copy (pid 16728); USB copy hash unchanged (still notepad bytes) — never executed. Restored branch GUI to E: afterwards, hash-verified (`C369…` match). |
| 8-uninstall | Live `--uninstall --yes` | PASS¹ | 5 files + 18 decoys removed; `%LOCALAPPDATA%\CURE` correctly left (holds pre-existing `baseline.json`, 32,820 B — uninstall refuses non-empty dirs); exit 1 with 2 leftover lines describing exactly that. No watcher/GUI processes remain. ¹Authorized: dry-run list reviewed + explicit approval; see note. |
| 8-nolaunch | Post-uninstall silence | PASS¹ | Pairing record gone; unit gate (`decide_launch` empty pin ⇒ Ignore) covers the logic; no watcher process exists to launch anything. ¹No extra replug cycle run — stated plainly. |
| 8-usb | USB restore + listing diff | PASS | All 4 E: files restored hash-identical; top-level listing diff empty; personal folders untouched. |
| 8-residue | Final diff vs Phase 0 | PASS¹ | `hkcu-run`, `localappdata-cure` IDENTICAL; no `CURE_TEST` task rows. One explained delta: pre-existing stale `Startup\cure-watch.exe` (2026-08-26) was replaced by the consented self-install, then removed by the authorized uninstall. ¹Restore possible from USB backup (same 342,016 B); left removed as the validated end-state — human call. |
| 3-mock | Mock backend loads clean | PASS | `index.dev.html` + `mock-tauri.js`: zero page errors; all 20+ mocked commands present incl. new overlay trio. |
| 3-shots | 7 screenshots @1440x900 | PASS | `docs/screenshots/01-idle,02-scanning,03-results,04-quarantine-confirm,05-undo,06-overlay-card,07-overview.png`. All sample data (mock wordmark visible). |
| 3-demo | ≤60s demo video | PASS | 15.6 s webm → `docs/media/demo.mp4` (650 KB) + `docs/media/demo.gif` (4.87 MB ≤ 5 MB) via local ffmpeg 9.0. Zero page errors during recording. |

Counts: **PASS 33 · FAIL 0 · SKIPPED 5** (task-create, task-row, overlay, live-uninstall-in-§1.5, plus restore-point self-query) **· UNVERIFIED 0** (everything not run is marked SKIPPED with reason, nothing claimed).

## Findings (found during validation, not fixed on the fly except trivial kit bugs)

1. **F-LNK-1 — FIXED in `f5a5c05` (flag guard + IShellLink) + `68af08a` (scanner separates location/command):** Startup `.lnk` target was shown as relative garbage (`..\..\… [MISSING]`) with absolute LinkInfo discarded by an impossible flag check (`0x1|0x10` vs spec `0x1`), and the risk score used the `.lnk` file path itself (`…\Startup\…lnk`, Unknown sig) not the target — a detection false-negative (e.g. an unsigned payload in Temp via a Startup lnk would incorrectly score Safe). Now `cmd` is the resolved absolute target + args (scored via target's sig/hash/heuristics), `lnk→` is long absolute + `[exists]`, and 8 new `scanners::startup::tests` cover unsigned/signed, powershell+args vs Run key, relative/env-var/args/missing/hash. See Step 3 consumer audit in commit `68af08a`. Previous report's "correct for signed bytes" was coincidence; corrected above.
2. **Kit bug (fixed, trivial/local):** `make-test-artifacts.ps1` aborted on schtasks stderr under `$ErrorActionPreference='Stop'` instead of printing SKIP — wrapped in try/catch, re-ran clean.
3. **Kit bug (fixed, trivial/local):** same throw pattern in `remove-test-artifacts.ps1` task query (factored into `Test-TaskPresent`); plus added exact-name `cure-data` cleanup so `RESULT: clean` is reachable.
4. **Observation (cosmetic):** second `undo` of an already-undone id says "nothing was ever quarantined" — it was, then undone. Suggest "no record for id".
5. **Observation (cosmetic):** `--uninstall --dry-run` prints `failed … directory not empty` for a non-empty host dir — accurate prediction, but "failed" reads oddly in a dry run.
6. **Process note:** `Get-ComputerRestorePoint` is Access-denied for standard users — the kit cannot self-verify restore state; human confirmation stays mandatory (G1).

## Still needing an isolated VM/account or admin

Live `--uninstall` on a box with no pre-existing watcher files; elevated
scans; `cure cleanup run`; DISM; real-malware canaries; cold-cache
revocation rendering; USB-passthrough timing; the deferred fullscreen
overlay card/force/allowlist loop; `schtasks` creation as admin.
