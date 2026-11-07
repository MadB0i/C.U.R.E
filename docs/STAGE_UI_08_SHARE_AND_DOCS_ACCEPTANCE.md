# Stage UI-08 — Share Result Card, Docs Refresh & Premium Sweep

**Date:** 2026-11-05
**Status:** COMPLETE (14/14 Native CDP Checks Passed + real agent-detection audit re-run)
**Binary Tested:** `gui/src-tauri/target/release/cure-gui.exe` (Real Tauri v2 / WebView2 native process)
**Verification Harnesses:** `gui/devtools/ui08-native-verify.py`, `testing/native/native-agent-audit.py`

---

## 1. Scope & Objective

1. **Share Result Card (MVP2 TASK 8)** — one click builds a PNG with aggregate
   counts only; the operator previews the exact file before it is saved.
2. **Docs refresh for real users** — README rewritten to match the actual product
   (the old one marketed the removed LUMA cat mascot and "Holographic Rescue
   Scanner"), fresh screenshots, corrected architecture docs.
3. **Premium sweep** — every emoji glyph removed from the shipped frontend
   (close buttons, check/warn icons, output markers), zero emojis repo-wide.

## 2. Key Changes Made

- **Share card (frontend)**: canvas builder in `app.js` (`buildShareCard`,
  1200×630) drawing six aggregate tiles — findings in last scan, flagged
  processes, canary alerts, actions taken, quarantined (session), session
  events — plus an honest scan-status line ("No scan has completed this session
  yet — these are session counters only") and a footer stating the card's
  limits. No paths, no names, no secrets. Preview modal (`#share-overlay`)
  shows the exact PNG (`data:image/png;base64`) with Save/Cancel/Escape and
  focus management; save is recorded in the session feed.
- **Share card (backend)**: new `#[tauri::command] share_card_png(name,
  data_base64)` reusing the `decode_case_file_png` guard (PNG signature + size
  caps) and the fixed `case-files/` destination; filename reduced to
  `[A-Za-z0-9_-]`. Registered in the invoke handler; mirrored in
  `mock-tauri.js` where the public demo **rejects the write** (preview-only
  boundary, same as case files).
- **Premium sweep**: `✕` close buttons → text "Close"; `"Copied ✓"` →
  `"Copied"`; `[✔]` output markers → `[OK]`; storage-rescue check/warn glyphs →
  inline SVG marks; agent-radar "✔ Window Closed" → "Window Closed".
  `tools/audit_ui_cleanliness.py` now reports **0 emoji lines** (was 23).
- **Docs**: README.md rewritten (capabilities, CLI examples, honest limitations,
  corrected architecture tree, fresh screenshots); stale LUMA-era media removed
  (`cure-preview.gif`, `cure-demo.mp4`, luma-era PNGs) and replaced with
  current-stage captures (`docs/media/cure-*.png`); `docs/ARCHITECTURE.md`
  corrected (no `luma.js`, four new CLI commands, demo IPC description);
  `gui/devtools/README.md` suite list corrected (mascot absence, agent radar,
  share card).

## 3. Native CDP Verification Results (14/14 PASS)

| # | Check | Result |
|---|---|---|
| 1 | Share button exists on the Overview | PASS |
| 2 | Preview modal opens | PASS |
| 3 | Preview shows the exact PNG (data URL, 98 KB) | PASS |
| 4 | Save button focused for keyboard flow | PASS |
| 5 | Save reports a path | PASS |
| 6 | Saved file exists on disk | PASS |
| 7 | Saved file is a real PNG (signature check) | PASS |
| 8 | Saved under the session data dir (`case-files/`) | PASS |
| 9 | Saved file is a plausible card size (73 KB) | PASS |
| 10 | Export is recorded in the session feed | PASS |
| 11 | Session counter reflects the export | PASS |
| 12 | Modal reopens | PASS |
| 13 | Escape closes the modal | PASS |
| 14 | Cancel closes the modal | PASS |

## 4. Real agent-detection audit (100% real, fixture-only termination)

`testing/native/native-agent-audit.py` re-run against the same real binary:

- Spawns its **own** fixture process (`opencode.exe`, a renamed interpreter
  sleeping with fake `--api-key`/`--token`/Bearer arguments) and nothing else.
- The **real** `agent_radar` backend must detect it **by PID**; identity
  hardening asserts the renamed binary is reported `Unverified`/`SuspectMismatch`
  and never "Verified genuine agent".
- Secret masking is asserted: no raw `sk-*`/`ghp_*`/Bearer values anywhere in
  the backend response.
- Termination touches **only the harness's own child** (PID-reaped,
  lease-file ownership verified — name-based killing is banned repo-wide by
  `core/tests/qa_process_safety.rs`). No other process — including any live AI
  agent on the machine — is ever signaled.

## 5. Screenshot Evidence

`testing/native/screenshots-ui08/native/` + `docs/screenshots/ui08/`:
`01-share-card-preview.png`, `02-overview-with-share-entry.png`.

## 6. Gates

- `cargo fmt --check` clean (both workspaces); clippy `-D warnings` clean;
  root workspace 598 tests pass; GUI crate 14 tests pass.
- `tools/audit_ui_cleanliness.py`: **0 emoji lines**; remaining "mascot"
  matches are the functional operator posture widget (deliberately kept).
- `gui/dist` ↔ `gui/dist-demo` shared assets byte-identical; dev shell exact.
