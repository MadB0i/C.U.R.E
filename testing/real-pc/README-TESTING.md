# Real-PC validation checklist (manual, normal user, no admin)

Companion kit: `make-test-artifacts.ps1`, `remove-test-artifacts.ps1`,
`CURE_TEST_overlay.ps1` (this folder). Everything the kit creates lives in
the `CURE_TEST_` namespace (`%TEMP%\CURE_TEST`, one HKCU Run value, one
Startup shortcut, one scheduled task); removal is verified item-by-item.

Rule: run every kit script with `-DryRun` first. Anything this doc states
about CURE's behavior comes from the code (paths below); anything not
verified on a live box is marked UNVERIFIED.

## 0. Prerequisites & safety

| | |
|---|---|
| Do | Build release binaries: `cargo build --release -p cure_cli -p cure_watch` (repo root → `target\release\cure.exe`, `target\release\cure-watch.exe`) and `cargo build --release` inside `gui\src-tauri` (`→ cure-gui.exe`). Binary names verified in `cli/Cargo.toml` (`[[bin]] name = "cure"`), `watch/Cargo.toml`, `gui/src-tauri/Cargo.toml`. |
| Do | Open a **non-elevated** PowerShell. Optional but recommended: VM snapshot first. |
| Do | `make-test-artifacts.ps1 -DryRun`, read the plan, then run it for real. |
| CURE should do | N/A (setup step). |
| Screenshot | — |
| Cleanup | Step 10. |

Use `--data-dir %TEMP%\CURE_TEST\cure-data` on every `cure` invocation below
so `baseline.json` and `quarantine/` stay inside the test dir. (Default data
dir is the folder holding `cure.exe` — `default_data_dir`,
`cli/src/main.rs:156`.)

## 1. Artifacts exist

| | |
|---|---|
| Do | `.\make-test-artifacts.ps1` and confirm each printed line. |
| CURE should do | N/A. Verify yourself: `%TEMP%\CURE_TEST\CURE_TEST_helper.exe` exists; `Get-ItemProperty HKCU:\…\Run -Name CURE_TEST_run`; Startup `CURE_TEST_startup.lnk` exists; `schtasks /Query /TN \CURE_TEST_task` (see 2b). |
| Screenshot | — |
| Cleanup | Step 10. |

## 2. Scan detects all three persistence items

| | |
|---|---|
| Do | `cure.exe --data-dir %TEMP%\CURE_TEST\cure-data scan` |
| CURE should do | List three rows: `CURE_TEST_run` (registry-run source, `HKCU\…\CurrentVersion\Run` — scanner reads `Run`+`RunOnce`, `core/src/scanners/registry.rs:9-10`), `CURE_TEST_startup.lnk` (startup-folder), `CURE_TEST_task` (scheduled-task, if created). Expect risk **Safe/VALID-ish**: the helper is a byte copy of signed notepad, so detection ≠ flagging. A HIGH here would be a false positive worth reporting. |
| Screenshot | Full scan output showing the three `CURE_TEST_` rows (README-worthy). |
| Cleanup | Step 10 (artifacts stay until step 9). |

### 2b. Scheduled task is conditional (standard user)

`make-test-artifacts.ps1` attempts `schtasks /Create … /SC ONLOGON` and
prints SKIP when denied (the Task Scheduler root folder normally requires
elevation). If SKIP: the scan-checklist item for `CURE_TEST_task` becomes
"absent, correctly not listed". If creation succeeded (elevated token),
verify the task row appears. UNVERIFIED live: which outcome a given box
produces — record yours.

## 3. Registry item is guidance-only (never auto-moved)

| | |
|---|---|
| Do | `cure.exe --data-dir … quarantine <id-of-CURE_TEST_run>` (answer `y`). |
| CURE should do | Print manual backup/remove guidance (`cli/src/main.rs:649 print_manual_guidance`, registry-run arm prints `reg export` / `reg delete` steps) and move **nothing**: `is_file_backed()` is false for `RegistryRun` (`core/src/model.rs:35-44`). Verify the Run value still exists afterwards. |
| Screenshot | Guidance output (shows "evidence before remediation"). |
| Cleanup | Remove the Run value in step 10 (script does it). |

## 4. Quarantine the Startup shortcut

| | |
|---|---|
| Do | `cure.exe --data-dir … quarantine <id-of-CURE_TEST_startup.lnk>` (answer `y` at the `Quarantine this file?` prompt, `cli/src/main.rs:601`). |
| CURE should do | `moved: …\Startup\CURE_TEST_startup.lnk` → `to: <data_dir>\quarantine\<id>_CURE_TEST_startup.lnk`; prints `restore anytime with: cure undo <id>`. `records.json` holds a `Committed` record with size+sha256+security snapshot. The `.lnk` is gone from Startup. |
| Screenshot | The `moved:`/`to:` receipt lines (README-worthy). |
| Cleanup | Step 5 restores it; step 10 removes it. |

## 5. Undo restores bytes (shortcut)

