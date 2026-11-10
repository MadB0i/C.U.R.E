# C.U.R.E — Portable Windows Security & Storage Rescue

> **Evidence-first Windows security console and storage rescue toolkit.**  
> Catches the stealth malware, script droppers, and LOLBins that slip past standard antivirus.  
> **100% Offline · Zero Telemetry · Local Heuristic Engine · Portable EXE.**

<p align="center">
  <a href="https://github.com/MadB0i/C.U.R.E"><img src="https://img.shields.io/badge/Platform-Windows_10_%7C_11-0078D6?style=for-the-badge&logo=windows&logoColor=white" alt="Platform: Windows 10/11"></a>
  <a href="https://github.com/MadB0i/C.U.R.E"><img src="https://img.shields.io/badge/Rust-2021_Edition-DEA584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 2021"></a>
  <a href="https://github.com/MadB0i/C.U.R.E"><img src="https://img.shields.io/badge/Tauri-v2_Desktop-24C8D8?style=for-the-badge&logo=tauri&logoColor=white" alt="Tauri v2"></a>
  <a href="https://github.com/MadB0i/C.U.R.E"><img src="https://img.shields.io/badge/Privacy-Zero_Telemetry-10B981?style=for-the-badge" alt="Zero Telemetry"></a>
  <a href="https://github.com/MadB0i/C.U.R.E"><img src="https://img.shields.io/badge/Tests-620+_Passing-success?style=for-the-badge" alt="620+ Tests Passing"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue?style=for-the-badge" alt="MIT License"></a>
</p>

---

### 🎥 Watch C.U.R.E in Action

<p align="center">
  <img src="docs/media/cure-showcase.gif" alt="C.U.R.E Showcase Demo" width="960" style="border-radius: 12px; box-shadow: 0 8px 32px rgba(0,0,0,0.5);">
</p>

> **All-In-One Rescue Console:** Watch real-time heuristic scanning uncover hidden script droppers, live AI agent radar monitoring, and the Sentinel Oculus moving 23.5 GB of dev cache between drives with verified zero data loss.

---

## 🛡️ The Real Threat: Why Windows Defender Misses Modern Malware

Most modern viruses and trojans don't look like `.exe` files anymore. Modern attackers exploit legitimate Windows tools and obscure startup locations:

1. **Obfuscated Script Droppers:** Attackers hide payloads inside uncompiled `.bat`, `.vbs`, or `.ps1` scripts using Base64 encoding (`-EncodedCommand`) or download cradles (`IEX`, `Net.WebClient`). Standard antivirus often skips them until cloud telemetry runs.
2. **Living-off-the-Land Binaries (LOLBins):** Attackers abuse trusted, Microsoft-signed Windows executables (`certutil.exe`, `bitsadmin.exe`, `msiexec.exe`) to silently download and execute malware from remote servers.
3. **Deep Persistence Hooks:** Malware anchors itself into obscure Windows corners—IFEO debuggers, AppInit DLLs, COM hijacking, WMI event consumers, and hidden Scheduled Tasks.
4. **Destructive Antivirus Cleanup:** Many antivirus tools blindly delete files, corrupting system references or breaking legitimate tools.

### How C.U.R.E Protects You:

- **Instant Local Heuristics:** Evaluates command line flags, entropy, and drop zones in real-time. No cloud required.
- **10-Surface Deep Inspection:** Scans Registry Run/RunOnce, IFEO, AppInit DLLs, COM Hijacks, Active Setup, WMI, Services, Tasks, and Startup Folders.
- **Move, Never Delete:** Every flagged threat is safely isolated into an encrypted, SHA-256 sealed quarantine vault. If needed, you can restore any file with **1-click undo**.
- **100% Offline & Zero Telemetry:** No data ever leaves your computer. No accounts, no background tracking, no internet connection required.

---

## ⚡ The 3 Core Rescue Workstations

