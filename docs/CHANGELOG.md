# Changelog — C.U.R.E

## 2026-10-04 — UI-REDESIGN (Rescue Console: System X-Ray, Luma, Disk Cleanup)

**Visual identity replaced. No backend behaviour changed.** No `#[tauri::command]`,
no event name or payload, no safety semantic, no coverage reporting and no
honesty wording was touched. Every `invoke` call site is byte-identical.

**Removed:** the circular 01–09 Evidence Sweep radar, the small bordered Luma
card artwork, the numbered `01…05` sidebar, and the old token set.

**Replaced with:**

- `gui/dist/xray.js` + `xray.css` — **SYSTEM X-RAY**. A sectioned machine:
  nine persistence-layer plates on a left rail, a bounded chassis board with
  deterministic etched routing, the system core with border pins and a live
  collector readout, orthogonal persistence-bus routes, and an inspection beam
  that travels to whichever collector the backend reports as running. Layer
  state maps the real coverage states verbatim (`Checked`, `Partial`,
  `AccessDenied`, `CheckFailed`, `Unavailable`, `NotChecked`).
- `gui/dist/luma.js` + `luma.css` — **Luma** rebuilt as an original articulated
  vector operator with ten states: `idle`, three rotating scan variants
  (`scan-console`, `scan-scanner`, `scan-lens`), three rotating cleanup
  variants (`clean-sweep`, `clean-sort`, `clean-recycle`), `success`, `review`
  and `seal`. Every state has a distinct static pose, so reduced motion loses
  motion and never meaning.
- `gui/dist/storage-core.js` + `storage.css` — **Segmented Storage Core** for
  Disk Cleanup: one arc and one proportional bar per real engine category,
  sized from `scan_cleanup` totals, with the reclaimed arc drawn from the real
  `bytes_freed` against the real pre-cleanup total.
- `style.css` — graphite depth ramp, hairline highlights, local ambient
  illumination, restrained violet identity accent. Evidence views gained a
  measured column, risk rails and real score meters.
- Disk Cleanup is a **primary destination** (`#nav-cleanup`), reachable without
  going through findings.

**Navigation is icon + title only.** The `01…05` labels are gone.

**Honesty preserved:** no fabricated percentages, bytes, counts or coverage.
`Access denied` never renders as green, cleanup never invents progress, the
confirmation dialog and evidence inspector still contain no assistant artwork,
and Overview's disk figure reports only what this session really measured.

**Verification:** `gui/devtools/evidence-console.py all` — 428 checks. The
blanket "no gradient" string ban was replaced with measured pixel gates
(violet share, ≤3 dominant saturated hue families, dark surface, tonal depth),
plus an offline asset guard and production-allowlist parity check. Rust
workspace and GUI tests, `cargo fmt --check` and strict clippy all pass; the
release WebView2 binary embeds exactly the new asset set.

## 2026-09-24 — F-LOLBIN-1 (signed script hosts scored Safe)

**Found by read-only probe on the same box.** A signed Microsoft LOLBin
with suspicious arguments scored `0 Safe`: `-20 trusted` + `-40 Valid`
cancelled `+25`/`+30` heuristics (e.g. `powershell -Hidden -EncodedCommand`,
`rundll32 …Temp\evil.dll`, `mshta http://…` — all Safe under Valid).

**Fix** (`core/src/risk.rs`, Run-key and `.lnk` together): known LOLBins
(powershell/pwsh/cmd/mshta/rundll32/regsvr32/wscript/cscript/msbuild) with
any command-line heuristic firing get **neither** discount — heuristics
decide; `%VAR%` expands across the whole command first; one new
LOLBin-gated remote-arg heuristic (URL/UNC → +25, floor Suspicious).
Benign uses (plain `-File`, local `.hta`, `shell32.dll,Control_RunDLL`)
stay `0 Safe`. Live FP check on this PC: 114 entries, **0 verdict
changes**. See `docs/checks/2026-09-24-flolbin-1-fix.md` for the full
before/after table.

**Baseline-id note:** F-LNK-1 changed Startup `.lnk` `command` from the lnk
path to `resolved target + args`, and `make_id` hashes `command` — so old
`.lnk` entries appear as **removed + added** in `baseline.json` after
upgrading. Quarantine records are unaffected (`original_path` is still the
lnk file; old ids still undo — locked by regression test
`old_lnk_record_with_stale_command_id_still_undoes`).

## 2026-09-24 — F-LNK-1 (Startup .lnk false-negative)

**Found during real-PC validation on a standard-user Windows 11 box.** The Startup scanner stored the `.lnk` file path itself as `entry.command` (and thus scored it), not the shortcut's target. Two bugs combined:

1. **LinkInfo flag guard** (`core/src/lnk.rs:155`): required `0x1|0x10` while the spec defines only `0x1`. Shell-written shortcuts carry `0x1` alone, so the absolute `LocalBasePath` was discarded and the relative fallback `.\target.exe` was used — display showed `..\..\… [MISSING]` and `is_file` checked relative to `cwd`, not the link.
2. **Scanner separation** (`core/src/scanners/startup.rs:69`): `command == location == lnk path` for every Startup entry. Scoring (`risk.rs:83`, `cli 339`, `gui 274`) and reports therefore used the lnk path's heuristics/signature/hash, not the target's — an unsigned payload in Temp via a Startup lnk incorrectly scored Safe (false-negative).

**Fix:** `f5a5c05` corrects the flag to `0x1`; `68af08a` makes the scanner set `location = lnk path` (what quarantine moves) and `command = resolved absolute target + args` (what gets scored), via `IShellLink::GetPath(SLR_NO_UI|NOUPDATE|NOSEARCH)` first with raw MS-SHLLINK fallback, env-var expansion, relative→absolute via workdir/parent, and long-path expansion. `9911a05` expands 8.3 short names for stable display. Display now shows long absolute `lnk→: C:\… [exists]` and scoring uses the target's Valid/Unsigned/Invalid, drop-zone, and hash IOC. Added `68af08a` consumer audit and 8 `scanners::startup::tests` (unsigned vs signed, powershell+args vs Run key, relative/env-var/args/missing/hash) — all 284 core tests pass.

**Impact:** Detection false-negative closed; quarantine/undo still act on the .lnk file (location), never the target — verified live with `CURE_TEST` kit after fix.
