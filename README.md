# C.U.R.E. — Clean USB Rescue Engine

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

269 Rust tests passing across the workspaces. No production or security
guarantees beyond what the test suite and reports verify.

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
