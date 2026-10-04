# Evidence Console redesign audit

> **Historical.** This audits the interface as it stood before the Rescue
> Console redesign. For current verification see
> [UI-REDESIGN-VERIFICATION.md](UI-REDESIGN-VERIFICATION.md).

Audited 2026-10-04 from 4bcdbf8. Existing framework-free Tauri v2 contracts remain authoritative.

## Product surface and transitions

Overview; Rescue landing -> explicit overlay review -> scan -> results; persistence audit; process findings; quarantine -> scoped undo; cleanup idle -> measurement -> selection -> confirmation -> result; session log; experimental Canary off/on/alert; login observation -> progress -> correlated evidence -> export. Actions require explicit operator input. No automatic quarantine.

## Contracts and constraints

Local Tauri commands: run_auto_scan, list/close/allowlist overlay candidates, entry_details, reveal_location, quarantine_entry, undo_entry, list_quarantine, report exports, incident observation/export, process termination, cleanup scan/run, Canary start/stop/status, local folder/log/exit. Events: scan-progress, incident-progress, canary-alert. Eight persistence sources plus processes and ransom indicators. Only four collectors expose explicit coverage rows; completion of other collectors cannot be represented as verified coverage. Watcher pairing/USB status has no frontend API.

Quarantine DTO omits existing size/hash/security snapshot/fidelity notes. Extend presentation metadata without changing move or restore semantics. Registry, service, WMI, IFEO, AppInit and COM are guidance-only. Fresh scan IDs and backend scope/integrity checks must remain enforced.

## Visual evidence

Inspected current desktop, narrow-window and published results screenshots. Computed DOM contains 21 font-size values and 17 border-radius variants. Grid wallpaper, violet glow, cartoon orb, random node geometry, repeated banners, decorative chips, disabled primary destinations and large empty landing dominate evidence. CSS/JS contain successive animation and mascot systems.

## Copy defects

Remove guardian/mascot language; 'quarantined automatically', 'auto-quarantined'; 'durable receipts' for session-only history; and unqualified clean status on incomplete coverage. A heuristic Safe score is not a verified signature. Experimental Canary alerts are evidence, not a ransomware diagnosis. Export performs a fresh scan.

## Target architecture

Five primary destinations: Overview / Rescue / Investigate / Quarantine / Monitor. Secondary controls preserve cleanup, persistence filters, processes, login observation and Canary. Evidence Sweep is source-indexed and advances only on scan events. Findings share a dedicated master/detail inspector. Quarantine exposes original/archive paths, integrity and fidelity metadata and scoped undo.

## Design system

Graphite evidence console; Segoe UI Variable/Segoe UI; Cascadia Code/Consolas for technical values. Neutral stepped surfaces, violet action accent, semantic green/amber/red only for states. 6px radius, 4px spacing base, 1px hairlines, 180ms transitions. No remote assets or new runtime framework.

## Verification assumptions

Existing MJS tooling uses Playwright and several obsolete mascot/random-canvas hooks. Preserve useful entry points and behavioral checks; replace visual assumptions with Python Playwright deterministic state/layout/pixel/accessibility checks. Capture 1440x900 documentation plus 900x600 supported-window and 390px fallback checks. Validate long attacker-controlled values, focus/keyboard/confirmation, reduced motion, no horizontal overflow, partial/unavailable/denied/error states. Root and separate GUI Rust tests/fmt/clippy required. Desktop overlay fixture remains explicitly opt-in.

## Release audit

GitHub latest verified v0.1.0, published 2026-08-27, with cure.exe, cure-watch.exe, cure-gui.exe and portable ZIP. Existing tag must remain untouched. Workflow currently stages only executables and emits no checksum artifact. Recommend a new pre-1.0 minor release after verification; record exact candidate commit and remote CI separately from local checks.
