# Companion pass — 2026-10-04

This focused pass follows the user's revised brief. It adds Luma, original
authored SVG artwork, to the existing Evidence Console without replacing its
navigation, Evidence Sweep or evidence-first control flow.

## Design and boundaries

Luma has a plum asymmetric bob, violet clasp/lapels, neutral jacket and
inspection tablet. The contained portrait uses the console's spacing, system
fonts and restrained accent. Overview is compact; scan places her beside the
instrument above its source list. Cleanup places the stage above real data.
No character appears inside findings, the inspector, confirmations or archive
records. Background motion pauses while a modal makes the app inert.

Five poses cover idle, scanning, cleanup, completed and review. Idle breathes
2px and blinks; scanning tracks the actual source index with a small gaze
change; cleanup moves a brush 8 degrees with small paper motion. Cleanup
motion starts only after existing explicit confirmation. Completed/review
poses are calm. No findings with incomplete coverage uses review, never a
clean claim. Pausing persists locally across screens/reloads; reduced motion
removes loops and gaze transitions while keeping the artwork visible.

The component never invokes backend commands. Authored SVG is static;
backend strings use textContent. No Rust implementation, risk scoring,
quarantine/undo behavior, dependencies, telemetry or remote assets changed.
Production's explicit asset allowlist now includes companion.js/css (six
files total); mocks and development drivers remain excluded.

## Concrete fixes

- Consolidated stale cleanup CSS selectors against actual markup, restored
  the category bars and readable two-column metrics, and retained the summary
  at the supported minimum window size. Added Included/Skipped selection text.
- Cleanup's busy stage says it is waiting for the engine result. There is no
  backend incremental cleanup progress API; no fabricated percent, files or
  freed bytes appear. Final copy uses exact engine receipt fields.
- Fixed mock cleanup counting categories as files and retaining removed files
  on rescan. Fixtures now count selected items, preserve locked failures and
  remove only successful selections. These changes affect tests/media only.

## Verification

| Gate | Result |
| --- | --- |
| Workspace Rust tests | 364 passed, 0 failed |
| Tauri tests | 4 passed; 1 interactive fixture intentionally ignored |
| Root/GUI formatting and strict Clippy | Passed; GUI all features included |
| Sanitized production build | Passed |
| JS syntax, offline source guard, whitespace/hygiene | Passed |
| Python Playwright full suite | 312 checks passed |
| Existing layout / pixel entry points | 80 / 68 checks passed |
| Documentation capture / demo | 152 / 1 checks passed |
| Real CLI baseline and fixture quarantine/undo | Passed |
| Rebuilt production WebView2 | 50 checks passed; no runtime errors/app remote requests |
| Native axe | Seven view scans, zero violations |

Browser counts overlap; they are not summed. New checks cover pause persistence,
live reduced-motion changes, decorative SVG exclusion from accessibility,
cleanup confirmation cancellation, actual busy state, exact success/failure
receipts, retained items on rescan and partial-coverage review poses. Existing
keyboard, focus, hostile-path, offline, source-state and remediation checks
remain in use. Desktop 1440×900, 900×600 and 390px fallback were reviewed;
computed native styles retain Segoe UI, a 6px radius and no decorative shadow.

Native validation used the actual production EXE in WebView2, not mock HTML
or a test-driver build. Its scan used real read-only collectors. Only an inert
temporary Startup fixture was moved after confirmation and restored with
byte identity. No host process or persistence was remediated. Offline reload,
Luma/pause, focus/trapping/Escape, 900×600/200% CDP emulation and reduced motion
passed. Actual host DPI was 125%; this pass did not repeat the earlier physical
window-resize gate. Temporary logs/captures remain outside the repository.

## Media and remaining gates

Nine 1440×900 PNGs include welcome, scan, findings, inspector, confirmation,
archive, no findings, busy cleanup and cleanup receipt. All were inspected.
The labelled sample GIF is 1100×688, 166 frames, 20.76 seconds at 8fps; MP4 and
social preview were refreshed. Representative frames including first/last
were inspected. It opens on loaded Overview, follows quarantine/undo, then
shows separately confirmed cleanup with consistent counts and remaining data.
README remains professional and evidence-first.

Human NVDA, elevated comparison, live overlay force-close and OS-wide motion /
other physical DPI settings remain unvalidated. Native permanent cleanup was
not run against host files; mock flows and existing Rust tests cover it.
Add Pause/Resume and Reduced motion label announcements to the existing
NVDA checklist in the historical final audit. Confirm the decorative portrait
does not add speech or obscure source/status announcements.

Remote PR-head CI is pending: no push, merge, tag or publication was performed.
Existing backend coverage/metadata/fidelity limitations and unsigned-binary
disclosures still apply. The changed embedded assets require regeneration;
use the new candidate identified in [release readiness](RELEASE-READINESS-0.2.0.md).
