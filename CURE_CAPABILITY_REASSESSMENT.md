# C.U.R.E Capability Reassessment: LUMA Uplink & Storage Rescue

**Date:** 2026-10-30  
**Repository:** `D:\Projects\CURE`  
**Branch:** `feat/agent-intelligence-release`  
**Harness / Verification:** Native Tauri desktop binary (`cure-gui.exe`), Playwright CDP test harness, full Rust workspace test suite (465 core tests, 10 GUI tests, 0 clippy warnings).

---

## Executive Summary

Prior to this upgrade, C.U.R.E risked plateauing as a reactive evidence viewer—visually distinctive, but functionally resembling a specialized combination of Windows `cleanmgr.exe`, Autoruns, and an event viewer. While its telemetry was honest and offline-first, its value proposition was largely informational. Users were told where files were, but still had to manually solve their primary storage crisis: **critical congestion on the primary drive (C:) with nowhere safe to put files without risking broken applications or unrecoverable loss.**

With the delivery of **LUMA Uplink** and **LUMA Storage Rescue**, C.U.R.E transitions into an **active on-device operator workstation**. LUMA evolves from an idle decorative companion into a functional, telemetry-coupled cyber-forensic assistant that guides cross-volume relief, preflights headroom, enforces non-destructive transfer, executes dual-integrity verification (file count, total bytes, sampled SHA-256 hashes), and issues an auditable, reversible recovery receipt.

---

## 1. What Is Now Genuinely Stronger?

1. **Active Guided Remediation Without Destruction:**
   - Standard disk cleanup tools delete files in place. If an app or user needed them, they are gone.
   - LUMA Storage Rescue provides cross-volume migration from congested drives (C:) to secondary storage (D:), keeping all original source data completely intact while verifying the copy bit-for-bit before suggesting retirement.
2. **Strict Cryptographic & Structural Verification (`CandidateVerification`):**
   - The engine does not rely on Windows Explorer's unreliable copy dialog. It verifies source file counts, destination file counts, byte-level sizes, and computes SHA-256 hashes over files to confirm zero silent corruption.
3. **Deterministic Safety Classification:**
   - Folders are segmented strictly into `Safe` (Downloads, Videos, Pictures, Desktop), `Caution` (Cargo cache, Pip cache, npm cache, local git workspaces with explicit reconfiguration instructions), and `DoNotMove` (Windows, Program Files, critical AppData). The system explicitly refuses unsafe migrations.
4. **Cohesive Operator Persona (LUMA Uplink):**
   - LUMA is no longer a detached static cat asset. Through the **LUMA Uplink** rig (tactical collar antenna, brow optic reticle, orbiting scout drone node, and evidence-responsive aura), LUMA reflects the real operational state of the workstation across Rescue, Agent Radar, and Storage Workstation.
   - When congestion is detected on C:, LUMA highlights relief potential and guides preflight. When scanning persistence vectors or agent processes, LUMA locks probes to real telemetry targets.

---

## 2. Does C.U.R.E Now Do Something Windows Default Tools Do Not Do Well?

**Yes. Decisively.**

| Feature / Capability | Windows Default Tools (`cleanmgr`, Storage Sense) | C.U.R.E LUMA Storage Rescue |
| :--- | :--- | :--- |
| **Space Recovery Strategy** | Deletes temp files and empties Recycle Bin (often recovers only 200MB–2GB; files regenerate within 48h). | Relocates multi-gigabyte personal libraries and bulky build caches across physical volumes. |
| **Safety & Reversibility** | In-place permanent deletion. No rollback. | Non-destructive dual-residency. Original source data is preserved on C:. Complete revert instructions provided. |
| **Integrity Assurance** | None (blind delete). | Dual-integrity verification: File count parity, byte parity, sampled SHA-256 hash validation. |
| **Developer Cache Awareness** | Clueless. Ignores `.cargo/registry`, `pip/cache`, `npm-cache`, or treats them as untouchable system files. | Recognizes developer toolchains as `Caution`, details exact environment variable updates (`CARGO_HOME`, `PIP_CACHE_DIR`). |
| **System Folder Protection** | Vague dialogs ("Some files cannot be deleted"). | Strict architectural hard-block with clear forensic reasons why Windows/AppData must remain local. |

---

## 3. Is LUMA Now a Real Product Feature Rather Than Just a Mascot?

**Yes, though she remains an assistant rather than an autonomous actor.**

- **Telemetry Coupling:** LUMA's visual state is driven directly by backend state transitions. In Storage Rescue, LUMA calculates staged relocation payloads and explains the preflight checklist. In Agent Radar, LUMA enters scout lock mode across supported AI agent processes.
- **Operator-First Communication:** LUMA never spams conversational filler or pretends to possess artificial consciousness. Her dialogue is concise, evidence-first, and operational:
  > *"Operator, 2 folders (3.9 MB) are staged for relocation to D:. C.U.R.E will copy and bit-verify the transfer while keeping your source files intact."*
