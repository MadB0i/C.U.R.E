# MVP acceptance — Agent Intelligence

**Date:** 2026-10-23
**Branch:** `feat/agent-intelligence-mvp`
**Baseline:** `0fc6d77` (`main`); review HEAD `ad8294c` plus local corrective changes
**Release binary:** `gui/src-tauri/target/release/cure-gui.exe`
**Binary SHA-256:** `DD0D0B35E3B7DB4A2473FD0425CED50BDBD676AA9739DD3ABD0B20F73A840A65`

## Verdict

Local acceptance for the Agent Intelligence MVP is **CONDITIONAL GO**. Every
acceptance requirement that can be exercised locally passes. The sole blocked
gate is requirement 16: branch CI has not been run green because this review
did not push and `gh` is unauthenticated.

The baseline shared CI failure was a test race, not a product failure.
`testing/native/native-e2e.py` clicked `#sw-scan`, slept 1.2 s, then required
the transient `#sw-progress` element to be visible. On a clean CI profile the
scan completed within that sleep. The failure was reproduced locally at the
exact assertion `the storage scan shows real progress`. The corrected test
installs a MutationObserver before the click and waits for the observed
progress signal; the clean-profile native run now passes all 48 checks.

The exact CI log could not be retrieved without authenticated repository
access, so the local reproduction is strong evidence rather than
log-confirmed identification. A pushed branch run must still turn the shared
`Native end-to-end journey` check green before this becomes an unconditional
GO.

No push, merge, tag or release was performed.

## States

| State | Meaning |
|---|---|
| **PASS** | Executed and observed in this review. |
| **IMPLEMENTED** | Present in the reviewed source; not independently exercised in this run. |
| **OPEN** | Known and not addressed. |
| **BLOCKED** | Could not be completed for an external reason. |

## Acceptance matrix

