# Changelog — C.U.R.E

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