- **Visual Integration:** The scout probe drone node dynamically orbits beside LUMA and emits holographic signal threads that link to active telemetry cards, bridging character animation with forensic utility.

---

## 4. What Still Feels Weak? (Blunt Assessment)

1. **NTFS Junctions & Shell Redirection Are Still Manual:**
   - In this release, Storage Rescue copies and verifies data non-destructively, establishes the target structure on D:, and gives the operator copy-paste PowerShell commands or Explorer shortcuts. It does *not* automatically modify registry Known Folder GUIDs (`SHSetKnownFolderPath`) or inject directory junctions (`mklink /J`). While this satisfies the "move, never delete" and "no silent changes" principles, less technical users might still want a one-click "Switch Known Folder" confirmation.
2. **Candidate Discovery Breadth:**
   - Candidates are currently identified from known user directories, standard developer caches (`cargo`, `pip`, `npm`), and top-level root project workspaces. Large custom installations (e.g. Steam libraries, Docker desktop disk images, HuggingFace model caches) are not yet automatically scanned unless they reside in standard AppData or known paths.
3. **Real-time Transfer Progress Granularity on Gigabyte Payloads:**
   - While the Tauri event system streams file-level progress, Windows I/O buffering on massive multi-gigabyte archives can make transfer progress feel bursty rather than smooth.

---

## 5. Recommended Next Capability

**Capability: LUMA Workstation Known-Folder Relocation & Shadow Junction Manager**

- **Why:** Now that cross-volume verified copy is implemented and battle-tested, the natural follow-up is an **Operator-Approved Redirection Hook**.
- **What it would do:**
  1. For `Safe` user folders (Downloads, Videos, Documents), offer an explicit, confirmation-gated Windows API call (`SHSetKnownFolderPath`) to cleanly re-point Windows Shell to D: without broken registry keys.
  2. For `Caution` developer directories, offer an optional, verified NTFS junction (`CreateSymbolicLinkW` / `FSCTL_SET_REPARSE_POINT`) so existing scripts and compilers continue resolving paths seamlessly while physical blocks reside on D:.
  3. Include a one-click "Undo Redirection" feature that points Known Folders back to C: at any time.

---

## 6. Captured Native Screenshots

All screenshots were captured from the running native Windows binary (`cure-gui.exe`) via Chrome DevTools Protocol at 1440x900 resolution:

1. `testing/native/screenshots-luma-uplink/01-rescue-luma-uplink.png`  
   *Rescue console with LUMA Uplink rig active (collar transceiver antenna, brow visor optic, orbiting scout probe drone, telemetry connection).*
2. `testing/native/screenshots-luma-uplink/02-agent-radar-scout.png`  
   *AI Agent Radar with scout probe lock and real-time process detection sweeps.*
3. `testing/native/screenshots-luma-uplink/03-storage-rescue-overview.png`  
   *Storage Workstation with LUMA Storage Rescue tab, critical C: drive congestion warning, D: drive headroom analysis, and operator guidance.*
4. `testing/native/screenshots-luma-uplink/04-storage-rescue-candidates.png`  
   *Candidate segmentation table showcasing Safe to Relocate (Downloads, Videos, Documents, Desktop), Caution (Toolchains), and Blocked system paths.*
5. `testing/native/screenshots-luma-uplink/05-storage-rescue-plan-modal.png`  
   *Preflight migration plan modal displaying verified source accessibility, destination writeability, headroom validation, and non-destructive policy notice.*
6. `testing/native/screenshots-luma-uplink/06-storage-rescue-receipt.png`  
   *Cryptographic rollback & verification receipt detailing bit-exact file count, total bytes, SHA-256 sample verification, and source preservation.*

---

## 7. Definition of Done Checklist

- [x] `cargo fmt --check` passes with zero formatting issues.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.
- [x] `cargo clippy --manifest-path gui/src-tauri/Cargo.toml --all-targets -- -D warnings` passes with zero warnings.
- [x] `cargo test --workspace` passes (465 core tests, 5 cli tests, 10 winwatch tests, 58 watch tests).
- [x] `cargo test --manifest-path gui/src-tauri/Cargo.toml` passes (10 GUI unit tests).
- [x] `gui/dist` and `gui/dist-demo` shared JS/CSS assets verified byte-identical.
- [x] Native screenshots captured, reviewed, and verified free of visual defects.
- [x] Local-only, offline-first, evidence-first, non-destructive principles preserved.