### 1. 🛡️ Virus Rescue (`#scan-view`)
- **Cyber-Cat Heuristic Scanner:** Deeply inspects your system for hidden droppers, suspicious arguments, and persistent malware.
- **Safe Test Threat Button:** Test detection capabilities anytime with a single click—spawns a harmless test dropper in `%TEMP%` to verify the scanner.
- **Custom Rules Engine:** Easily add custom detection rules (`%APPDATA%\cure\rules\*.rule`) to catch specific suspicious patterns or red-team payloads.
- **Industry Standard Export:** Export complete forensic dossiers directly to **SARIF v2.1.0** and JSON for audits and GitHub Code Scanning.

### 2. 🤖 AI Agents Radar (`#view-agents`)
- **Coding Bot Discovery:** Automatically identifies running AI coding agents and local LLM engines (Antigravity, Claude Code, Cursor, OpenCode, Codex, Ollama, LM Studio).
- **Process & Memory Telemetry:** Groups multi-process instances (e.g., 7 child PIDs) into a unified telemetry card with real-time RAM and port tracking.
- **Credential Protection:** Automatically masks sensitive API keys, session tokens, and secrets from reports and displays.

### 3. 💾 Storage & Move Rescue (`#cleanup-view`)
- **Instant Space Discovery:** Finds the true largest space consumers (`.codex`, `.android`, `.gradle`, `.cargo`, caches) in **2–4 seconds** without freezing your PC.
- **Uninstalled App Leftovers:** Detects and flags orphaned folders left behind by uninstalled applications.
- **Sentinel Oculus Bit-for-Bit Relocation:** Safely moves heavy directories from a crowded drive (e.g., C: to D:) with bit-for-bit SHA-256 verification and automatic NTFS redirection. **Zero data loss guaranteed.**

---

## 💻 CLI Companion (`cure.exe`)

For terminal lovers, DevOps engineers, and CI/CD automation:

```powershell
# Interactive system scan
cure scan

# Export results in SARIF v2.1.0 for GitHub Actions / CI/CD
cure scan --format sarif --output results.sarif

# Discover running AI agents and LLM processes
cure agents

# Inspect storage health and largest consumers
cure storage scan

# Quarantine a detected threat (safely reversible)
cure quarantine <id>
cure undo <id>
```

---

## 🚀 Quick Start

### Option 1: Run the Standalone Binary (Fastest)
Download the latest portable release **`CURE.exe`** and launch it directly. No installation required.

### Option 2: Build From Source

**Prerequisites:**
- Windows 10 or 11 (64-bit)
- [Rust toolchain](https://rustup.rs/) (edition 2021)
- WebView2 Runtime (pre-installed on Windows 10/11)

```powershell
# 1. Clone the repository
git clone https://github.com/MadB0i/C.U.R.E.git
cd CURE

# 2. Run Desktop GUI (Tauri v2)
cargo run --release --manifest-path gui/src-tauri/Cargo.toml

# 3. Or run the CLI Companion
cargo run --release -p cure_cli -- scan
```

---

## 🧪 Comprehensive Verification Suite

C.U.R.E is backed by **620+ automated unit, integration, and sealed-sandbox tests**:

```powershell
# Run all workspace tests (620+ passing)
cargo test --workspace

# Zero-warning clippy linter check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --manifest-path gui/src-tauri/Cargo.toml --all-targets -- -D warnings

# Sealed-sandbox malware detection validation
python testing/native/sandbox-malware-test.py
```

---

## 📜 Core Guarantees

1. **100% Offline:** Never makes outbound network calls.
2. **Explicit Consent:** Never starts a scan or alters files without an explicit user click.
3. **Move, Never Delete:** Threats go to quarantine with full 1-click restore.
4. **Honesty Over Comfort:** Inaccessible files show as `INCOMPLETE`, never fake "Clean".
5. **Mask Secrets:** API keys and credentials are never stored, logged, or displayed unmasked.

---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.
