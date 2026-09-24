# C.U.R.E. — Clean USB Rescue Engine

[![CI](https://github.com/MadB0i/C.U.R.E/actions/workflows/ci.yml/badge.svg)](https://github.com/MadB0i/C.U.R.E/actions/workflows/ci.yml)

A local-first Windows security and forensic diagnostic tool focused on
evidence-based detection, investigation, quarantine and cleanup.

## Overview

C.U.R.E. inspects how software persists on a Windows machine — autoruns,
startup folders, scheduled tasks, services, WMI, and related mechanisms —
and risk-scores every finding with its evidence attached.

It is evidence-first: nothing is remediated on the basis of a score
alone. Detections are shown with their source, target, reason, and
available actions, and the operator decides what happens next.

The architecture is local-first. All scanning, scoring, quarantine, and
reporting happen on the machine. There is no telemetry, no account, and
no network threat lookup.

Destructive actions always require explicit confirmation, and there is
no automatic destructive remediation: quarantine is a recorded move with
a scoped undo, never a silent delete.

## Features

- Startup persistence auditing
- Services / scheduled tasks / WMI / IFEO / AppInit / COM inspection
- Process Sentinel
- Post-login incident observation and correlation
- Evidence-oriented scan results
- Quarantine with scoped undo
- Disk cleanup with explicit confirmation
- Event logging
- Experimental Canary Guard
- Local application data storage
- Accessible futuristic desktop UI

C.U.R.E. is a diagnostic instrument, not an antivirus: no guaranteed
malware detection, no real-time protection, no cloud threat
intelligence, and no automatic malware removal.

## Architecture

- Rust security/diagnostic core (`core`, `cli`)
- Tauri v2 desktop GUI (`gui/src-tauri`, separate workspace)
- Separate watcher (`watch`) and Canary components
- Local-first: application data lives in an app-owned directory, no telemetry

## Security Principles

- Read-only detection by default
- Explicit confirmation for destructive actions
- Evidence before remediation
- No telemetry
- No credential extraction
- No keylogging
- No screenshots
- No process/DLL injection
- Canary Guard is experimental, not a production antivirus feature

## Status

V4 — released / actively developed.

Test counts are not hardcoded here (they move with every fix): the CI badge
above is authoritative. Reproduce locally with the commands under Build
(root workspace, then GUI workspace). No production or security guarantees
beyond what the test suite and reports verify.

## Security model / threat model

What is trusted:

- The rescue USB **you prepared** and the host-installed copies it pins on
  first consented run (`%LOCALAPPDATA%\CURE\cure-gui.exe`, pinned by
  SHA-256; the watcher launches nothing else, ever).
- Your own explicit confirmations (Start Rescue click, `[y/N]` prompts,
  `--yes` flags). Nothing destructive runs without one.

What is NOT trusted:

- Any other USB stick, including its `.cure-trigger` file and any
  executables it carries. A copied trigger without this machine's pairing
  token is ignored; a drive-supplied binary is never executed.
- Data from the scanned machine: file names, registry values, task XML,
  and window titles are attacker-controlled input. They are displayed
  escaped (never executed, never passed to a shell) and scores treat them
  as evidence, not verdicts.

Cooperative vs enforced: quarantine/undo integrity (pending records,
atomic saves, hash checks) is **enforced** by the engine. Stopping malware
that is already running as your user is **not** — same-user code execution
is outside what any user-space scanner can enforce, and C.U.R.E does not
claim otherwise.

Network: none by default. Scanning, scoring, quarantine, and reporting are
fully offline (cache-only certificate revocation; the shipped webview UI
loads no remote content — CI fails on any remote URL in `gui/dist`). The
only network opt-in is CLI `--online-revocation` (live CRL/OCSP fetch).
Ransomware help resources are named as plain text, never links, so there is
nothing in the UI that can navigate anywhere.

Watcher pairing: a USB launches the GUI only if its `.cure-trigger` carries
this machine's token (`cure-watch pair E:`) AND the pinned host copy still
matches its SHA-256 pin. After `cure-watch --uninstall` (below) there is no
pairing record, so no trigger can launch anything — proven through the same
pure launch gate the watcher uses (`decide_launch` with an empty pin always
ignores).

## Watcher install + uninstall

First run of `cure-watch.exe` asks Yes/No. Yes self-installs a copy into
your Startup folder and pins `%LOCALAPPDATA%\CURE\cure-gui.exe` for later
launches; No installs nothing. The GUI does not manage the watcher — the
command below is the interface.

```bat
cure-watch --uninstall              :: confirm, then remove (idempotent)
cure-watch --uninstall --yes        :: non-interactive
cure-watch --uninstall --dry-run    :: preview only, changes nothing
```

It removes exactly what self-install created (same constants, unit-tested
against drift): the Startup copy, `%APPDATA%\cure-watch-consent.json`,
`%APPDATA%\cure-watch.log`, `%LOCALAPPDATA%\CURE\` (pinned exe + pairing
record, dir removed only if empty), and `~cure-canary-*` decoys in
Desktop/Documents/Downloads. Reparse points are never followed; anything
unexpected is left in place and reported. It ends with a verification pass
printing `clean` or `leftovers` (exit 0/1).

## Build

```bat
cargo build --release        :: cure.exe + cure-watch.exe (root workspace)

cd gui\src-tauri
cargo build --release        :: cure-gui.exe (GUI workspace)
```

Run the test suites (root workspace, then GUI workspace):

```bat
cargo test --workspace

cd gui\src-tauri
cargo test --workspace
```

Requires Windows 10/11 with WebView2 (preinstalled on modern systems).

## License

MIT — see [LICENSE](LICENSE).
