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

## 7. Overlay dismissal (dummy window)

Background (exact criteria, `gui/src-tauri/src/main.rs:1426-1479` +
`core/src/overlay.rs:51-56`): a **visible** window is closed only if
**all** hold — `WS_EX_TOPMOST` set (`:1431`), `WS_CAPTION` unset
(`:1432`), owner binary resolvable and **not** `ValidSigned` (`:1467`;
`Unknown`/`Invalid`/`Unsigned`/`ValidRevocationUnknown` all match), not
cure-gui's own window, not under `%WINDIR%` (`is_under_windows_dir`,
`:1360-1366`). Close sequence: `PostMessageW(WM_CLOSE)`, 500 ms wait,
then `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess` (`:1486-1516`).
Trigger: **only** the Start Rescue click (`gui/dist/app.js:4394-4400`
invokes `dismiss_overlays`); nothing auto-runs it — verified single call
site.

| | |
|---|---|
| Do | `.\CURE_TEST_overlay.ps1 -Minutes 5` (compiles unsigned `CURE_TEST_overlay.exe` via `csc.exe` if needed; refuses to launch if it ever reports signed). Confirm the small topmost window appears. |
| Do | In cure-gui click **Start Rescue**. |
| CURE should do | The dummy closes **gracefully** (its `DefWindowProc` handles `WM_CLOSE`; the terminate fallback should be unnecessary). Report line names the window; `(terminated)` must NOT appear. |
| Screenshot | Before (dummy visible) / after (closed + report line) — README-worthy. |
| Cleanup | The window self-closes after N minutes regardless; `remove-test-artifacts.ps1` deletes the exe. |

**Finding — FP surface (from code, UNVERIFIED live):** the criteria hit
*any* visible + topmost + borderless + non-valid-signed + non-Windows-dir
window. Legitimate examples: unsigned indie games in borderless-windowed
mode, AutoHotkey/custom-utility topmost palettes, unsigned installer
splashes, unsigned kiosk apps. If such a window ignores `WM_CLOSE` for
500 ms, its **process is terminated** (`TerminateProcess(handle, 1)`) —
potential data loss. Mitigations in code: AND-of-four rule, own/system
exclusions, unattributable-window skip (`:1458`, `:1461` `continue`), and
no auto-run (explicit Start Rescue click only). Test negative control:
a topmost borderless window owned by a *signed* binary (e.g. PowerShell-
hosted) must be left alone.

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

**Finding: no uninstall command exists in the code** (`git grep
"uninstall" -- '*.rs' '*.ps1'` returns nothing). Removal is fully manual —
delete exactly these (paths from `self_update.rs:46-50`,
`consent.rs:55-57`, `logger.rs:9`, `pairing.rs:228-241`) after killing the
process:

```bat
taskkill /IM cure-watch.exe /F
del "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\cure-watch.exe"
del "%APPDATA%\cure-watch-consent.json"
del "%APPDATA%\cure-watch.log"
rmdir /s "%LOCALAPPDATA%\CURE"
del "%USERPROFILE%\Desktop\~cure-canary-*" "%USERPROFILE%\Documents\~cure-canary-*" "%USERPROFILE%\Downloads\~cure-canary-*"
```

| | |
|---|---|
| Do | Run the commands above one by one. |
| CURE should do | N/A — verify yourself: `Get-Process cure-watch` empty; all five paths gone; decoys gone from the three folders (names `~cure-canary-*`, `core/src/canary.rs:15-24`; planter never overwrites, `winwatch/src/lib.rs:52-54`). Re-run `cure-watch.exe` → consent prompt reappears (marker gone = `AskNow`, `consent.rs:30-43`). |
| Screenshot | Before/after dir listings (README-worthy for the "reversible" claim). |
| Cleanup | N/A (this IS cleanup). |

## 10. Final cleanup

| | |
|---|---|
| Do | `.\remove-test-artifacts.ps1` — expect `RESULT: clean`. Any `LEFTOVERS` line is a test-kit bug: report it. |
| Do | Re-run `cure.exe --data-dir … scan` and confirm zero `CURE_TEST_` rows. |
| Screenshot | `RESULT: clean` output. |

## What this kit deliberately does NOT cover (VM-only)

`cure cleanup run`, DISM, elevated scans, real-malware canaries, cold-cache
revocation, USB-passthrough timing — see `AUDIT.md` "Needs VM validation".
