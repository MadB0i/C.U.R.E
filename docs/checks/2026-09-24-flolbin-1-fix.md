# F-LOLBIN-1 fix — signed script hosts with suspicious args (2026-09-24)

Branch `validation-and-docs`. No scoring rule other than the below was touched.

## What "-20 trusted" is, and the thresholds

`-20 trusted` fires when the command string contains a whole `Program Files` /
`System32` / `SysWOW64` component (`path_has_token`, `core/src/risk.rs`): the
binary lives in a trusted install location. It is a *location* signal, not a
verdict on the binary. Thresholds (`risk_level`): **<15 Safe, 15–39
Suspicious, ≥40 HighRisk.**

## The bug

A signed Microsoft LOLBin with malicious-looking arguments scored Safe
because `-20 trusted` + `-40 Valid` cancelled the heuristic (`+25`/`+30`).
Probe outputs (pre-fix, `score_with_signals`, Run-key and `.lnk` identical):

- `powershell.exe -WindowStyle Hidden -EncodedCommand …` + Valid → **0 Safe**
- `cmd.exe /c %TEMP%\payload.exe` + Valid → **0 Safe** (literal `%TEMP%` never expanded)
- `mshta.exe http://…` + Valid → **0 Safe** (no heuristic fired at all)
- `rundll32.exe …Temp\evil.dll` + Valid → **0 Safe**

## The fix (`core/src/risk.rs`)

1. `LOLBIN_NAMES` (9 hosts incl. `regsvr32.exe`) + `is_lolbin_program`
   (exact file-name match on the normalized program path).
2. When the program is a LOLBin **and** a command-line heuristic fires
   (`drop_zone`, sneaky-PowerShell, or the new remote-arg), **neither**
   the trusted (−20) **nor** the Valid (−40) discount applies; a scoreless
   `trust discounts withheld` evidence reason is added instead. Penalties
   (Invalid +40, hash IOC) are untouched; with no heuristic firing,
   scoring is byte-identical to before.
3. `%VAR%` is expanded across the **whole command** (program + args) via
   the existing `scanners::services::expand_env_vars` before any heuristic
   runs — Run-key and `.lnk` paths together (both flow through
   `score_with_signals`).
4. One new heuristic, LOLBin-gated only: remote argument (`://` URL or UNC
   `\\host\share`) → **+25**, so a LOLBin fetching remote payload scores
   ≥ Suspicious on that signal alone (discounts are withheld whenever any
   heuristic fires, so 25 is the floor).

## Before → after (machine-verified)

Run-key and `.lnk` styles score identically in both eras (same pipeline;
parity asserted in tests). Scores from probe runs + passing unit tests:

| case | sig | before | after |
|---|---|---|---|
| ps `-WindowStyle Hidden -EncodedCommand` | Valid | 0 Safe | **25 Suspicious** |
| ps `-WindowStyle Hidden -EncodedCommand` | Unknown | 5 Safe | **25 Suspicious** |
| cmd `/c %TEMP%\payload.exe` (expanded) | Valid | 0 Safe | **30 Suspicious** |
| mshta `http://…/payload.hta` | Valid | 0 Safe | **25 Suspicious** |
| rundll32 `…Temp\evil.dll,Entry` | Valid | 0 Safe | **30 Suspicious** |
| rundll32 `…Temp\evil.dll,Entry` | Unknown | 10 Safe | **30 Suspicious** |
| benign `rundll32 shell32.dll,Control_RunDLL` | Valid/Unknown | 0 Safe | 0 Safe (unchanged) |
| benign `rundll32 …\System32\legit.dll` | Valid/Unknown | 0 Safe | 0 Safe (unchanged) |
| benign `powershell -File …\Program Files\…\tool.ps1` | Valid/Unknown | 0 Safe | 0 Safe (unchanged) |
| benign `cmd /c …\System32\*.bat` | Valid/Unknown | 0 Safe | 0 Safe (unchanged) |
| benign `mshta C:\Windows\help\local.hta` | Valid/Unknown | 0 Safe | 0 Safe (unchanged) |

## Live false-positive check (this real PC, read-only)

Pre-fix release scan vs post-fix release scan (`--data-dir` temp dirs):
**114 entries before and after, 0 verdict changes** (all Safe both runs).
Coverage honesty: no live entry on this box is a LOLBin-with-heuristic
(no powershell/cmd/mshta/rundll32 findings at all; the single `%windir%`
literal entry stayed Safe both runs), so the new paths fired on **0 live
entries** — the behavior change is covered by unit tests, and the live
check proves **no FP regression** on a real startup surface.

## Baseline-id note (for the CHANGELOG)

Unrelated to scoring but adjacent: F-LNK-1 changed Startup `.lnk`
`command` from the lnk path to `resolved target + args`, and `make_id`
hashes `command` — so old `.lnk` entries appear as **removed + added**
in `baseline.json` after upgrading. Quarantine records are unaffected
(`original_path` is still the lnk file; old ids still undo — regression
test `old_lnk_record_with_stale_command_id_still_undoes`).