| | |
|---|---|
| Do | `cure.exe --data-dir … undo <id>`; then `Get-FileHash` the restored `.lnk` and compare with the pre-quarantine hash if recorded. |
| CURE should do | `restored:` + `to:` lines, plus any `note:` fidelity lines from `security_notes` (`cli/src/main.rs:723-728`). Undo is scoped to startup/task roots (`cmd_undo` roots at `cli/src/main.rs:720-721`) — restore outside them is refused. |
| Screenshot | Undo output (with zero notes = full-fidelity restore). |
| Cleanup | Step 10. |

## 6. ACL roundtrip on the TEMP test file (via `--startup-root` override)

Background: only `StartupFolder`/`ScheduledTask` locations are
quarantinable, so the `%TEMP%\CURE_TEST\CURE_TEST_file.txt` probe is
reached through the override flag (`--startup-root`, `cli/src/main.rs:41`).

| | |
|---|---|
| Do | `cure.exe --data-dir … --startup-root %TEMP%\CURE_TEST scan` → note the file's id. `icacls` output was saved to `CURE_TEST_acl-before.txt` and the hash to `CURE_TEST_hash-before.txt` by the setup script. |
| Do | `cure.exe --data-dir … --startup-root %TEMP%\CURE_TEST quarantine <id>` (answer `y`), then `cure.exe --data-dir … --startup-root %TEMP%\CURE_TEST undo <id>`. The override root keeps the restore in scope. |
| CURE should do | Move + restore with `note:` lines only if something could not be put back (`core/src/acl.rs` restores owner/group/DACL-as-SDDL, attributes, timestamps; failures become notes, never silent). |
| Check | `Get-FileHash` equals `hash-before.txt` (must match exactly). `icacls CURE_TEST_file.txt` ACE lines match `acl-before.txt`; **control-flag-only differences (e.g. an added `AI` inherited flag) are acceptable** — documented in `core/src/acl.rs:24` (Windows normalizes DACL control bits on write; owner/group/ACEs round-trip exactly). |
| Screenshot | Before/after `icacls` diff (README-worthy fidelity proof). |
| Cleanup | Step 10. |

## 7. Overlay review (dummy window)

Background (exact criteria, `gui/src-tauri/src/main.rs` candidate
collection + `core/src/overlay.rs` matcher): a **visible** window is
*shown for confirmation* only if **all** hold — `WS_EX_TOPMOST` set,
`WS_CAPTION` unset, owner binary resolvable (unattributable windows are
skipped and can never match) and **not** `ValidSigned`, not cure-gui's own
window, not under `%WINDIR%`, covering ≥ **90% of its monitor**
(`OVERLAY_COVERAGE_THRESHOLD`, `core/src/overlay.rs`), and not on the
user's path+hash allowlist (`overlay-allowlist.json` in the app data dir).
Trigger: **only** the Start Rescue click (invokes `list_overlay_candidates`;
verified call sites in `gui/dist/app.js`). **Nothing closes without a
per-window click**: Close = graceful `WM_CLOSE` only
(`close_overlay_window(hwnd, force=false)`); process termination happens
only through the explicit per-window Force-close button
(`force=true` → `TerminateProcess`, never an automatic fallback).

| | |
|---|---|
| Do | `.\CURE_TEST_overlay.ps1 -Minutes 5` (compiles unsigned `CURE_TEST_overlay.exe` via `csc.exe` if needed; refuses to launch if it ever reports signed). Confirm the small topmost window appears. |
| Do | In cure-gui click **Start Rescue**. |
| CURE should do | A review card appears (process, full path, PID, signature state, size, coverage %) with **Close window** / **Force close** / **Don't ask again for this app** buttons, plus **Continue scan**. NOTE: the kit dummy is small (~480×200, <25% coverage), so under the 90% rule it is correctly **NOT listed** — this itself is the coverage-gate check. For a positive match use the repo's fullscreen fixture instead: build + run `testing\fake-overlay` (fullscreen, borderless, topmost, unsigned), then Start Rescue and expect a card for it. |
| Do (positive) | Click **Close window** on the fake-overlay card. |
| CURE should do | Graceful close (fixture handles `WM_CLOSE`); card reports closed, `(terminated)` must NOT appear. |
| Do (allowlist loop) | Re-run the overlay, click **Don't ask again for this app** on its card, close it, re-run it, Start Rescue again. |
| CURE should do | No card this time (`overlay-allowlist.json` gained a path+hash entry); delete that file to reset. |
| Screenshot | Card with PID/signature/coverage (README-worthy); closed line without `(terminated)`. |
| Cleanup | Windows self-close after N minutes regardless; `remove-test-artifacts.ps1` deletes the kit exe (fake-overlay is repo tooling, remove manually). |

**Known limits (honest note, kept):** a *fullscreen* unsigned borderless
topmost window (unsigned indie game, AHK utility, installer splash) is
still *shown* — a human must judge it. Force close terminates the owning
process on explicit click only. Signed binaries and `%WINDIR%` owners are
never listed; unattributable windows are skipped. All UNVERIFIED live.

