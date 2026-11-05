# C.U.R.E 0.2.0 — Release draft (paste into the GitHub Release when publishing)

> Prepared for the repository owner. Publishing (push, tag, GitHub Release)
> is deliberately left as a human click — nothing in this folder is uploaded
> by automation.

## What to attach

| File | What it is |
|---|---|
| `CURE-portable.exe` | The desktop Evidence Console, single portable executable (frontend embedded — no install needed). |
| `cure-cli.exe` | The `cure` command-line scanner (scan / report / incident / agents / change / recorder / casefile). |
| `SHA256SUMS.txt` | SHA-256 checksums for both files. |

## Verify before running (recommended for your users)

```powershell
# Compare against SHA256SUMS.txt
Get-FileHash .\CURE-portable.exe -Algorithm SHA256
```

## Windows SmartScreen (unsigned binary)

The binaries are not code-signed yet, so Windows shows
**"Windows protected your PC"** on first run. That is expected for any
unsigned tool. To run:

1. Click **More info**.
2. Click **Run anyway**.

Publishing users can verify the download by checking the SHA-256 above —
if it matches, the file is exactly the one built from this repository.
(A code-signing certificate removes this prompt entirely; that is an
owner purchase decision, not a code change.)

## Suggested release text

**C.U.R.E — Evidence-first Windows security console**

- **Ten-surface persistence scan** — the autorun locations Windows Defender
  does not surface, scored with ATT&CK mapping: Registry Run/RunOnce, Startup
  folders, Scheduled Tasks (arguments scored), Services, WMI subscriptions,
  IFEO/AppInit, COM hijacks, Winlogon & Active Setup, running processes,
  ransom indicators.
- **Move Bot** — one click moves space-hungry folders from C: to D:; copied,
  verified bit-for-bit, Windows switched over, undo-able. Nothing is deleted
  until the copy is verified.
- **Quarantine with scoped undo, canary tripwire, login observation,
  Agent Radar, Flight Recorder, storage forensics** — local only, zero
  telemetry, no auto-scan, move-never-delete.

Requirements: Windows 10/11 x64, WebView2 Runtime (preinstalled on modern
Windows). Optional: [Rust](https://rustup.rs) to build from source.

Full verification commands and the honest limitations list are in the
repository README.
