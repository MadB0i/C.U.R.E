# MVP acceptance — Agent Intelligence

**Date:** 2026-10-23
**Branch:** `feat/agent-intelligence-mvp`
**Baseline:** `0fc6d77` (working tree, uncommitted)
**Release binary:** `gui/src-tauri/target/release/cure-gui.exe`
**Binary SHA-256:** `BA63795BB1C14DB07314702C999C0FEA92F1CE8374582C558EB1E7BB98946FAF`

## Verdict

Local acceptance for the Agent Intelligence MVP is **PASS**. The browser
suite, core tests, GUI tests, native end-to-end run, release build, privacy
audits and evidence media all pass.

The shared `main` CI is still **BLOCKED**: baseline run `39794547887` at
`0fc6d77` fails in `Build GUI` at the `Native end-to-end journey` step. The
same native journey passes locally against the final release binary. Branch
CI could not be checked because `gh` is unauthenticated.

No commit, push, merge or release was performed.

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
| 5 | Coverage must be honest, including reconciliation and limits. | **PASS** | Native coverage: baseline captured, watcher started, `snapshot_reconciled: true`, complete, zero overflow/dropped events. Case label reads `Reconciliation used: yes`. |
| 6 | Refuse a drive root as a workspace. | **PASS** | Native IPC call with `C:\` returned: `Choose a project folder, not an entire drive. Monitoring a drive root is too broad.` |
| 7 | Recover an interrupted session without inventing a clean stop. | **PASS** | `interrupted_session_is_recovered_as_incomplete` verifies stopped state, last-observation end time, incomplete coverage and the reconciliation warning. |
| 8 | Agent Case File contains no paths, file names, prompts, credentials, user or machine names. | **PASS** | Native model audit returned `[]`. Core and browser privacy tests pass. Case PNG contains no private fields. |
| 9 | Preview the case before local export; export survives live refresh. | **PASS** | Native and browser runs preview and export. Browser regression keeps Export enabled after a 3.5 s refresh. |
| 10 | Windows Change Radar compares persistence without calling changes malware. | **PASS** | Native comparison completed; browser shows two review leads; UI states these are not malware verdicts and offers no remediation. |
| 11 | Public demo remains synthetic and cannot write a real case file. | **PASS** | Agent export in the public demo returns `preview-only and are never written`; demo case carries a SYNTHETIC DEMO band. No remote requests were observed. |
| 12 | LUMA artwork refresh keeps every authored state and reduced-motion contract. | **PASS** | Native run reached all 12 states and stopped every animation under reduced motion. Browser checks confirm the rig, palette, petting, keyboard access and pose separation. |
| 13 | Native screenshots show the real release build, not the mock. | **PASS** | Screenshots 12–14 were captured from the final release binary. Screenshot 13 is a full 1200×1500 case PNG rendered from the real `caseDataUrl`. |
| 14 | A 25–30 second agent walkthrough exists. | **PASS** | `docs/media/agent-demo.mp4`, 25.44 s, synthetic public demo. |
| 15 | Formatting, linting and tests pass. | **PASS** | See the gate table below. |
| 16 | Branch CI is green. | **BLOCKED** | `gh` is unauthenticated; baseline shared CI fails on a step that passes locally. |

## Native evidence

Final native run: **48 checks passed**.

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
fixture. Screenshots 12–14 are in `docs/screenshots/`.

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
| `cargo test --workspace` | PASS — core 431, CLI 5, dirwatch 7, watch 58 |
| GUI `cargo fmt --check` | PASS |
| GUI `cargo clippy --all-targets -- -D warnings` | PASS |
| GUI `cargo test` | PASS — 10 passed, 1 ignored (interactive desktop test) |
| GUI `cargo build --release` | PASS |
| Browser `verify` | PASS — 595 checks |
| Native end-to-end | PASS — 48 checks |
| UTF-8 BOM scan | PASS — 0 of 160 text files |
| `node --check` shipped JS | PASS — 16 of 16 |
| Remote URL scan in `gui/dist` | PASS — 0 |
| `gui/dist` / `gui/dist-demo` shared assets | PASS — 6 shared files byte-identical |

## Evidence media

| Artefact | What it shows |
|---|---|
| `docs/screenshots/10-agent-radar.png` | Synthetic demo radar and live session. |
| `docs/screenshots/11-agent-case.png` | Synthetic demo case file. |
| `docs/screenshots/12-native-agent-radar.png` | Native radar: two detected agents, real four-event session. |
| `docs/screenshots/13-native-agent-case.png` | Native full case PNG, reconciliation used, no private fields. |
| `docs/screenshots/14-native-agent-case-view.png` | Native case preview inside the application. |
| `docs/screenshots/15-luma-before.png` | LUMA idle before the artwork refresh. |
| `docs/screenshots/16-luma-after.png` | LUMA idle after the artwork refresh. |
| `docs/media/agent-demo.mp4` | 25.44 s synthetic agent walkthrough. |

The native case PNG has SHA-256
`C4FBD70B35D21041119CA89BC161C516035DCF2E6FF98B43CF7537BA9F887C59`.
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

## CI status

- Baseline `main` run: `39794547887`, commit `0fc6d77`, conclusion `failure`.
- Failing job: `Build GUI`; failing step: `Native end-to-end journey`.
- All other baseline jobs/steps passed.
- Local final release binary passed the same native journey (48 checks).
- Branch CI was not run or checked; no push was made.
