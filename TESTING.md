# TESTING.md — cure-watch live-fire validation

End-to-end proof that **cure-watch**, already running on a machine, detects a
rescue USB and launches the GUI **even while a fullscreen topmost overlay is
covering the screen** — and that the GUI lands *on top* of that overlay.

Everything here runs against test fixtures only:

- `testing/fake-overlay/` — an inert, clearly-labeled fullscreen window
  (`CURE TEST FIXTURE — NOT REAL MALWARE`). It renders static text and does
  nothing else: no file access, no encryption, no network, no persistence.
  Review `testing/fake-overlay/src/main.rs` before running it; it's ~200 lines
  of plain WinAPI window creation.

## What you need

- A clean Windows 10/11 VM (snapshot first so you can roll back).
- Rust toolchain (MSVC) in the VM, or copy built exes from the host.
- A USB stick — or, if the VM has no USB passthrough, any folder + `subst`
  (procedure below covers both; `subst` drives are indistinguishable to
  cure-watch because it polls drive letters via `Path::exists()`).
- WebView2 runtime (preinstalled on Win10/11; needed by cure-gui).

## Build

```bat
cargo build --release                        :: cure-watch.exe (+ core/cli)
cargo build --release                        :: in testing\fake-overlay\ -> fake-overlay.exe
cd gui\src-tauri && cargo build --release    :: cure-gui.exe
```

## Procedure

1. **Install & start the watcher.** In the VM run
   `target\release\cure-watch.exe` once. It copies itself into
   `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\` and keeps
   running. Confirm the console prints "watching for rescue USBs".

2. **Confirm durable logging started.** Open `%APPDATA%\cure-watch.log`. You
   should see a `[startup]` line and an `[install]` line (see expected output
   below).

3. **Launch the fake overlay.** Run
   `testing\fake-overlay\target\release\fake-overlay.exe`. Verify it covers
   the entire desktop edge-to-edge, including the taskbar, and that its title
   reads `CURE TEST FIXTURE — NOT REAL MALWARE` (visible in taskbar/Alt+Tab).
   Leave it running.

4. **Prepare the rescue drive.** On a real USB:
   ```bat
   copy gui\src-tauri\target\release\cure-gui.exe E:\
   echo CURE-TRIGGER-V1> E:\.cure-trigger
   ```
   No-USB fallback inside the VM (adjust paths):
   ```bat
   mkdir C:\cure-usb-test
   copy gui\src-tauri\target\release\cure-gui.exe C:\cure-usb-test\
   echo CURE-TRIGGER-V1> C:\cure-usb-test\.cure-trigger
   subst X: C:\cure-usb-test
   ```
   Optional negative control (watcher must *ignore* this one):
   ```bat
   mkdir C:\cure-bad-test
   echo not-a-real-trigger> C:\cure-bad-test\.cure-trigger
   subst Y: C:\cure-bad-test
   ```

5. **Attach/insert the drive** (plug in the USB, or run the `subst` commands).

6. **Read the log within a few seconds.** Check `%APPDATA%\cure-watch.log`
   for the detection sequence. Expected shape (UTC timestamps):
   ```
   2026-08-23T14:02:11Z [startup] watcher started (pid 4188, polling every 1500 ms)
   2026-08-23T14:02:11Z [install] installed watcher to C:\Users\you\AppData\...\Startup\cure-watch.exe
   2026-08-23T14:05:47Z [drive] new drive appeared: X:\
   2026-08-23T14:05:47Z [drive] new drive appeared: Y:\
   2026-08-23T14:05:47Z [trigger] VALID C.U.R.E trigger on X:\; launching GUI
   2026-08-23T14:05:48Z [launch] launched X:\cure-gui.exe
   2026-08-23T14:05:47Z [trigger] invalid/missing trigger on Y:\; ignoring
   ```
   Event vocabulary: `startup`, `install`, `drive`, `trigger`, `launch`,
   `launch-error`. If anything fails (GUI missing, spawn error), the reason is
   in `launch-error` lines — this log is authoritative even if the visual
   check is ambiguous.

7. **Visual z-order check (the whole point).** Within ~2 seconds of insertion,
   the C.U.R.E scan window should appear **in front of the fake overlay**
   without any interaction. The auto-launched instance intentionally requests
   topmost + focus at startup and again after ~1.2 s, then stays topmost for
   the session (Defender-style surfacing; manual double-click launches are
   unaffected and behave like normal windows).
   - PASS: scan UI visible above the red TEST FIXTURE screen.
   - FAIL: only the fixture visible → press Alt+Tab; if C.U.R.E is behind the
     overlay, record it as a bug (topmost race) with the log excerpt.

8. **Confirm a real scan happened.** After ~15 s the GUI finishes its animated
   pass; verify a fresh `baseline.json` exists on the drive root (`X:\baseline.json`
   / `E:\baseline.json`) and that its timestamp matches this run. That proves
   the launched GUI actually completed work, not just opened a window.

9. **Tear down.**
   ```bat
   taskkill /IM cure-gui.exe /F
   taskkill /IM fake-overlay.exe /F
   taskkill /IM cure-watch.exe /F
   subst X: /D          :: if used
   subst Y: /D          :: if used
   ```
   Roll back the VM snapshot (or delete the Startup-folder copy of
   cure-watch.exe manually) so the watcher doesn't linger in the VM.

## Pass criteria

| # | Criterion | Evidence |
|---|-----------|----------|
| 1 | Watcher self-installed and logged startup | `[startup]`/`[install]` lines |
| 2 | New drive detected while overlay covered screen | `[drive]` line |
| 3 | Trigger validated (and invalid trigger rejected) | two `[trigger]` lines |
| 4 | GUI spawned successfully | `[launch]` line |
| 5 | GUI visible ABOVE the topmost fixture, zero clicks | screenshot / eyes |
| 6 | Scan completed | fresh `baseline.json` on drive |

## Troubleshooting

- **No log file at all** — `%APPDATA%` unset? Logging is best-effort and never
  blocks the watcher; the console output mirrors the same events.
- **`[launch-error]` no cure-gui found** — exe wasn't copied to the drive root
  before attachment, and none sits next to cure-watch.exe either.
- **GUI opens but behind the overlay** — genuine finding; capture the log
  excerpt + Alt+Tab state and report it. Known suspects: focus-stealing
  prevention timing (the 1.2 s re-surface pass exists to cover this).
- **GUI never opens, no error** — WebView2 runtime missing in the VM.
