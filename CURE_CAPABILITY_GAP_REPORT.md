# C.U.R.E — Product Capability Gap Report
**Document**: `CURE_CAPABILITY_GAP_REPORT.md`  
**Date**: October 2026  
**Context**: Evaluation for LUMA Uplink & Storage Rescue Capability Upgrade  
**Repository**: `D:\Projects\CURE`  

---

## Executive Summary

C.U.R.E has established strong engineering rigor around local-first forensic scanning, digital signature verification, explicit evidence levels (`Observed`, `Correlated`, `Unknown`), reversible quarantine, and transparent audit logging. Visually, the tactical dark console and holographic HUD have matured substantially.

However, from a **product value and user utility standpoint**, C.U.R.E still feels like a traditional Windows maintenance tool dressed in a futuristic shell. The core issue is that while the security and forensic features are disciplined, the daily workstation utilities (specifically Storage) mimic ordinary "Disk Cleanup" paradigms: scanning for temporary cache files and proposing deletions. Deletion is high-anxiety, low-yield, and already handled by Windows built-in tools. 

To become an indispensable system operator console, C.U.R.E must evolve from **"another disk cleaner and spectator mascot"** to an **"intelligent, on-device operator companion that safely guides major cross-volume resource relocations"**.

---

## 1. What C.U.R.E Can Do Today That Is Genuinely Useful

1. **Evidence-First Persistence Audit & WinTrust Verification**
   - Direct inspection of registry Run keys, Startup folders, Scheduled Tasks, Win32 Services, WMI event subscriptions, IFEO debuggers, and COM hijack locations.
   - Non-negotiable evidence tags: explicit separation between directly observed facts and correlated hypotheses.
   - Offline cryptographic signature verification against the Windows catalog database and embedded Authenticode certificates without network round-trips.

2. **AI Agent Radar & MCP Configuration Discovery**
   - Inventory of local AI tool environments (Claude Desktop, Cursor, Codex, OpenCode, Cline, Aider) and active MCP servers.
   - Detection of configuration sprawl and secret token exposure with mandatory masking (`masked`, zero credentials displayed or written to disk).
   - Read-only flight recorder tracking workspace modifications during active agent sessions without false causality claims.

3. **Safe, Reversible Quarantine Architecture**
   - Captures original discretionary and system ACLs (SDDL), timestamps, and alternate data streams into structured rollback receipts.
   - Never deletes in-place; moves to controlled quarantine storage with explicit user authorization.

4. **Honest Measurement & Incomplete State Reporting**
   - Degrades transparently when running non-elevated: areas blocked by permissions are explicitly designated `INCOMPLETE` rather than masquerading as "Clean".
   - Measures real allocated bytes on disk via `GetCompressedFileSizeW` instead of naive file size sums.

---

## 2. What Feels Generic or Weak Compared with Built-in Windows Tools

1. **Storage Workstation Mirrors the "WinDirStat / Disk Cleanup" Paradigm**
   - The current storage tool focuses heavily on finding temp caches, crash dumps, and hash-verified duplicates for deletion.
   - Windows already includes *Storage Sense* and *Disk Cleanup* (`cleanmgr.exe`), which automatically purge temporary internet files and Windows Update leftovers.
   - Purging 1.2 GB of temp files provides temporary relief for 48 hours before caches rebuild, failing to solve systemic disk exhaustion.

2. **Missing Cross-Drive Intelligence (The "Full C: / Empty D:" Dilemma)**
   - The primary storage crisis for Windows power users, developers, and gamers is: **C: (SSD system partition) is critically full (red warning bar in Explorer), while secondary drives (D:, E:, or secondary NVMe/SATA SSDs) have 500 GB to 1 TB of idle capacity**.
   - Built-in Windows folder redirection (Folder Properties → Location → Move) is notoriously brittle:
     - Fails mid-transfer with opaque `0x80070020` sharing violations if any process has an open handle.
     - Offers no pre-flight verification or checksum confirmation.
     - Can permanently merge folders or mangle system paths if the destination path is mistyped.
     - Does not support developer workspaces, package caches, or large application directories safely.
   - C.U.R.E currently measures disk space but does not guide the operator through resolving cross-volume capacity bottlenecks.

3. **Passive Mascots vs. Functional Local Operators**
   - LUMA currently functions primarily as an expressive decorative avatar responding to scan events (waving, purring, sleeping).
   - A true operator console requires an on-device copilot that evaluates system state, highlights risks in human terms, structures actionable multi-step operations, and assists with safe verification.

---

## 3. What Features Feel Visually Impressive but Low-Value

1. **Canvas Treemap Navigation**
   - Treemaps look technologically complex, but wading through hundreds of colored rectangles forces the user to manually hunt for space.
   - Operators do not want to hunt; they want clear, curated recommendations: *"Your Downloads folder is 48 GB and has not been cleaned in 6 months; your D: drive has 420 GB free. Here is a verified 2-minute migration plan."*

2. **Generic Duplicate File Queuing**
   - Finding duplicates by full hash is cryptographically sound, but asking users to manually select which copy of 300 duplicated assets to delete creates high cognitive friction and fear of accidental data loss.

3. **Status Pill and Holographic HUD Without Guided Action**
   - Holographic targeting rings and sweep beams look slick, but if the end result is simply a static list of files with a "Delete" button, the experience falls flat.

---

## 4. The High-Value Capability Upgrade: "LUMA Storage Rescue"

To create immediate, tangible differentiation from generic utilities, C.U.R.E must introduce **LUMA Storage Rescue**:

### Core Differentiation: Migration & Relocation Over Deletion
- **Problem Solved**: Safely relieve severe C: drive exhaustion by identifying, planning, and executing verified cross-drive migrations (e.g. to D:) without breaking applications or risking data loss.
- **Strict Safety Tiers**:
  - `SAFE`: Personal user content libraries (`Downloads`, `Videos`, `Pictures`, `Documents`, `Desktop`, user media, manual archives).
  - `CAUTION`: Developer workspaces, package caches (`.cargo`, `npm-cache`, pip), game libraries, tools requiring post-move directory reconfiguration.
  - `DO NOT MOVE (Blocked)`: Windows system trees (`C:\Windows`), `Program Files`, `ProgramData`, critical AppData system stores.
- **Guided Operator Protocol**:
  1. *Drive Pressure Scan*: Automated ratio check comparing source volume pressure (C:) against target availability (D:).
  2. *Candidate Discovery*: Identifies high-yield relocation targets with real file counts and allocated sizes.
  3. *LUMA Safety Assessment*: Explains safety rationale and path-adjustment prerequisites.
  4. *Plan Construction*: Validates destination capacity with a 10% safety buffer.
  5. *Dual-Integrity Copy & Verify*: Copies data non-destructively, verifies exact byte match, file count parity, and critical sample hashes.
  6. *Reversible Receipt*: Generates an explicit migration manifest documenting source, destination, hash checks, and rollback/cleanup guidance.
  7. *Zero Silent Deletions*: Source data is **never silently erased**. Users choose between retaining dual copies or confirming manual source retirement once verification is inspected.

---

## Conclusion & Strategic Roadmap

By pairing **LUMA Storage Rescue** with the **LUMA Uplink** visual and behavioral redesign, C.U.R.E transforms into a proactive, trusted forensic workstation. The following phases implement this capability end-to-end across core Rust modules, Tauri IPC channels, mock testing fixtures, and the desktop frontend console.