## 8. Watcher token flow (needs a spare USB or `subst` drive)

Background (from code): pairing record at
`%LOCALAPPDATA%\CURE\watcher-pairing.json` + pinned copy
`%LOCALAPPDATA%\CURE\cure-gui.exe` (`watch/src/pairing.rs:44-48,228-241`);
stamp with `cure-watch pair E:` (exit 0/1/2, `watch/src/main.rs:42-87`);
trigger file `<drive>:\.cure-trigger` = `CURE-TRIGGER-V2:<64-hex>\n`
(`pairing.rs:34-37,91`, max 256 bytes). Launches ONLY the pinned copy via
`--data-dir <drive>` (`main.rs:454-457`). Ignore reasons are log-only
(`%APPDATA%\cure-watch.log`, `trigger` event): `no trigger file on
drive; ignoring`, `malformed trigger file; ignoring`,
`trigger token mismatch; ignoring`, plus `host GUI copy …` variants
(`pairing.rs:166-191`). **Consent first**: first launch prompts Yes/No;
Yes writes `%APPDATA%\cure-watch-consent.json` and self-installs
`cure-watch.exe` into `%APPDATA%\…\Startup` (`main.rs:111-116,222-234`).

| Case | Do | CURE should do | Screenshot |
|---|---|---|---|
| Correct USB | `cure-watch pair E:` on a USB, reinsert while watcher runs | Console `drive appeared`, log `launched pinned … for E:\`; pinned `cure-gui.exe` process starts | Log excerpt + GUI window |
| Missing trigger | Insert a drive with no `.cure-trigger` | Silent on console; log `drive E:\ ignored: no trigger file on drive; ignoring` | Log excerpt |
| Wrong token | Write `CURE-TRIGGER-V2:` + 64 other hex chars to `.cure-trigger` | Silent; log `… token mismatch; ignoring`; nothing launches | Log excerpt |
| Swapped exe on USB | Valid token + attacker-style `cure-gui.exe` dropped on the USB | Pinned host copy launches (log shows the `%LOCALAPPDATA%` path); USB exe never executes | Log line with host path (README-worthy) |
| Cleanup (each case) | `taskkill /IM cure-gui.exe /F`, delete test trigger files | No residue on test media | — |

UNVERIFIED live: all five rows (needs watcher + media on a live box).

## 9. cure-watch removal check

FIXED (was: no uninstall command): `cure-watch --uninstall` now exists
(`watch/src/uninstall.rs`; derived from the same constants self-install
uses, unit-tested against drift). Dry-run first, then run it:

```bat
cure-watch --uninstall --dry-run
cure-watch --uninstall
```

| | |
|---|---|
| CURE should do | List the 9 targets (5 files + host dir + 3 decoy sweeps), ask `[y/N]` (refuses without `--yes` when non-interactive), remove, then print `clean` or `leftovers` with exit 0/1. Reparse points are never followed. After uninstall, reinserting a paired USB must launch nothing (no pairing record → pure gate ignores; log `arrival ignored: no pairing record`). |
| Verify yourself | `Get-Process cure-watch` empty; `%APPDATA%\…\Startup\cure-watch.exe`, `%APPDATA%\cure-watch-consent.json`, `%APPDATA%\cure-watch.log`, `%LOCALAPPDATA%\CURE\` gone; `~cure-canary-*` gone from Desktop/Documents/Downloads. Re-run `cure-watch.exe` → consent prompt reappears (marker gone = `AskNow`, `consent.rs:30-43`). Manual fallback (same paths, from `self_update.rs:46-50`, `consent.rs:55-57`, `logger.rs:9`, `pairing.rs:228-241`): |
| | `taskkill /IM cure-watch.exe /F` + delete the five paths above. |
| Screenshot | `clean` output (README-worthy for the "reversible" claim). |
| Cleanup | N/A (this IS cleanup). |

## 10. Final cleanup

| | |
|---|---|
| Do | `.\remove-test-artifacts.ps1` — expect `RESULT: clean`. Any `LEFTOVERS` line is a test-kit bug: report it. |
| Do | Re-run `cure.exe --data-dir … scan` and confirm zero `CURE_TEST_` rows. |
| Screenshot | `RESULT: clean` output. |

## Needs manual validation (UNVERIFIED live — record outcomes here)

- `cure cleanup run`, DISM (incl. the second prompt), elevated scans.
- Real-malware canaries (kit stays synthetic by design), cold-cache
  revocation rendering, USB-passthrough trigger timing.
- Watcher: consent Yes/No, self-install, all five §8 USB rows, `--uninstall`
  incl. locked-file leftovers, post-uninstall no-launch.
- Overlay: fullscreen fixture card, graceful Close, explicit Force close on
  the kit dummy, allowlist loop, signed-owner negative control.
- Quarantine/undo of the ACL probe (hash-exact, ACE-level compare).
- `remove-test-artifacts.ps1` ending `RESULT: clean` on a fully-built box.
