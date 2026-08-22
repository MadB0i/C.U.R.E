# C.U.R.E — Clean USB Rescue Engine

A portable, install-free Windows security toolkit that detects the most common
ways malware survives a reboot (persistence mechanisms), risk-scores every
finding, and lets you **quarantine (never delete)** anything malicious — without
ever touching user data.

Phase 2 turns the single-crate CLI into a Cargo workspace with four parts:

- `cure` (CLI) — scan / diff / quarantine / undo from any terminal.
- `cure-watch` — background watcher that auto-launches the GUI when a trusted
  rescue USB is inserted (zero-click).
- Tauri v2 GUI (`gui/`) — animated radar interface that auto-scans on launch and
  auto-cleans high-risk file persistence.
- `cure_core` — the shared engine: scanners, scoring, baselines, quarantine.

## Workspace layout

```
cure/
├── Cargo.toml            workspace root: members = ["core", "cli", "watch"]
│                         (gui/src-tauri is excluded — separate webview build)
├── core/                 lib crate `cure_core`
│   └── src/
│       ├── model.rs      PersistenceEntry, RiskLevel, ScoredEntry, make_id()
│       ├── risk.rs       pure scoring heuristics (unit-tested on any OS)
│       ├── baseline.rs   baseline.json save/load/diff
│       ├── quarantine.rs move-to-quarantine ledger + undo
│       └── scanners/
│           ├── mod.rs        aggregates all sources
│           ├── startup.rs    Startup folder walker
│           ├── scheduled_tasks.rs  Tasks XML walker (UTF-16 aware)
│           └── registry.rs   Run/RunOnce keys (#[cfg(windows)])
├── cli/                  bin crate `cure_cli` → binary `cure.exe`
├── watch/                bin crate `cure_watch` → binary `cure-watch.exe`
│   └── src/{main, detector, drives, trigger}.rs
└── gui/
    ├── dist/             plain HTML/CSS/JS frontend (radar animation)
    └── src-tauri/        Tauri v2 backend → binary `cure-gui.exe`
```

## Building

Requires Rust stable with the MSVC toolchain. The workspace builds everything
except the GUI:

```bat
cargo build --release          :: cure.exe + cure-watch.exe (+ cure_core)
cargo test  --workspace        :: all engine + watcher tests
```

The GUI is its own standalone workspace (it pulls in the WebView2/Tauri stack):

```bat
cd gui\src-tauri
cargo build                    :: debug build of cure-gui.exe
```

First GUI build downloads ~500 crates and takes a few minutes; afterwards it is
incremental. Running `cure-gui.exe` needs the Microsoft Edge WebView2 runtime,
which is preinstalled on Windows 10/11.

## The CLI

```bat
:: full scan + baseline save (data defaults next to cure.exe — ideal for USB)
E:\cure.exe scan --data-dir E:\cure-data

:: what appeared since the last scan?
E:\cure.exe diff --data-dir E:\cure-data

:: quarantine by id / restore
E:\cure.exe quarantine <id> --data-dir E:\cure-data
E:\cure.exe undo <id>       --data-dir E:\cure-data

:: sandboxed testing against fake directories
E:\cure.exe scan --startup-root X:\fake-startup --tasks-root X:\fake-tasks --data-dir X:\fake-data
```

Risk scoring rules (see `core/src/risk.rs`): +30 command path in a
temp/downloads/public drop zone; −20 Program Files/System32/SysWOW64;
+25 randomized-looking entry name; +25 PowerShell `-enc`/`-WindowStyle Hidden`;
+10 exe running directly from a user profile. ≥40 HIGH-RISK, ≥15 SUSPICIOUS,
else SAFE. Scores are a triage aid — read the printed reasons before acting.

## Creating a rescue USB (auto-launch flow)

One-time setup of the USB stick:

1. Copy to the drive root:
   - `target\release\cure-gui.exe` (the animated GUI)
   - optionally `target\release\cure-watch.exe` (only needed the first time)
2. Create the trigger file at the drive root named `.cure-trigger`
   containing exactly `CURE-TRIGGER-V1`:

```bat
echo CURE-TRIGGER-V1> E:\.cure-trigger
```

(The watcher tolerates trailing newlines/spaces. Keep it ASCII/UTF-8.)

End-to-end flow on a machine where the watcher is already installed:

