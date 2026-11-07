# C.U.R.E

> Evidence-first Windows security console. Finds the persistence and malware
> artifacts Windows Defender skips. Local only. Zero telemetry.

[![CI](https://github.com/MadB0i/C.U.R.E/actions/workflows/ci.yml/badge.svg)](https://github.com/MadB0i/C.U.R.E/actions/workflows/ci.yml)
[![Platform: Windows 10/11](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011%20x64-0078D6.svg)](https://github.com/MadB0i/C.U.R.E)
[![Offline First](https://img.shields.io/badge/Offline--First-Zero%20Telemetry-green.svg)](https://github.com/MadB0i/C.U.R.E)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

[![C.U.R.E demo](docs/media/cure-preview.gif)](docs/media/cure-demo.mp4)

**[▶ Watch the 43-second demo](docs/media/cure-demo.mp4)** — live ten-layer
scan, Investigation Console, and the Move Bot relocating folders from C: to
D: in one click. Recorded on the labelled demo build: every path on screen is
sample data.

---

## What it does

- **Ten-surface persistence scan** — Registry Run, Startup folders, Scheduled
  Tasks (**arguments scored**), Services, WMI subscriptions, IFEO/AppInit,
  COM hijacks, **Winlogon & Active Setup**, running processes, ransom
  indicators. ATT&CK-mapped, evidence attached.
- **Script-cradle & LOLBIN detection** — signed binaries invoking
  `certutil`/`bitsadmin`/`msiexec` with URLs, `IEX`/`DownloadString`
  payloads, hidden/encoded PowerShell, network-share persistence.
- **Move Bot** — one click moves space-hungry folders from C: to D:; copied,
  verified bit-for-bit, Windows switched over, undo-able.
- **Quarantine** — move-never-delete, SHA-256 integrity seals, scoped undo.
- **Canary tripwire & login observation** — decoys watched for tampering;
  post-login correlation (DIRECT/STRONG/PARTIAL/WEAK).
- **Storage forensics** — Detective Mode and the Storage Workstation.
- **Share result card** — aggregate counts only, preview before save.

---

## Virus detection — proven in a sealed sandbox

`testing/native/sandbox-malware-test.py` builds a sealed sandbox, plants real
industry-standard malware test material, runs the **real engine** against it,
asserts the detections, then deletes every trace (12/12 checks):

| Planted threat | Engine result |
|---|---|
| Known-bad hash sample (live SHA-256 against the compiled IOC list) | **HIGH-RISK — "Known Malware Hash"** |
| `certutil -urlcache http://…` downloader (registry Run + task) | **HIGH-RISK (55)** — LOLBIN remote argument |
| Scheduled task: `powershell -w hidden -enc <payload>` | **Detected** — argument scoring (v1 scored this 0) |
| Persistence on a UNC network share | **SUSPICIOUS +15** — "Defender skips these" |
| EICAR test file | intercepted by the host AV before the scan — independent proof the test material is genuine |

The same run shows **3 high-risk / 3 suspicious — exactly the planted
threats, zero false findings** — and the sandbox is provably deleted
afterwards.

---

## Quick Start

```powershell
git clone https://github.com/MadB0i/C.U.R.E.git
cd C.U.R.E\gui\src-tauri
cargo run --release
```

CLI:

```powershell
cargo run --release -p cure_cli -- scan
cargo run --release -p cure_cli -- report --format json
cargo run --release -p cure_cli -- agents
```

Requires Windows 10/11 x64 and WebView2 (preinstalled on modern Windows).

---

## Principles

Local only · zero telemetry · no auto-scan · move-never-delete ·
incomplete coverage is reported as **INCOMPLETE**, never as "clean" ·
secrets are masked, never stored.

---

## Verification

```powershell
cargo test --workspace          # 605 tests
cargo clippy --workspace --all-targets -- -D warnings
python testing/native/sandbox-malware-test.py   # sealed-sandbox malware proof
python gui/devtools/ui10-native-verify.py       # scanner acceptance vs the real exe
```

## License

MIT — see [LICENSE](LICENSE).