| # | Requirement | State | Evidence |
|---|---|---|---|
| 1 | Detect supported OpenCode and Codex CLI executables by exact name. | **PASS** | Six names are documented and tested. Native run detected **2** agent kinds and plotted **2** radar signals. |
| 2 | Presence is never presented as activity or authorship. | **PASS** | Radar note says presence is not proof of activity. Native and browser suites assert the UI refuses attribution. No `Attributed` event is emitted. |
| 3 | Record a real workspace session and show created, modified, removed and renamed changes. | **PASS** | Native session recorded **4** changes, one of each kind, against a disposable fixture. |
| 4 | Stop-time reconciliation must not duplicate watcher observations. | **PASS** | `reconciliation_does_not_duplicate_watcher_events` pins 4 events and the correct byte totals. Native case records 4 events, not 7. |
| 5 | Coverage must be honest, including reconciliation and limits. | **PASS** | Native coverage: baseline captured, watcher started, `snapshot_reconciled: true`, complete, zero overflow/dropped events. Case label reads `Reconciliation used: yes`. A regression now marks a final snapshot with unreadable directories incomplete. |
| 6 | Refuse a drive root as a workspace. | **PASS** | Native IPC call with `C:\` returned: `Choose a project folder, not an entire drive. Monitoring a drive root is too broad.` |
| 7 | Recover an interrupted session without inventing a clean stop. | **PASS** | `interrupted_session_is_recovered_as_incomplete` verifies stopped state, last-observation end time, incomplete coverage and the reconciliation warning. |
| 8 | Agent Case File contains no paths, file names, prompts, credentials, user or machine names. | **PASS (qualified)** | Native model audit returned `[]`; engine-captured fields contain no private data. The optional operator note is free text, is screened for path/address patterns, and is not a secret scanner. The UI and this report warn the operator not to type credentials. Case PNG contains no private engine fields. |
| 9 | Preview the case before local export; export survives live refresh. | **PASS** | Native and browser runs preview and export. Browser regression keeps Export enabled after a 3.5 s refresh. |
| 10 | Windows Change Radar compares persistence without calling changes malware. | **PASS** | Native comparison completed; browser shows two review leads; UI states these are not malware verdicts and offers no remediation. |
| 11 | Public demo remains synthetic and cannot write a real case file. | **PASS** | Agent export in the public demo returns `preview-only and are never written`; demo case carries a SYNTHETIC DEMO band. No remote requests were observed. |
| 12 | LUMA artwork refresh keeps every authored state and reduced-motion contract. | **PASS** | Native run reached all 12 states and stopped every animation under reduced motion. Browser checks confirm the rig, palette, petting, keyboard access and pose separation. |
| 13 | Native screenshots show the real release build, not the mock. | **PASS** | Screenshots 12–13 were captured from the final release binary. Screenshot 13 is a full 1200×1500 case PNG rendered from the real `caseDataUrl`. |
| 14 | A 25–30 second agent walkthrough exists. | **PASS** | `docs/media/agent-demo.mp4`, 25.44 s, synthetic public demo. |
| 15 | Formatting, linting and tests pass. | **PASS** | See the gate table below. |
| 16 | Branch CI is green. | **BLOCKED** | `gh` is unauthenticated and no push was made. The baseline shared CI failure was locally reproduced and fixed in the native test; a pushed CI run has not confirmed it. |

## Native evidence

Final clean-profile native run: **48 checks passed**. The run used isolated
`TEMP`, `TMP` and `LOCALAPPDATA` directories so the scan was fast enough to
exercise the progress race that broke CI.

- **Supported agents:** OpenCode and Codex CLI (`2` kinds, `8` processes in
  the captured sample).
- **Flight session:** 4 events — 1 created, 1 modified, 1 removed, 1 renamed.
- **Coverage:** complete within recorder limits, stop-time reconciliation
  used, no attribution.
- **Case file:** no path/file-name hits; 4 correlated events; `+933 B` net
  metadata change in the captured case.
- **Windows comparison:** completed; review leads only, never a verdict.
- **Storage fixture:** the app removed exactly its 64.0 MB fixture and the
  receipt separated the estimate from the observed free-space change.
- **Companion:** 12 states reached; reduced motion left 0 running animations.
- **Process containment:** no new files were written next to the executable.

The native run uses a disposable temp workspace and removes only its own
fixture. Screenshots 12–13 are in `docs/screenshots/`. A path-bearing
case-view screenshot from an earlier run was removed rather than shipped.

## Browser evidence

`python gui/devtools/evidence-console.py verify` passed **595 checks**,
including:

- first-run and primary navigation;
- Agent Radar presence and non-attribution;
- session start/stop, event filtering and sequence identity;
- case preview, privacy audit and export state;
- Windows Change Radar wording;
- synthetic public demo behavior;
- LUMA states, keyboard access, petting and reduced motion;
- layout and accessibility checks.

## Verification gates

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | PASS — core 434, CLI 5, dirwatch 7, watch 58 |
| GUI `cargo fmt --check` | PASS |
| GUI `cargo clippy --all-targets -- -D warnings` | PASS |
| GUI `cargo test` | PASS — 10 passed, 1 ignored (interactive desktop test) |
| GUI `cargo build --release` | PASS |
| Browser `verify` | PASS — 595 checks |
| Native end-to-end | PASS — 48 checks |
| UTF-8 BOM scan | PASS — 0 of 197 scanned text files |
| `node --check` shipped JS | PASS — 16 of 16 |
| Remote network URL scan in `gui/dist` | PASS — 0 (SVG XML namespace declarations excluded) |
| `gui/dist` / `gui/dist-demo` copied assets | PASS — 13 of 14 byte-identical; `storage-workstation.css` is intentionally extended for the demo banner and `index.html` is intentionally rebuilt |

## Corrective changes in this review

- **Native CI race:** replaced the fixed 1.2 s wait with a pre-click
  MutationObserver and `wait_for_function` for the scan-progress signal.
- **Recorder deduplication:** a merged duplicate now keeps the newest
  confidence and rebuilds the note for the merged kind and confidence.
- **Reconciliation coverage:** a final snapshot with unreadable directories
  now marks coverage incomplete instead of claiming completeness.
- **Case-file sanitizer:** rejects whitespace-delimited absolute Unix and home
  paths in addition to Windows paths, file names and addresses.
- **Privacy wording:** UI, demo and documentation now distinguish
  engine-captured fields from the optional free-text operator note and warn
  that the note is not a secret scanner.
- **Demo build:** `tools/make-demo-frontend.py` now copies `agent.css` and
  `agent-radar.js`; the demo bundle was regenerated.
- **Evidence:** screenshots 11–13 were regenerated after the fixes. The
  path-bearing native case-view screenshot was removed.

## Evidence media

| Artefact | What it shows |
|---|---|
| `docs/screenshots/10-agent-radar.png` | Synthetic demo radar and live session. |
| `docs/screenshots/11-agent-case.png` | Synthetic demo case file. |
| `docs/screenshots/12-native-agent-radar.png` | Native radar: two detected agents, real four-event session. |
| `docs/screenshots/13-native-agent-case.png` | Native full case PNG, reconciliation used, no private fields. |
| `docs/screenshots/15-luma-before.png` | LUMA idle before the artwork refresh. |
| `docs/screenshots/16-luma-after.png` | LUMA idle after the artwork refresh. |
| `docs/media/agent-demo.mp4` | 25.44 s synthetic agent walkthrough. |

The native case PNG has SHA-256
`DEE26A7DC8F39FFB54FAE0D0635914FDC8F2E9F9F5F422FD1837743B809F2920`.
The walkthrough MP4 has SHA-256
`31D5388E26A191539BAAD3C3618C9C8AEB481500F85340855A5D7096108342C7`.

## Known limits and residual risk

- Detection is exact executable-name matching only. Wrappers, renamed
  binaries and unsupported agents are not identified.
- Correlation is process sampling plus filesystem timing, not ETW. A
  short-lived process between samples can be missed.
- Interrupted-session recovery is unit-tested at the session-model boundary.
  The actual crash/relaunch picker path was code-reviewed but not fault-injected
  in the native run.
- The OS-level watcher failure path is exercised by unit logic and code
  review; the final native run observed zero overflows, so the forced
  `ReadDirectoryChangesW` failure was not injected.
- Native agent-presence checks are conditional: CI can legitimately have no
  supported agent running. The captured run had two.
- The walkthrough is synthetic demo data, not a recording of a real workspace.
- Screenshot 12 contains a disposable test workspace path. It contains no
  user name or machine name.
- The optional operator note is free text. Path and address screening reduces
  accidental disclosure but cannot guarantee that a credential will never be
  typed; this remains an operator-responsibility limit.
- The exact baseline CI log could not be downloaded without authenticated
  repository access. The failure was reproduced locally at the same
  assertion, but CI-side log confirmation is still absent.
- LUMA screenshot 16 has a minor white tail-cap/right-cheek tangency during
  idle sway. It is a visual note, not a functional or acceptance failure.

## CI status

- Baseline `main` run: `39794547887`, commit `0fc6d77`, conclusion `failure`.
- Failing job: `Build GUI`; failing step: `Native end-to-end journey`
  (2026-10-22 14:30:02–14:31:38).
- All other baseline jobs/steps passed.
- Local cause: fixed-delay progress assertion raced a fast clean-profile scan.
- Corrected native check passes locally against the final release binary with
  isolated profile directories (48 checks).
- Branch CI was not run or checked; no push was made.
- **GO condition:** push the corrective change, then require the branch CI run
  to pass `Native end-to-end journey` and all other jobs. Without that run the
  verdict remains conditional.
