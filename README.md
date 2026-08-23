# C.U.R.E — Clean USB Rescue Engine

A portable, install-free Windows security toolkit that detects the most
common ways malware survives a reboot, risk-scores every finding, and lets
you **quarantine (never delete)** anything malicious — without touching
user data.

## What's in the box

- **`cure`** (CLI) — scan / diff / quarantine / undo from any terminal
- **`cure-watch`** — background watcher that auto-launches the GUI when a
  trusted rescue USB is plugged in (zero-click)
- **GUI** (Tauri v2) — live animated scan interface with a real-time
  network-graph visualization, auto-cleans high-risk file persistence
- **`cure_core`** — shared engine: scanners, risk scoring, baseline diffing,
  quarantine, Authenticode + hash-based detection

```
cure/
├── core/     shared engine (cure_core) — scanners, risk.rs, baseline, quarantine
├── cli/      cure.exe
├── watch/    cure-watch.exe — USB-trigger auto-launcher
└── gui/      cure-gui.exe (Tauri v2 + animated frontend)
```

## Build

```bat
cargo build --release          :: cure.exe + cure-watch.exe
cargo test  --workspace        :: 60+ tests, engine + watcher

cd gui\src-tauri
cargo build                    :: cure-gui.exe (needs WebView2, preinstalled on Win10/11)
```

## CLI usage

```bat
cure.exe scan --data-dir E:\cure-data
cure.exe diff --data-dir E:\cure-data
cure.exe quarantine <id> --data-dir E:\cure-data
cure.exe undo <id>       --data-dir E:\cure-data
```

Disk cleanup (scan-only report, then explicit confirmed deletes of
regenerable junk — temp files, browser caches, recycle bin, Windows.old;
`--include-downloads` additionally lists old installers for per-file opt-in):

```bat
cure.exe cleanup scan
cure.exe cleanup run [--include-downloads] [--dism]
```

Risk scoring: temp/downloads drop zone +30, trusted system path −20,
randomized name +25, hidden/encoded PowerShell +25, valid Authenticode
signature −40, invalid signature +40, known-malware hash match = forced
HIGH-RISK. Scores are a triage aid — read the printed reasons.

## Making a rescue USB

1. Copy `cure-gui.exe` (and `cure-watch.exe`, if setting up auto-launch for
   the first time) to the USB root.
2. `echo CURE-TRIGGER-V1> E:\.cure-trigger`

Once `cure-watch.exe` is running on a machine (self-installs to Startup on
first run, no admin needed), it polls for new drives every ~1.5s and
auto-launches the GUI when it sees a valid trigger file — full scan, live
progress, auto-quarantine of high-risk findings, all state saved to the
USB itself.

## Safety model

- Quarantine is always a **move**, never a delete — every action has a
  JSON record and an exact `undo`.
- Only reads/writes inside scanned persistence locations + its own data
  dir. Never touches Documents/Downloads/Desktop.
- Registry autoruns are read-only (detected + scored, not modified) —
  surfaced for manual review.

## Known limitations

- Trigger file isn't cryptographically signed yet (Ed25519 planned)
- Registry entries aren't auto-remediated, only flagged
- Signature check is verdict-only — no publisher/certificate extraction
- Hash-intel list is a small compiled-in demo seed, not a live threat feed
- Not yet covered: services, WMI subscriptions, IFEO, COM hijacks
- Startup `.lnk` targets aren't resolved; task XML parsing only grabs the
  first `<Command>`

## License

MIT — see [LICENSE](LICENSE).
