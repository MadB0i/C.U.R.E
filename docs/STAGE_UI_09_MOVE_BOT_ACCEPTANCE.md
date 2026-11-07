# Stage UI-09 — Move Bot (one-click C: → D:) & Human-Language Navigation

**Date:** 2026-11-05
**Status:** COMPLETE (19/19 Native CDP Checks + 15/15 Mock-Flow Journey Checks Passed)
**Binary Tested:** `gui/src-tauri/target/release/cure-gui.exe` (Real Tauri v2 / WebView2 native process)
**Verification Harnesses:** `gui/devtools/ui09-native-verify.py`, `gui/devtools/ui09-mock-flow-verify.py`

---

## 1. What the operator asked for, in their own words

"Scan AI agents, scan the C drive, and if the user sees something big, one click
and the bot moves it from C: to D: and does the rest. Premium look. Don't
rebuild every button — just make the titles and navigation speak human."

## 2. What was built

### The Move Bot (`gui/dist/move-bot.js`, new)

One button — **"Move it for me — one click"** (Rescue tab of the Storage
Workstation) and **"Free up C: drive — let me handle it"** (Detective Mode
header). After that single click the bot does everything:

1. Reads the real drives and safe-move candidates.
2. Shows **one** confirmation card in plain words: which folders, how big,
   where they go, and the promise — *"I copy everything first, check it bit by
   bit, then switch Windows over. Nothing is deleted until the copy is fully
   verified, and you can undo."*
3. Optional tick-box (default **off**): *"After the copies are verified, also
   free the space on C: (removes the originals)."*
4. Then, hands-free: copies with a live progress bar (real
   `rescue-migration-progress` events), verifies checksums, switches Windows'
   known folders to the new location (`storage_rescue_activate`), and — only if
   ticked — frees the space on C: (`storage_rescue_cleanup_source` with the
   activation nonce).
5. Done card in plain words: *"All moved and verified. 127.2 GB now lives on
   drive D:. I also freed 73.8 GB on C:."*
6. Any engine failure stops the bot safely in human words ("I stopped before
   anything changed. Your files are exactly where they were.") with a
   one-click Undo of the switchover where one exists. Escape is refused while
   a move is running — the bot never abandons work mid-flight.

It drives exactly the same engine commands as the manual Rescue flow — no new
powers, no new backend surface; it is orchestration and plain language only.

### Human-language navigation pass (text only; every ID and feature preserved)

- Investigate lenses: "Running Process Sentinel" → **Running programs**;
  "Windows Change Radar · Trusted Snapshot Comparison" → **System changes**;
  "Ransomware Indicators & Tripwires" → **Ransomware signs**;
  "Nine-Layer Inspection Coverage" → **What was checked**. Buttons:
  "Capture trusted snapshot" → **Save snapshot**, "Compare now" → **Compare**.
- Rescue panel: the operator console now speaks as the **Move Assistant**
  ("Your C: drive is running out of room. Pick the folders below and I will
  move them…"); step chips read *Measured your drives / Pick what to move /
  Safety checks / Verified copy / Switch over / You approve cleanup*; segments
  read **SAFE TO MOVE · Your personal folders**, **MOVABLE, NEEDS A TWEAK ·
  Developer tools**, **PROTECTED · NEVER MOVED · Windows itself**; action bar
  reads "Ready to move / Space freed on C:", with **Pick folders myself**
  (manual flow kept) beside the bot's primary button.
- Assistant guide text humanized in `storage-rescue.js`.

## 3. Verification results

### Native, real backend (19/19 PASS — read-only by design)

| Check | Result |
|---|---|
| Human titles present in Investigate ("Running programs", "System changes", "Ransomware signs", "What was checked") | PASS ×4 |
| Old jargon titles gone | PASS |
| Rescue one-click bot button exists and speaks human | PASS ×2 |
| Manual multi-step flow still available beside the bot | PASS |
| Action bar speaks human | PASS |
| Bot opens and finishes its **real** drive check | PASS |
| Confirmation card lists the machine's **real** folders (Downloads Library 264.2 MB, Videos & Captures, Documents Library, Desktop Stash — 425.4 MB total) or states honestly when there is nothing safe | PASS |
| Plain-words promise + cleanup option default OFF | PASS ×2 |
| Cancel closes the bot, nothing moved | PASS |
| Detective entry exists, speaks human, opens the same bot, closes | PASS ×4 |
| Light theme + 1366×768 screenshots | PASS |

### Full journey, labelled mock shell (15/15 PASS)

The native run stops at the confirmation gate on purpose — executing it would
move the real machine's personal folders. The mock shell completes the entire
journey using the exact commands and event shapes the real engine exposes:

| Check | Result |
|---|---|
| Bot opens; 3 safe folders listed | PASS ×2 |
| One click (with "also free the space" ticked) → step timeline shows | PASS ×2 |
| Escape refused while working | PASS |
| Done card: *"All moved and verified. 127.2 GB now lives on drive D:. I also freed 73.8 GB on C:."* | PASS |
| Every moved folder listed, all targets on D: | PASS ×2 |
| Finish closes; status refreshes | PASS |
| **Failure path**: engine error → "I stopped — your files are safe … Your files are exactly where they were." → error card closes | PASS ×3 |
| Zero page errors across the whole journey | PASS |

## 4. Screenshot Evidence

`testing/native/screenshots-ui09/` + `docs/screenshots/ui09/`:
`01-move-bot-confirm-real.png` (real machine data), `02-rescue-light-theme.png`,
`03-rescue-1366x768.png`, `mock-flow-done.png`.

## 5. Gates

- No Rust changes in this stage (frontend orchestration only); the release exe
  was rebuilt with the new frontend and the harnesses ran against it.
- `tools/audit_ui_cleanliness.py`: 0 emoji lines. `gui/dist` ↔ `gui/dist-demo`
  byte-identical (incl. `move-bot.js`); dev shell exact; `move-bot.js`
  registered in both Tauri asset allowlists and the demo copy list; the mock
  backend already mirrored every command the bot calls (write paths still
  refuse in the public demo).
- Safety: no process is terminated by either harness; the native run moves
  nothing (stops at the confirm gate).
