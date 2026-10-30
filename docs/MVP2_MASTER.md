# C.U.R.E MVP 2.0 — MASTER SPEC (all features)

How to use this file: it is the full spec. You will be told which TASK to execute.
Execute ONLY that task. Never begin the next one. Respect every rule in AGENTS.md.
If a task conflicts with a product principle, STOP and ask.

## Cross-cutting requirements (apply to every task)
- Every feature has: core logic (core crate), a CLI command with --json output
  following existing CLI conventions, and a GUI screen/panel following existing design.
- Every list/screen handles: loading, empty, error, and INCOMPLETE states honestly.
- Non-admin vs admin: work without admin where possible; if something needs admin,
  show INCOMPLETE with the reason, never fail silently, never pretend it was checked.
- Long operations: run off the UI thread, emit progress, be cancellable.
- Evidence: every new finding uses the explicit Evidence constructors
  (observed / correlated / unknown). No implicit defaults.
- Paths: handle long paths, spaces, non-ASCII (Assamese/Hindi usernames), and missing/denied folders.
- Tests: use fake fixtures under testing/. Never read real user data in tests.
- GUI: keep gui/dist and gui/dist-demo in sync; mock backend must mirror new commands.

## TASK 1B — Evidence model hardening (skip if already DONE)
- No Default for EvidenceLevel. Constructors only: Evidence::observed/correlated/unknown(detail).
- Existing findings get Observed with a detail describing WHAT was directly observed.
- Reject/sanitize causal wording for correlated/unknown at construction time, in production code:
  caused by, created by, written by, dropped by, installed by, launched by, started by,
  changed by, modified by, deleted by, responsible for, did this (case-insensitive). Test each.
- Old baselines/reports without the evidence field must still load (serde default that
  does NOT claim Observed). Add old-format fixtures + tests.
- Merge or document reason() vs plain_english_reason().
Done when: old fixtures load, no implicit Observed, all checks pass.

## TASK 2 — AI Agent Radar (read-only)
Goal: show which AI coding agents/tools exist or are running on this PC and what they can reach.
Detect (metadata only, nothing executed):
- Running processes: codex, opencode, claude, cursor, aider, cline, gemini-cli, continue
  (match image name + command line; mask secrets inside command lines).
- Agent home/config folders: %USERPROFILE%\.codex, .claude, .config\opencode, relevant
  %APPDATA%/%LOCALAPPDATA% folders.
  Report existence, last-modified time, size class. Do not read conversation/history contents.
- MCP server inventory: parse claude_desktop_config.json, .mcp.json, .cursor\mcp.json, other
  known locations. For each server show: name, command, args, transport (stdio/http),
  and observed facts such as "launcher is npx/uvx/docker/powershell", "args contain filesystem root C:\",
  "remote URL present", env var NAMES only. Facts, not verdicts. Never execute any server.
- Secret exposure: scan known agent config files and .env files in user-selected folders for key-like
  values (common provider prefixes, long high-entropy tokens). Report: key type guess, file, "masked".
  NEVER store/log/display any character of the value. Skip files above a size limit; respect INCOMPLETE.
- VS Code/Cursor extensions: Copilot, Continue, Cline and similar (read extension manifests only).
- Persistence: agent-launching startup entries / scheduled tasks (reuse existing persistence scanner).
UI: Agent Radar screen with grouped cards (Running / Installed / MCP servers / Secret exposure / Persistence),
evidence badge on every row, "last checked" time, and an INCOMPLETE section listing what could not be read.
CLI: `cure agents [--json]`.
Tests: fake config fixtures for each agent/MCP format, malformed JSON, empty machine, secret masking proof.
Done when: works with zero agents installed and with several; no secret value appears anywhere in output/logs.

## TASK 3 — Change Radar (snapshot diff)
- User selects folders, takes Snapshot A, later Snapshot B. Store per file: path, size, mtime, content hash
  (hash only files under a configurable size limit; larger = metadata only, say so).
- Diff: Added / Modified / Deleted / Renamed. Rename detection is best-effort and labelled Correlated.
- Stored under existing --data-dir. No file contents stored. Snapshot list with delete option.
- Exclusion list (node_modules, target, .git objects optional toggle) to keep large folders manageable.
- CLI: `cure change snapshot <folder>`, `cure change list`, `cure change diff <a> <b> [--json]`.
- GUI: simple two-step flow (take snapshot -> compare), grouped diff view with counts, filter box.
Tests: create/edit/delete/rename detection, large-folder non-blocking, cancel mid-scan, denied subfolder -> INCOMPLETE.

