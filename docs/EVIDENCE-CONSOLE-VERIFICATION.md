# Evidence Console verification — 2026-10-04

The implementation was audited before modification. See
[the architecture and safety audit](EVIDENCE-CONSOLE-AUDIT.md). The frontend
remains framework-free, offline, and backed by the existing Tauri commands.

## Results

| Gate | Local result |
| --- | --- |
| Rust workspace tests | 364 passed: CLI 5, core 294, dirwatch 7, watch 58 |
| Tauri headless-safe tests | 4 passed; 1 interactive desktop fixture intentionally ignored |
| Workspace and GUI `cargo fmt --check` | Passed |
| Workspace and GUI Clippy, all targets, warnings denied | Passed |
| Root and GUI release builds, version 0.2.0 | Passed |
| Shipped JavaScript syntax, offline URL guard, `git diff --check` | Passed |
| Native Python Playwright full suite | 312 checks passed |
| Existing layout command | 80 checks passed |
| Existing pixel command | 68 checks passed |
| Documentation capture command | 152 checks passed; nine 1440×900 PNGs |
| Documentation demo command | Completed launch → scan → finding → inspector → confirm → quarantine → undo → separately confirmed cleanup |
| Real CLI baseline guard | Passed; isolated output, Desktop/home unchanged |
| Real CLI quarantine regression | Passed; unconfirmed call refused, explicit move, byte identity, idempotence, scoped undo |

The browser checks cover keyboard focus/trapping/return, WCAG AA axe checks,
no remote requests, 900×600 minimum and 390px fallback through 1920px,
reduced motion, source status text, partial/denied/unavailable/failed
collectors, errors/retry, empty views, long hostile values, explicit
confirmation/cancel, nested inspector confirmations, quarantine metadata,
restore receipts, cleanup, processes, Canary alerts and login observation.

The shared harness replaces obsolete random-node and Rakshak expectations;
existing MJS verification/layout/pixel/capture entry points are retained.
The CLI regression now respects the existing required `--yes` contract.

## Visual review

The nine public screenshots, 900×600 idle/scan/partial states, reduced-motion
scan and hostile long-path inspector were inspected. Window scrolling owns
overflow, technical values wrap without losing their content, and the
footer stays inside the window. Findings lead the investigation view;
confirmation explicitly separates changes, unchanged behavior and undo.

The reviewed media uses labelled deterministic sample data. The GIF is
1100×688, 166 frames, eight frames per second, 20.76 seconds; an MP4 is also saved.
This demonstrates the interface and control flow, not malware detection.

## Limits of this verification

The companion pass additionally passed 50 checks against the rebuilt production
Windows/WebView2 executable: offline launch of all six embedded assets, motion
pause/persistence, real scan, fixture move/undo, keyboard focus, emulated
900×600 and 200% scaling, reduced motion and zero runtime errors/remote
application requests. Seven native axe scans found zero violations. Host DPI
was 125%; the earlier audit's physical resize check was not repeated in this
pass. See [companion verification](COMPANION-PASS-VERIFICATION.md) for details.

Browser/native automation does not substitute for an elevated/standard-user
comparison or a human NVDA pass.
The interactive overlay fixture and native desktop E2E driver remain opt-in
and were not run during this redesign. No host process was terminated or
host persistence remediated by the GUI tests. Real CLI move/undo checks used
only inert files in temporary directories. Native disk cleanup deletion was
not exercised against host files; its state/receipt checks use mock data.

Four backend collectors report explicit coverage. Other completed collectors
remain “Coverage not reported”; Watcher/USB connectivity has no GUI status
API. ACL/timestamp fidelity is best effort. Signature/hash metadata is
collected on demand and may be unavailable. Risk scoring is heuristic;
Canary is experimental; the local IOC provider remains a demo fixture.

Remote CI for the new local candidate is pending until the branch is pushed.
No tag was created and no release was published.
