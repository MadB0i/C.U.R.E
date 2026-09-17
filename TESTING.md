# TESTING.md — validation procedures (Windows-only)

All procedures below are Windows 10/11-only. The engine's pure logic
(risk scoring, canary state machine, baseline diff, task-XML parsing) is
covered by `cargo test --workspace` on any platform, but every
end-to-end flow here needs Windows: registry autoruns, Authenticode,
`ReadDirectoryChangesW`, drive-letter polling, the Startup folder, and
WebView2.

> Honesty rule: do not claim a test was performed unless it was actually
> performed. Record the outcome (PASS/FAIL + evidence) with each run.

## 0. Unit + lint baseline (any Windows dev machine)

```bat
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Both must be fully green. `gui/` is a separate workspace (own lockfile):
build it with `cargo build --release` inside `gui\src-tauri`.

## 1. Watcher: first-run consent

On first launch `cure-watch.exe` shows a Yes/No message box:

- **Yes** → writes `%APPDATA%\cure-watch-consent.json`
  (`{"status":"enabled"}`), self-installs (see §2), starts watching.
- **No** → writes `{"status":"declined"}`, installs nothing, exits.
  Run again only after deleting the marker (see reset below).

The marker is strict JSON: malformed content, a missing `status` field,
or an unknown value re-asks on next launch (fail-closed — garbage never
enables background watching). Covered by `consent::tests` unit tests.

The prompt also discloses the experimental canary guard (decoy files +
folder monitoring, §5) — enabling the watcher enables that too.

### Reset consent for testing

```bat
del %APPDATA%\cure-watch-consent.json
del "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\cure-watch.exe"
```

Delete both: the marker (to be asked again) and the installed copy (to
verify fresh self-install, not the update path).

## 2. Watcher: self-install / self-update

On every consented start the watcher reconciles
`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\cure-watch.exe`
with the binary actually running:

| State | Behavior | Log line |
|---|---|---|
| No installed copy | copy self there | `[install] installed watcher to …` |
| Byte-identical copy | do nothing (no churn) | `[install] installed copy already up to date at …` |
| Different bytes | replace via temp-file + replacing move; **never** delete-first | `[install] updated stale watcher at …` |
| Replace fails (in use / access denied) | old copy left intact, current process keeps watching | `[install] deferred update of …; old copy left intact` |
| Already running from the installed copy | nothing to do | (no line) |

Verify an update: install an older build, run the new binary from another
folder with consent enabled, confirm the `updated stale watcher` line and
that the Startup copy's bytes now match the new binary. Decision logic is
unit-tested (`self_update::tests`, incl. temp-file replacement tests).

## 3. How to verify the watcher is running

1. Console prints `cure-watch is watching for rescue USBs (Ctrl+C to stop)…`.
2. `%APPDATA%\cure-watch.log` gains `[startup] watcher started (pid …, polling every 1500 ms)`.
3. A `cure-watch.exe` process exists in Task Manager.

If `%APPDATA%` is unset the watcher runs in portable mode (no install, no
durable log) and says so on the console. Logging is best-effort and never
blocks watching.

## 4. Trigger flow (live-fire, clean VM recommended)

Fixtures: `testing/fake-overlay/` — an inert, clearly-labeled fullscreen
window (`CURE TEST FIXTURE — NOT REAL MALWARE`). No file access, no
encryption, no network, no persistence. Review
`testing/fake-overlay/src/main.rs` (~200 lines of plain WinAPI) before
running it.

What you need: clean Windows 10/11 VM (snapshot first), Rust MSVC toolchain
or copied release exes, a USB stick (or `subst` fallback), WebView2
(preinstalled on Win10/11).

Build:

```bat
cargo build --release                        :: cure-watch.exe (+ core/cli)
cargo build --release                        :: in testing\fake-overlay\ -> fake-overlay.exe
cd gui\src-tauri && cargo build --release    :: cure-gui.exe
```

Procedure:

1. **Consent + install.** Run `target\release\cure-watch.exe` once in the
   VM. Answer **Yes** at the consent prompt. Confirm the console prints
   "watching for rescue USBs" and the log shows `[startup]` + `[install]`
   lines (see expected shape below).
2. **Launch the fake overlay.** Run `fake-overlay.exe`. Verify it covers
   the desktop edge-to-edge and its title reads
   `CURE TEST FIXTURE — NOT REAL MALWARE`. Leave it running.
3. **Prepare the rescue drive.** On a real USB:
   ```bat
   copy gui\src-tauri\target\release\cure-gui.exe E:\
   echo CURE-TRIGGER-V1> E:\.cure-trigger
   ```
   No-USB fallback:
   ```bat
   mkdir C:\cure-usb-test
   copy gui\src-tauri\target\release\cure-gui.exe C:\cure-usb-test\
   echo CURE-TRIGGER-V1> C:\cure-usb-test\.cure-trigger
   subst X: C:\cure-usb-test
   ```
   Optional negative control (must be *ignored*):
   ```bat
   mkdir C:\cure-bad-test
   echo not-a-real-trigger> C:\cure-bad-test\.cure-trigger
   subst Y: C:\cure-bad-test
   ```
4. **Attach/insert the drive**, then read `%APPDATA%\cure-watch.log`
   within seconds. Expected shape (UTC):
   ```
   2026-08-23T14:02:11Z [startup] watcher started (pid 4188, polling every 1500 ms)
   2026-08-23T14:02:11Z [install] installed watcher to C:\Users\you\AppData\...\Startup\cure-watch.exe
   2026-08-23T14:05:47Z [drive] new drive appeared: X:\
   2026-08-23T14:05:47Z [trigger] VALID C.U.R.E trigger on X:\; launching GUI
   2026-08-23T14:05:48Z [launch] launched X:\cure-gui.exe
   2026-08-23T14:05:47Z [trigger] invalid/missing trigger on Y:\; ignoring
   ```
   Event vocabulary: `startup`, `install`, `consent`, `drive`, `trigger`,
   `launch`, `launch-error`, `canary`. The log is authoritative even if the
   visual check is ambiguous.
5. **Visual z-order check.** Within ~2 s the C.U.R.E window should appear
   **in front of** the fake overlay, zero clicks (topmost + focus at
   startup, re-surfaced after ~1.2 s; manual launches are normal windows).
   - PASS: scan UI above the red fixture screen.
   - FAIL: only fixture visible → Alt+Tab; if C.U.R.E is behind, record log
     excerpt + Alt+Tab state as a topmost-race bug.
6. **Confirm a real scan.** After ~15 s verify a fresh `baseline.json`
   exists on the drive root with this run's timestamp. The scan now covers
   auto-start services, WMI subscriptions, IFEO debuggers, AppInit DLLs,
   and per-user COM hijacks in addition to Run keys, Startup, and tasks
   (see `cure scan` output: each finding carries its ATT&CK id).
7. **Tear down.**
   ```bat
   taskkill /IM cure-gui.exe /F
   taskkill /IM fake-overlay.exe /F
   taskkill /IM cure-watch.exe /F
   subst X: /D          :: if used
   subst Y: /D          :: if used
   ```
   Roll back the snapshot (or delete the Startup copy + consent marker) so
   the watcher doesn't linger.

Status: VM phases (Setup → PostReboot → OverlayAndUSB) last reported
incomplete (guest aborted); do not mark them passed until re-run.

## 5. Canary guard

Two integrations share one engine (`core::canary`, pure state machine) and
one acquisition loop (`cure_dirwatch::run_dir_guard`):

- **Watcher** (automatic after consent): plants `~cure-canary-*` decoys in
  Desktop/Documents/Downloads, watches them via `ReadDirectoryChangesW`,
  polls for shadow-wipe tools. Alerts → `%APPDATA%\cure-watch.log` lines
  tagged `[canary]` (`[TAMPER]` / `[BURST]` / `[REWRITE]` / `[SHADOW-WIPE]`).
- **GUI** (manual toggle): same decoys + watchers, alerts → `canary-alert`
  Tauri events rendered in the UI.

Test on a scratch folder set (never your real profile if avoidable):

1. Start the guard (answer Yes to watcher consent, or flip the GUI toggle).
2. Confirm 6 `~cure-canary-*` files appear per watched dir.
3. Modify/rename one decoy → expect a `[TAMPER]` log line / UI alert.
4. Create ~10 new files with the same odd extension within seconds →
   expect `[BURST]` / `[REWRITE]` alerts.
5. Stop (kill watcher / GUI toggle off) and delete the decoys.

The guard is **experimental**: it detects decoy tampering and crude
bulk-encryption patterns. Never describe it as full ransomware protection.

## 6. GUI cleanup E2E (real backend, sandboxed)

The GUI ships a self-driving test mode compiled in (`E2E_RUNNER_JS` in
`gui/src-tauri/src/main.rs`, inert unless env-gated). It drives the REAL
webview through: Start Rescue (incl. real overlay dismissal) → results →
footer buttons → cleanup scan → tick first download → confirm dialog →
expect `Freed …` status → write JSON → exit(0). A second mode
(`CURE_E2E_EXIT`) clicks the real Exit button so the harness can assert
the process terminates.

Sandboxed run (no real user data at risk — all scan roots are env-driven):

```bat
:: 1. seed a sandbox (adjust $sb); old installer must be >30 days old.
::    Also drop a HighRisk startup seed so the scan auto-quarantines something:
::    any random-looking name works because the sandbox lives under %TEMP%
::    (drop-zone path + random name = HighRisk):
@echo off> %sb%\startup\xk9q2zv7m1.bat
:: 2. rebuild GUI from HEAD:  cd gui\src-tauri && cargo build --release
:: 3. launch:
set CURE_E2E_CLEANUP=1
set CURE_E2E_OUT=%sb%\e2e-result.json
set USERPROFILE=%sb%\home
set TEMP=%sb%\temp & set TMP=%sb%\temp
set LOCALAPPDATA=%sb%\local
set SystemDrive=Q:
cure-gui.exe --data-dir %sb%\data --startup-root %sb%\startup --tasks-root %sb%\tasks
:: 4. wait for e2e-result.json (up to ~10 min on slow machines), then:
set CURE_E2E_EXIT=1
cure-gui.exe --data-dir %sb%\data --startup-root %sb%\startup --tasks-root %sb%\tasks
:: 5. assert: process observed alive, then gone within ~30 s
```

What PASS looks like (2026-09-16 runs, see `E2E-LOG.md` notes):
`ok:true`, `Freed 3.0 MB — deleted 3 of 4, 1 locked or failed`, the one
failure being a Chromium temp file locked inside the sandboxed TEMP
(os error 32, correctly reported per-item), `downloadsTicked:true`,
`tossSeen:true`, footer messages
(`No scan log yet…` pre-scan, `Quarantine folder opened`,
`Scan log opened` post-scan), `quarantineVerified:true` (seed
auto-quarantined → listed in the Quarantine view → undone through the UI),
`viewsVerified` (all five new views render; posture non-empty; audit shows
the seed), `canaryActive:true` (guard enabled, decoys on disk), and clean
self-exit. The sandboxed scan also exercises the live service/WMI/IFEO/
AppInit/COM enumerators (real machine services appear as findings).

Caveats: overlay dismissal runs against the real desktop (conservative
matcher — topmost + borderless + unsigned + not-own + not-system); the
process scan enumerates real processes (read-only); old-installer deletion
was proven on synthetic seeds only. A clean-VM run is still wanted for
full fidelity.

## 6b. Report export, details, and new-source spot checks

```bat
:: JSON + redacted TXT reports (explicit export; remediates nothing)
cure.exe report --data-dir E:\cure-data
cure.exe report --format json --redact --data-dir E:\cure-data
```

Verify: both files appear in the data dir; the TXT has COVERAGE with
`[CHECKED]` / `[NOT CHECKED]` / `[UNAVAILABLE]` states and never claims
"clean" for an unperformed check; the JSON has `findings`, `coverage`,
`intel_provider` (`DEMO…`), and `redacted:true`; with `--redact` no
username/home path survives (grep the file).

```bat
:: New persistence sources on a live machine (read-only)
cure.exe scan --data-dir E:\cure-data
```

Expect: `windows-service` rows with `T1543.003` (each shows start type,
account, scan-time state as evidence); `wmi-subscription` rows normally
empty on a clean box (the stock SCM Event Log filter/consumer scores
Safe when present); `ifeo-debugger`/`appinit-dlls` normally empty;
`com-hijack` rows for per-user CLSIDs. `cure.exe quarantine <id>` on any
non-file-backed finding must print backup-first manual guidance — never
relocate anything. Scheduled-task enumeration needs read access to
`C:\Windows\System32\Tasks`; unelevated runs may report 0 tasks (a
coverage honesty note, not a bug — elevate to compare).

Shortcut/task forensics: any `.lnk` finding prints its resolved target
(`lnk→ … [exists|MISSING]`) in `cure scan`; the GUI Startup Audit drawer
shows target/arguments/workdir, full task actions/author/run-level/
triggers, signature state, and publisher on expand, plus an
`Open location` button (Explorer select, strict id-based).

## 6c. Post-login incident investigation

```bat
:: Headless 30-second observation (read-only; installs/modifies nothing)
cure.exe incident --duration 30 --data-dir E:\cure-data
```

What it does: loads current persistence findings, watches process starts/
exits (WMI creation events + 500 ms snapshot diff) and window open/close
(titles + classes only — no screenshots, no keystrokes, no contents) for
the window, correlates by deterministic levels (DIRECT path match, STRONG
command/timing, PARTIAL name-only, WEAK proximity, NONE), prints the
timeline + correlations + verdict, and writes
`cure-incident-<stamp>.txt` with the full section.

Verdicts: CAUSE IDENTIFIED (DIRECT + transient window, same process) ·
STRONG CORRELATION · REVIEW REQUIRED · NO DIRECT EVIDENCE · INSUFFICIENT
OBSERVATION (failed/unavailable observation — never a clean claim).
**Correlation does not by itself establish malicious intent.**

GUI path: Scan Center → Incident view → pick 15/30/60/120 s → Start.
Progress shows poll/process/window counts (real numbers, no decoration);
timeline, correlations with evidence, transient-window panel, and TXT/JSON
export follow automatically. `CURE_E2E_CLEANUP` also drives a 15 s live
observation and asserts a usable result.

Reboot note: there is deliberately NO reboot-and-capture flow — that would
require installing persistence, which C.U.R.E. will not do silently or
otherwise. For true post-login capture, run `cure-gui.exe` (portable) or
`cure.exe incident` manually within a minute of logging in.

Privileges: observation works unelevated (verified: WMI creation events
deliver as standard user on Win10/11). Command lines are fetched only for
correlated processes (bounded); exports may contain third-party
command-line arguments — handle exported files accordingly.

## 6d. V2.3 validation procedures (all isolated, nothing persists)

Skipped-access honesty: run `cure.exe scan` unelevated and confirm a
`skipped: N task files (inaccessible — elevate to compare)` line when
system tasks deny reads (VERIFIED: 1 on the dev box). Re-run elevated to
compare — counts, not verdicts, are the assertion.

Scoped undo: quarantine any startup finding, then
`cure.exe undo <id> --tasks-root <other> --startup-root <other>` (or the
GUI equivalent with moved roots) must refuse with PermissionDenied and
leave the record intact; normal undo still restores. Unit tests cover
allow/deny/sibling-prefix (`C:\foo` must not authorize `C:\foobar`).

Watcher live-fire without touching the real machine: point `%APPDATA%`
at a temp dir, pre-seed `cure-watch-consent.json` (`{"status":"enabled"}`,
skips the modal), run `cure-watch.exe`, confirm the Startup copy + log
lines land under the temp dir only, `subst` a folder with a valid
trigger plus any survivable `.exe` renamed to `cure-gui.exe`, and confirm
`[drive]` → `[trigger] VALID` → `[launch]` plus a spawned process; then
kill everything, unsubst, delete the temp dir, and confirm the real
Startup folder is untouched (VERIFIED 2026-09-16; note: a copied
notepad.exe self-terminates outside System32 — use a cmd copy payload).

Short-lived processes: `cargo test -p cure_core live_short_lived` spawns
a uniquely-named cmd copy (~4 s) plus the fake-overlay fixture (~1.2 s,
requires `cargo build` in `testing/fake-overlay` first) inside a 4 s
observation and asserts exact-path observation, title metadata, and
transient classification (VERIFIED). Instant `cmd /C exit` is
opportunistic only — sub-500 ms detection is not guaranteed (use
`timeout.exe` never headless: it needs stdin; `ping` dwell is used).

Canary real-FS: `cargo test -p cure_dirwatch live_` runs the shared guard
against tempdirs only — decoy modify/rename/delete raise tamper, 20-file
noise stays tamper-free, restart re-alerts, shutdown is prompt (VERIFIED).
The suite never touches real documents; the UI/CLI keep the
EXPERIMENTAL label (asserted in docs, not code).

Self-update failures: `cargo test -p cure_watch self_update` covers
locked-target (old bytes intact, no tmp residue), stale-tmp overwrite,
same-file short-circuit, and byte-compare decisions (VERIFIED).

`gui/devtools/*.mjs` drive `gui/dist/index.dev.html` + `mock-tauri.js`
(canned backend) for pixel/DOM assertions: `verify.mjs`, `chipcheck.mjs`,
`cleanupcheck.mjs`, `pixelcheck.mjs`, `canarycheck.mjs`, plus screenshot
tools (`shots.mjs`, `sweepshots.mjs`, …).

- They assert **UI behavior**, not backend correctness — a green
  `cleanupcheck` does not prove the real Tauri plumbing (that's §6's job).
- Screenshot output defaults to `gui/dev-screenshots/`; set
  `CURE_SHOTS_DIR=<empty dir>` to keep runs from dirtying the repo. Only
  the five README-referenced PNGs are tracked; copy deliberate asset
  updates back by hand.
- Needs `npm install` once in `gui/devtools` (Playwright).

## Pass criteria (watcher live-fire)

| # | Criterion | Evidence |
|---|---|---|
| 1 | Consent asked once, persisted, honored | marker file + `[consent]` lines |
| 2 | Watcher self-installed and logged startup | `[startup]`/`[install]` lines |
| 3 | New drive detected while overlay covered screen | `[drive]` line |
| 4 | Trigger validated (and invalid trigger rejected) | two `[trigger]` lines |
| 5 | GUI spawned successfully | `[launch]` line |
| 6 | GUI visible ABOVE the topmost fixture, zero clicks | screenshot / eyes |
| 7 | Scan completed | fresh `baseline.json` on drive |

## Troubleshooting

- **No log file at all** — `%APPDATA%` unset? Logging is best-effort; the
  console mirrors the same events.
- **Consent re-asks every launch** — marker contains malformed JSON or an
  unknown value (fail-closed by design); delete it to start clean.
- **`[launch-error]` no cure-gui found** — exe wasn't on the drive root or
  beside the watcher.
- **GUI behind the overlay** — genuine finding; capture log + Alt+Tab state.
- **GUI never opens, no error** — WebView2 runtime missing in the VM.
- **`deferred update … old copy left intact`** — another watcher instance is
  running from the Startup copy; stop it and restart once to complete the
  update.