## TASK 4 — Flight Recorder (opt-in activity log)
Prerequisite inside this task: write a short mapping between the incident module's
DIRECT/STRONG/PARTIAL/WEAK verdicts and Observed/Correlated/Unknown, document it in docs/, and use ONE
vocabulary in the UI.
- OFF by default, nothing starts at launch. Clear toggle + always-visible "recording" indicator.
- Watch only user-chosen folders (reuse dirwatch). Record: path, event type, timestamp.
- Process attribution: the basic file-watch API usually does NOT identify the process. So by default
  process = Unknown. If an agent process (from Agent Radar) was running during the event window,
  mark Correlated with the text "an agent process was running at that time" — never "caused by".
  Observed only if attribution is genuinely provable. Do NOT add ETW/driver-level tracing in this task.
- Local-only, size-capped log with rotation, one-click "clear history", export via Case File later.
- CLI: `cure recorder start|stop|status|clear`, GUI timeline view with filters (time, folder, evidence level).
Tests: toggle on/off, cap + rotation, events without process info stay Unknown, wording enforcement.

## TASK 5 — Case File export
- Self-contained HTML (no network, no external fonts/scripts) + JSON.
- Sections: summary, system context (sanitized), Agent Radar findings, Change Radar diff,
  Flight Recorder timeline, evidence levels explained, "What C.U.R.E could NOT verify".
- Sanitize by default: usernames in paths -> <USER>, machine name masked, secrets removed entirely,
  no file contents. Provide a PREVIEW of exactly what will be exported before saving.
- Optional toggle for "include raw paths" with a clear warning.
- CLI: `cure casefile export --out <path> [--json]`.
Tests: username/machine name/secret values never appear in default output; opens fully offline.

## TASK 6 — Storage Intelligence 2.0 (improve existing, do not rewrite)
- Faster scan: parallel directory walk, live progress, cancellable. If admin, optional faster
  enumeration path; always keep a non-admin fallback.
- Duplicates: group by size -> partial hash -> full blake3. Never auto-delete. Actions go through
  existing quarantine/confirmed cleanup flow, showing exact paths first.
- "Developer leftovers": node_modules, target, .venv, __pycache__, build/dist caches. Show size per
  project + last-modified date, confirmation required, never touch a project modified recently unless user confirms.
- Treemap: drill-down on click, size labels, breadcrumb back, readable at 1366x768 and 1920x1080.
- Optional view: "folders changed in last 24h" (can link to Change Radar).
- Report before/after scan time on a large folder.
Tests: duplicate grouping, partial-hash collision handling, leftover detection, cancel, denied folders.

## TASK 7 — LUMA anime cat makeover
- Requires reference sheet in design-ref/luma/. If missing: STOP and tell me.
- Keep existing animation engine and state machine; swap art only.
- Layered SVG parts (head, ears, eyes, tail, body, paws). Orange-and-white anime-style cat. No copying existing characters/IP.
- States: idle, sleepy, scanning (ears up + magnifier), alert, threat-caught, cleaning, celebrate. Keep existing Hide control.
- Respect prefers-reduced-motion. Keep file size reasonable.
- Capture screenshots of every state at 1366x768 and 1920x1080, review them yourself, fix anything amateur,
  and list what you still judge weak. Save under docs/screenshots/luma/.

## TASK 8 — Premium UI polish + share card + demo + README
- Consistent spacing/typography/colors across Agent Radar, Change Radar, Recorder, Case File, Storage.
- Share result card (PNG): aggregate numbers only (e.g. agents found, MCP servers, files changed),
  no paths, no names, no secrets. Preview before saving.
- Demo flow using the labelled mock backend: Agent Radar -> Change -> Evidence timeline -> Case File.
  Produce screenshots + short GIF/MP4 under docs/media.
- README: update features, evidence-first explanation, honest limitations. Do NOT edit About/topics or release notes.

## NOT IN SCOPE FOR ANY TASK ABOVE
CI fixes, GitHub Actions, About/topics, version bump, release notes, signing, publishing, launch posts.
Those are handled later by me in one final release pass.
