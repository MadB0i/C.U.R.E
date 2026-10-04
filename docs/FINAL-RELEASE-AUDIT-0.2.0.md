# Final release audit — C.U.R.E 0.2.0

2026-10-04. Scope: final review and concrete fixes only; no new product features.

## A. Release verdict

**READY TO PUSH** `codex/evidence-console` and open a PR for remote CI.
**Not authorized for publication.** Human NVDA and PR-head CI remain merge /
release gates. No push, merge, tag or publication occurred in this task.

## B. Exact candidate commit

`ee44f787749175fb50fd6069b1669314a08f309b` — source, media and package.
The readiness/final audit follow-up is documentation-only. `BUILD-INFO.txt`
records the candidate SHA above; the final chat records the current HEAD.

## C. Final test matrix

| Gate | Result |
| --- | --- |
| `cargo test --workspace` | PASS: 364 (CLI 5, core 294, dirwatch 7, watch 58), 0 failures |
| GUI `cargo test` | PASS: 4, 0 failures; interactive overlay fixture ignored (1) |
| Root and GUI `cargo fmt --check` | PASS |
| Root Clippy workspace/all targets, `-D warnings` | PASS |
| GUI Clippy all targets/all features, `-D warnings` | PASS |
| Sanitized production release builds | PASS: workspace and GUI v0.2.0 |
| JS syntax / production offline source guard | PASS |
| Python Playwright `all` | PASS: 268 checks, including reduced motion and axe |
| Existing layout / pixel wrappers | PASS: 80 / 68 checks |
| Existing docs capture / demo | PASS: 108 checks / 1 runtime check |
| Real CLI baseline guard | PASS: all assertions, isolated output |
| Real CLI quarantine regression | PASS: refusal, confirmed move, integrity, idempotence, undo |
| Actual production WebView2 | PASS: 45 checks, zero console/runtime errors, zero app remote requests |
| Native axe WCAG 2/2.1/2.2 A/AA | PASS: 6 view scans, zero violations |
| Portable package / ZIP boundary / SHA-256 | PASS: 17 files, 7 manifest hashes, full staging/ZIP byte identity |
| Git diff whitespace / builder-path / common credential-marker scan | PASS |
| Human NVDA | NOT VALIDATED |
| Interactive real overlay close/force fixture | NOT VALIDATED (ignored intentionally) |
| Elevated vs standard-user comparison | NOT VALIDATED |
| OS-wide reduced-motion toggle / other physical DPI monitors | NOT VALIDATED (browser/native emulation passed) |
| Candidate remote GitHub CI / publishing job | NOT RUN; requires push + PR / explicit release authorization |

Counts overlap between browser modes; do not add them as independent cases.
No currently available local automated gate remains failing. The earlier
partial-coverage and native CSP failures were fixed and their flows rerun.

## D. Visual and copy review

Every final PNG and representative frames across the entire demo sequence
were inspected, including first/final frames. Desktop 1440px, 390px fallback,
900×600 and actual native window were reviewed. No remaining mascot, glow,
decorative gradients, clipped technical fields or unfinished empty states were
found. Values remain selectable and wrap; accent is restrained and status has
text. Computed native styles show one 6px radius, Segoe UI/Cascadia stacks and
one inset navigation accent; no decorative surface-shadow system.

Concrete fixes:

1. Partial coverage with findings previously said “Scan complete”. It now says
   “Review required · Coverage incomplete”; completed sweep says “Collection
   ended”. Added a deterministic partial-findings regression.
2. GIF/MP4 blank initialization lead-in removed; loop starts on loaded Overview.
3. Production binary embedded dev HTML/mock JS. Replaced directory embedding
   with an explicit production asset allowlist; native route probes confirm
   developer content is absent.
4. Direct Cargo builds lacked Tauri custom-protocol production context; made
   production context the default and gated env-triggered E2E drivers behind
   explicit `desktop-e2e`. Production contains no test-driver env markers.
5. Panic/source strings disclosed local builder paths. Shared build script
   remaps workspace/profile paths; all three final EXEs pass the hygiene gate.
6. Native CSP rejected IPC fetch and logged fallback errors. Allowed only
   Tauri's internal IPC schemes/host; final native flow has zero errors.

Trust-wording pass covered GUI strings, README, SECURITY and release notes.
No automatic quarantine or guaranteed protection claim remains. “Safe score”
is a heuristic classification, not a safety guarantee. Quarantine says move;
separate disk cleanup truthfully says permanent deletion. No destructive
engine behavior, scoring rule or watcher consent rule changed in this audit.

## E. Actual Windows validation

Launched a byte-identical copy of the built **production EXE**, using actual
WebView2 (Edge 154), not mock HTML or a feature-enabled test-driver build.
First-launch external networking was blocked for that child WebView via a
loopback-only debugging endpoint/unavailable proxy; offline reload passed.
No global Windows networking/security setting was changed.