1. `cure-watch.exe` runs at login (see self-install below).
2. It polls all drive letters every ~1.5 s using plain path-existence checks
   (`X:\` for A–Z) — no fragile Win32 device-notification APIs.
3. A newly appeared drive is checked for a valid `.cure-trigger`.
4. On a valid trigger it launches the GUI — `cure-gui.exe` from the USB root if
   present (self-contained rescue stick), otherwise the copy sitting next to
   `cure-watch.exe` — passing `--data-dir <drive-root>` so all state
   (baseline.json, quarantine/) lives on the USB.
5. The GUI auto-scans on launch with a live radar animation reacting to real
   progress events, auto-quarantines HIGH-RISK *file-backed* findings
   (Startup folder items, scheduled-task XML files), lists everything else as
   "needs review", saves a fresh baseline, and shows results.

Watcher self-install: run `cure-watch.exe` once; if it is not already in the
current user's Startup folder (`%APPDATA%\Microsoft\Windows\Start
Menu\Programs\Startup`), it copies itself there. No admin rights needed.

## Safety model

- Quarantine is always a **move** (rename when possible, verified copy+remove
  across drives) with a JSON record enabling exact `undo` restoration.
- C.U.R.E only ever reads/writes inside scanned persistence locations plus its
  own data dir. No content scanning, no bulk deletion, never touches
  Documents/Downloads/Desktop.
- The GUI's auto-clean only acts on file-backed sources; registry autoruns are
  read-only and are surfaced for manual review instead.

## What was actually verified vs written-but-unverified

Verified on Windows 11 x64 (MSVC, Rust 1.97):

- `cargo test --workspace`: 37 tests green (27 engine + 10 watcher), including
  detector/drive-diff logic and trigger-file acceptance tests that also pass on
  Linux/macOS.
- CLI end-to-end against fake roots: scan report, diff of new entries,
  quarantine move, exact undo restore.
- GUI: compiled with Tauri v2, launched for real, and observed through the full
  pipeline (webview boot → JS invoke → Rust scan → progress events →
  baseline.json written to `--data-dir`, HighRisk startup item auto-quarantined).
- GUI frontend: visually verified with a Playwright headless-browser harness
  against the mock backend (see below) — mid-scan radar, results view, and the
  quarantine-button flow were screenshotted and reviewed; both normal and
  `prefers-reduced-motion` modes are exercised automatically.

Written but not exercised automatically:

- The watcher's poll/self-install/launch loop compiles but was not left running
  or tested with a physical USB insertion (running it would install itself into
  the user's Startup folder). Its pure-logic halves are unit-tested; the OS
  glue is deliberately tiny.

## Frontend dev harness (no Tauri needed)

`gui/dist/index.dev.html` is a dev-only copy of the UI that loads
`mock-tauri.js` before `app.js`. The mock fakes `window.__TAURI__`:
progress events with realistic delays, a fake scan summary (cleaned /
needs-review / safe entries), and a stubbed `quarantine_entry` — so the whole
UI runs in any plain browser. The production entry point `index.html` never
references the mock, so it can't leak into the shipped app.

Re-run the visual check:

```bat
cd gui\devtools
npm install                        :: once — installs Playwright
npx playwright install chromium    :: once — downloads the browser
npm run verify                     :: headless run → gui\dev-screenshots\*.png
```

`verify.mjs` opens the page at 900x600 (matching the Tauri window), captures
mid-scan/results/post-quarantine screenshots in both normal and reduced-motion
modes, asserts the quarantine button confirms, checks that stagger animations
are skipped under `prefers-reduced-motion`, and fails loudly on any console or
page error.

## Current limitations (honest list)

- **The trigger file is not cryptographically signed.** Anyone who sees one
  rescue USB can clone `.cure-trigger` onto their own stick. Planned hardening:
  Ed25519 signature over a per-drive identity, verified against a pinned key.
- **Registry autoruns are never modified** — detected/scored only, manual
  removal instructions printed/shown.
- Startup `.lnk` targets are not resolved; task XML parsing is naive string
  search capturing only the first `<Command>`; COM-handler tasks are invisible.
- Heuristics are substring/ratio checks: false positives (legit tools in
  Downloads) and false negatives (lookalike names like `svchost`) exist.
  No Authenticode verification yet.
- Auto-clean quarantining a task XML does not kill an already-running instance.
- Drive detection polls every 1.5 s (a sub-second plug-in race is possible) and
  treats any mounted letter as a drive.
- Not covered yet: services, WMI subscriptions, IFEO, COM hijacks, Winlogon
  shell keys. Entry ids are FNV-1a 64-bit — stable, not collision-proof.
