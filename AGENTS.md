# C.U.R.E — rules for coding agents
Project: portable Windows security toolkit in Rust (workspace crates: core, dirwatch, watch, CLI, plus Tauri GUI crate). GUI assets in gui/dist and gui/dist-demo (keep byte-identical). Test fixtures in testing/. Full feature spec in docs/MVP2_MASTER.md.

Product principles (non-negotiable):
- Local only. No telemetry, no network calls for scanning.
- No auto-scan: scanning starts only from an explicit user click/command.
- Move, never delete: cleanup goes to quarantine/Recycle Bin with confirmation.
- Honesty over comfort: unreadable areas show INCOMPLETE, never "clean".
- Evidence-first: every finding carries an Evidence (Observed/Correlated/Unknown) created via the explicit constructors. Correlated/Unknown text never uses causal wording.
- Never claim an AI agent did something unless it is Observed.
- Secrets: never store, log or display secret values; report "found, masked" only.
- Exports are sanitized by default (usernames/paths).

Hard rules for every task:
1. Do ONLY the task in the current message. Do not start other roadmap items.
2. No unrelated refactors, renames or reformatting.
3. Do not touch .github/workflows, version numbers, About/topics, release notes.
4. Do not push, tag, publish or release. Commit locally on branch mvp2/<task-name>.
5. No new heavy dependencies without explaining why first.
6. If ambiguous or conflicting with a principle: STOP and ask.
7. Every new logic gets tests using fake fixtures under testing/, never real user data.

Definition of done:
- cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace pass (plus GUI crate checks if GUI touched).
- For UI changes: capture screenshots and review them yourself, fix anything amateur.
- Final report MUST include: files changed, what was verified, "What is NOT verified" section, risks/follow-ups. Be blunt.

CURRENT STATUS: Task 1 and 1B done. Next: Task 2.