The real scan read native collectors, displayed advancing source state (30
samples, 22 distinct states), then showed real results/evidence. An inert
Startup fixture and Task/data roots were isolated under `%TEMP%`; the fixture
was never executed. Registry/Services/WMI/process inspection remained
read-only. Nothing was moved before explicit confirmation. Cancel/Escape kept
the file unchanged, confirmation moved only that fixture, archived bytes/hash
matched, and Undo restored byte-identical content and an empty archive receipt.
No host process was terminated or host persistence remediated.

Overview, Rescue, results, inspector, confirmation and quarantine passed
layout/axe. Keyboard trapping, focus return and visible focus were checked.
Actual Windows resize reached 900×600 at host **125% DPI**, without document
overflow or footer clipping; OS Tab input retained visible focus. Native CDP
emulation checked 200% scaling and reduced motion (no running animations).
Mock globals/dev assets were absent. No app remote requests or runtime errors
were recorded. Temporary native captures/logs remain outside the repository;
the fixture instance was closed after validation. The packaged GUI hash equals
the executable used for these checks.

## F. Accessibility

Browser states pass automated axe and keyboard gates. Six native axe scans
also have zero violations. **A human screen-reader result is not claimed.**
Remaining short NVDA checklist (use only an inert fixture):

- On launch, read Overview posture/coverage and navigate the five named
  destinations; verify selected state and focus are announced sensibly.
- Start Rescue; verify scan source/progress and the final coverage limitation
  are announced without overwhelming repeated speech.
- Select a finding; verify inspector name, heading order, reasons, full path,
  hash and unavailable metadata can be read/copied. Tab remains in inspector;
  Escape returns to the selected finding.
- Open quarantine confirmation; hear its title and the three consequence
  sections. Cancel is initial focus; Escape/Cancel returns to its trigger.
  Approve only the fixture and verify the action result is announced.
- Read archive locations/hash/fidelity/Undo, restore the fixture, and hear the
  Restored receipt and empty state. Verify no stale dialog/background focus.

## G. Release artifacts

Use **`release-candidate/v0.2.0-final/`**. Original `v0.2.0/` is rejected and
preserved; its old hashes are not release-ready. Exact files and full hashes:

| Asset | SHA-256 |
| --- | --- |
| `cure-gui.exe` | `9b5a868617b1cc82b3096b65d6e8d19add055f161cbe8b58394022941fff0bc6` |
| `cure-watch.exe` | `7fe70d4d3c7b252a1f4c2f348f5ef98bf9866facbe594bc4a8eca03143912589` |
| `cure.exe` | `9338c30e85f9e4c3fc1e900ce33a9194f3dcf11bf8e461dc0b6c423e5210d28e` |
| `cure-v0.2.0.zip` | `2811ec75dc7f2e29786b08a42fc5ec8e9aad59eccd42eeebcbe73ec19d1ad3cb` |

The external `SHA256SUMS.txt` covers those four artifacts; internal manifest
covers three EXEs. ZIP has exactly the 17 files listed in
[release readiness](RELEASE-READINESS-0.2.0.md). ZIP/staging identity, no debug/
source/temp artifacts, no current builder paths/common credential markers,
production asset exclusion, source SHA and unsigned disclosure were checked.
All EXEs are NotSigned. GUI PE version and CLI version are 0.2.0; watcher
metadata is 0.2.0, with no PE version resource or `--version` handler.

Screenshots: seven 1440×900 PNGs. GIF: 1100×688, 122 frames, 15.26 seconds.
README and release notes match intended contents and threat-model limits.
Publish freshly computed checksums if CI rebuilds any bytes.

## H. Release-relevant limitations

Human NVDA, live overlay force-close, elevated comparison and other physical
DPI/OS motion settings remain unvalidated. Reported coverage exists for four
collectors; unknown coverage stays explicit. Watcher connection has no GUI
status API. Fidelity/optional evidence can be unavailable; ACL/timestamp
restore is best effort. Findings are heuristic, Canary experimental and IOC
provider a demo fixture. Quarantine does not stop running malware. Same-user
compromise is outside the enforcement model. Session timeline is not a durable
forensic log; report export rescans. Binaries are unsigned and WebView2 is a
host prerequisite. No malware was run for this audit.

## I. Next exact action

Push `codex/evidence-console` and open a PR against `main` so the PR-triggered
GitHub CI runs. Require CI success at the PR head and complete the NVDA checklist
before merging. Any code changes require relevant retests and a new candidate /
manifest. Tag/publish a new version only with explicit later authorization;
never overwrite v0.1.0.

Pre-push review inspected every commit and the complete diff against local
`main`, including the additive read-only evidence metadata in core. Deliberate
media replaces old screenshots; local captures, builds and candidate ZIPs are
ignored. The branch contains no accidental generated output or detected common
credential/current builder-path markers. The final follow-up commits are audit
and readiness documents only. Existing v0.1.0 tag remains
`8c8a17097e0e0fe0610e79885d346b754cdc2c4a`.
