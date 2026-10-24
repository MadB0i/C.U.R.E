# Agent Intelligence

Agent Radar is an **observation** tool for local coding agents. It detects
supported processes, records workspace metadata changes while a session is
running, and compares Windows persistence before and after. It does not read
prompts, transcripts, source files, credentials or agent memory, and it never
uploads anything.

## What is detected

| Agent | Exact executable names |
|---|---|
| OpenCode | `opencode.exe`, `opencode-cli.exe`, `opencode-tui.exe` |
| Codex CLI | `codex.exe`, `codex-cli.exe`, `codex-tui.exe` |

Detection is by executable name only. A renamed binary, a wrapper, a remote
agent or an unsupported tool is not identified. Process presence is shown as
presence: it does not prove that the agent is generating code, running a tool
or changing a file.

## Flight Recorder

Starting a recording captures a metadata-only baseline of the selected
workspace (path, size, modification time; up to 50,000 files), then watches
for created, modified, removed and renamed entries. Drive roots are refused.
Rapid duplicate notifications are coalesced, renames are paired, and the event
list is capped at 5,000 observations. Directory read failures and buffer
overflows are reported as coverage gaps rather than dropped silently. Final
snapshot reconciliation adds only changes the watcher did not already
record; it does not replay duplicates.

The session stores no file contents. It records whether a supported agent
process was present at the observation time. If the application closes while
a session is still recording, the next launch reloads the newest session as
**stopped and incomplete**, with a note that stop-time reconciliation did not
run. It does not invent a clean ending.

### Evidence language

Every event carries one of these states:

| State | Meaning |
|---|---|
| **Observed** | The change was seen while no supported agent process was present. |
| **Correlated** | The change was seen while a supported agent process was present. Correlation is not causation. |
| **Attributed** | Reserved for trustworthy per-process instrumentation. This build does not produce it. |
| **Unknown** | Coverage or timing was incomplete, so no relationship can be stated. |

The UI and case file say "correlated, not attributed" wherever that is the
strongest claim the data supports.

## Windows Change Radar

`Capture trusted snapshot` stores the persistence entries from the existing
read-only Rescue scan. A later compare reports **added**, **removed** and
**changed** entries across Run keys, Startup folders, scheduled tasks,
services, WMI subscriptions, IFEO entries, AppInit DLLs and COM
registrations. Each lead carries its before/after command, location, review
hints and an evidence statement.

These are review leads, not malware verdicts. A new startup entry can be an
installer, an update or an administrator's deliberate change. C.U.R.E never
modifies, disables or removes persistence from this view.

## Agent Case File

The Agent Case File is a PNG rendered from an allowlisted model. The model
contains labels, counts, timestamps, evidence-state counts, scoped Windows
change counts and an optional operator summary. It has no field for a
workspace path, file name, prompt, transcript, command line, user name or
machine name. A summary that looks like a path or address is withheld. The
note is free text, not a secret scanner; do not type credentials into it.

The public demo case file is marked **SYNTHETIC DEMO** and can be previewed
but not written.

## Commands

| Command | Purpose |
|---|---|
| `agent_radar` | List supported processes and the current session, if any. |
| `flight_start` | Validate a project folder and begin a recording. Refuses a second concurrent session and drive roots. |
| `flight_status` | Read the current session and its bounded event list. |
| `flight_stop` | Stop, reconcile the final snapshot and persist the session. |
| `windows_change_capture` | Save a trusted persistence snapshot. |
| `windows_change_compare` | Re-run the read-only persistence scan and compare. |
| `agent_case_file` | Build the privacy-safe case model for a stopped session. |

Session files are stored under the application data directory. Nothing is
sent over the network.

## Limits

- Only the six executable names listed above are recognised.
- File changes are inferred from metadata, not from an agent's own event
  stream, so a same-size rewrite within the timestamp resolution can be
  missed.
- Correlation depends on process sampling, not ETW. A short-lived process
  between samples can be missed.
- Windows persistence coverage is exactly the persistence coverage of the
  Rescue scan; incomplete rows remain incomplete.
- A session interrupted by an application exit is recoverable for review,
  but its coverage is incomplete because stop-time reconciliation did not run.
- The recorder observes a workspace, not an agent's internal tool calls.
  Process presence, filesystem timing and Windows persistence changes are
  separate evidence surfaces.
- The recorder is not a sandbox, an antivirus or an audit log for compliance.

## Verification

The browser acceptance suite covers process presence, explicit session
start/stop, event filtering, evidence wording, case-model path leakage,
Windows snapshot compare and the public demo's synthetic labels. The native
end-to-end run passes 48 checks against the release binary, including
drive-root refusal, a real four-change flight session with complete coverage
and a privacy-clean case file. The core tests cover detection, baseline/event
reconciliation, session limits, interrupted-session recovery, persistence
comparison and case-file sanitizing.

The synthetic walkthrough is `docs/media/agent-demo.mp4` (25.4 s). Screenshots
`docs/screenshots/12-native-agent-radar.png` and
`docs/screenshots/13-native-agent-case.png` are from the native release run.
