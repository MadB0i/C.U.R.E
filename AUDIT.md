# C.U.R.E — Read-Only Audit (2026-09-24, working tree @ `c0d9f63`)

Method: code reading + `git` inspection + `cargo test` / `cargo clippy` / `cargo fmt --check` /
`cargo deny` + CLI in scan-only modes (`scan`, `cleanup scan` with `--data-dir` pointed at a
temp dir). Nothing destructive was executed: no quarantine/undo, no cleanup run, no DISM, no
process kill, no overlay dismissal, no watcher install, no GUI test (the GUI overlay test drives
real desktop windows). Items not executed are listed under "Needs VM validation", not claimed.

## Summary (5 lines)

- Quarantine core is sound (move-not-delete, scoped undo, fresh-scan IDs) but the USB watcher
  will execute an attacker-supplied `cure-gui.exe` off any USB carrying a 14-byte trigger file.
- CLI `quarantine` needs no confirmation, Authenticode skips revocation checks, and WMI failures
  are swallowed as "0 entries" — three detection-trust bugs that each take one line to fix.
- Hygiene is good: no committed secrets/binaries, 267/267 root tests pass, clippy/fmt clean,
  both release builds pass; README's "269 tests" is wrong and the GUI loads Google Fonts remotely.
- Coverage gaps are honest-but-real: no common-Startup folder, no Winlogon/RunServices/BHO/LSA
  scanners, hash IOC is 3 hardcoded fixtures that override even valid signatures.
- Verdict: **Ready after P0–P1 fixes** (F-01–F-04 must be fixed before any public demo; F-05–F-15
  before calling it a security tool on a resume).

## Findings

| ID | Sev | Area | File:line | Evidence | Impact | Suggested fix | Effort |
|----|-----|------|-----------|----------|--------|---------------|--------|
| F-01 | P0 | watch trigger | `watch/src/main.rs:237` | `let candidates = [Some(drive_root.join(GUI_EXE_NAME)), beside_watcher];` then `Command::new(&candidate)… .spawn()` at :240-243. Trigger is `content.trim_end()==TRIGGER_CONTENT` (`watch/src/trigger.rs:12-17`), and the file itself warns "NOT cryptographically secure" (:7-11). | Any USB with `.cure-trigger` (14 bytes, documented format) + a `cure-gui.exe` gets arbitrary code execution as the user. Spoof cost ~zero. | Prefer `beside_watcher`; if the USB copy is used, require Authenticode `ValidSigned` + pinned-signer/hash check first, and prompt before launch. | M |
| F-02 | P0 | cli safety | `cli/src/main.rs:511-530` | `Quarantine` arms go straight to `quarantine_entry()` with no `confirm()` call; the only `confirm()` in the file is the cleanup one (:1049-1056, used at :1088 and :1120). GUI has per-item confirmation; CLI has none — a pasted wrong id moves the file. | One typo / misclicked id = file relocated out of Startup/Tasks with no second chance (undo exists, but user may not notice). | Add `[y/N]` confirm printing id, name, and full original path before `quarantine_entry`. | S |
| F-03 | P0 | authenticode | `core/src/signature.rs:475` | `fdwRevocationChecks: WTD_REVOKE_NONE` in `run_winverifytrust` (:467-493). Only `S_OK`/catalog `hr==0` yield Valid (:495-522 equivalent mapping), so no bad-as-trusted path — but a revoked-but-chained cert still returns `ValidSigned` and earns `-40`. | Revoked signer (e.g. leaked cert) validates as trusted; scoring then suppresses all other signals on that binary. | Use `WTD_REVOKE_WHOLECHAIN` (accept the offline-fail-closed tradeoff explicitly) or document why revocation is off. | S |
| F-04 | P0 | quarantine durability | `core/src/quarantine.rs:89-100` | `move_file` (:61-73) runs at :89, record is built + `save_records` at :90-100. If `save_records` fails, the file is moved with no record = unfindable by `undo`. Separately, `git grep "set_permissions\|set_acl" -- '*.rs'` returns nothing: ACLs/owner are not preserved or restored. | Orphaned quarantine (data-loss-shaped bug in a safety tool); restored files may come back with wrong ACLs on locked-down folders. | Write-then-move (save record with `pending` state first, or move to final name only after `save_records` succeeds); store + restore ACLs (or document "ACLs not preserved" in UI + README). | M |
| F-05 | P1 | detection | `core/src/scanners/wmi.rs:101-115` | `let result = scan_inner(&mut entries); … let _ = result; Ok(entries)` — every COM/WMI failure (denied, broken WMI) returns `Ok(vec![])`. Header promises surfacing (:33-35 per prior audit) but the error is discarded here. | A machine with blocked/broken WMI renders "0 WMI entries" = looks clean. Silent false-negative on exactly the machines that need scrutiny. | Propagate `Err` / return `(entries, SourceStatus)` like services/tasks do; render CHECK FAILED instead of zero rows. | S |
| F-06 | P1 | ffi | `core/src/scanners/services.rs:268-271` | `std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, …)` over a `Vec<u8>` with no alignment guarantee (the later cast at :319-320 carries `allow(cast_ptr_alignment)`; this one does not). | Misaligned struct read = UB; in practice garbage service records or a crash during enumeration. | Use `Vec<ENUM_SERVICE_STATUS_PROCESSW>` / `alloc_zeroed` with correct layout, or `ptr::copy` into aligned storage. | S |
| F-07 | P1 | privacy/gui | `gui/dist/index.html:7-9`, `gui/src-tauri/tauri.conf.json:19-21` | `<link rel="preconnect" href="https://fonts.googleapis.com">` + stylesheet link; `"security": {"csp": null}`. No app-level `fetch` exists (verified by grep: only hit is a `nomoreransom.org` data string), but the webview phones Google on every launch. | IP + User-Agent to Google on each start contradicts README's "no telemetry / local-first" posture; `csp:null` leaves nothing to constrain future frontend regressions. | Self-host Inter (or system fonts) and drop the preconnect; set an explicit CSP (`default-src 'self'`, no remote). | S |
| F-08 | P1 | scoring | `core/src/risk.rs:187-193` | `if let Some(_description) = hash_match { … scored.risk = HighRisk; }` — "beats every heuristic, even a valid signature" (comment :188). IOC list is 3 hardcoded demo hashes (`core/src/hash_intel.rs` + `known_bad_hashes.json`, labeled DEMO). | A fixture/synthetic hash on a legit signed binary forces HIGH with chip text "Known Malware Hash"; any future feed poisoning becomes unreviewable REMEDIATE-grade signal. | Cap hash override at Suspicious unless unsigned/invalid, or require a second corroborating signal; show feed provenance + description in UI. | S |
| F-09 | P1 | scoring | `core/src/risk.rs:115-116` | `command_norm.contains(t)` over `DROP_ZONE_TOKENS`/`TRUSTED_TOKENS` after `normalize` (:198-200, lower + `\`→`/`). No path-boundary checks. | `C:\Temp\system32\evil.exe` earns `-20 trusted`; `…\Program Files-fake\…` earns `-20`; trivial spoofing of ±20-30 points each way. | Match on path components (split `/`, compare segments) instead of substring. | S |
| F-10 | P1 | robustness | `cli/src/main.rs:356-358`, `core/src/entry_details.rs:81` | `s.entry.name[s.entry.name.len()-4..].eq_ignore_ascii_case(".lnk")` (length-guarded but byte-indexed). A name whose byte at `len-4` splits a multi-byte char (emoji/CJK, attacker-controllable via Startup filenames) panics the scan display path. | Panic during `scan` output = self-DoS on exactly the hostile input a scanner must survive. | Use `strip_suffix` / `ends_with` (case-insensitive via `to_ascii_lowercase().ends_with(".lnk")`). | S |
| F-11 | P1 | cli safety | `cli/src/main.rs:1151-1166` | `if dism { … run_dism_cleanup() }` runs immediately after the delete confirm; no second prompt names DISM. `dism_args` itself is fixed (:439-446 `cleanup.rs`, no interpolation — injection-safe, VERIFIED). | `--dism` buried in a flag list triggers a minutes-long elevated system mutation the user already said yes to "deletes", not to DISM. | Second explicit `[y/N]` ("Run DISM component cleanup? elevated, minutes") or require `--dism --yes-i-mean-dism`. | S |
| F-12 | P1 | cleanup | `core/src/cleanup.rs:386-392` vs `:105,:120` | Scan enumerates with `.follow_links(false)` (:105,:120) so junctions never appear as candidates — good. But `delete_path` does `if path.is_dir() { remove_dir_all }`, and `is_dir()` follows symlinks/junctions. | A dir-junction smuggled into a scanned tree root (or a TOCTOU swap between scan and run) turns `remove_dir_all` through the link. Reachability is narrow (requires attacker foothold in temp/cache + user confirm), hence P1 not P0. | In `delete_path`, use `symlink_metadata` + refuse/`remove_file`-only for reparse points; re-verify `!is_symlink` immediately before delete. | S |
| F-10b | P1 | coverage | (no lines — verified by absence) | `git grep "ProgramData\|Common Startup\|common_startup" -- core/` returns nothing. `startup.rs` walks per-user `%APPDATA%…\Startup` only, top level, non-recursive. | Machine-wide persistence in `C:\ProgramData\…\Startup` is invisible — a standard, non-exotic attacker location. | Add common-Startup enumeration (same file-only, no-execute treatment). | S |
| F-13 | P1 | docs | `README.md:65-66` | Claims "269 Rust tests passing". Measured this session: `cargo test --workspace` (root) = 224 core + 7 dirwatch + 36 watch + 0 cli = **267 passed, 0 failed**. GUI workspace has 2 more tests (`gui/src-tauri/src/main.rs:1952,2023`) that were deliberately NOT run (they drive real desktop windows). | Off-by-two in the flagship number; a reviewer running the documented command sees 267, not 269. | Say "267 (+2 GUI tests, separate workspace)" or move GUI tests into CI so one command reproduces the claim. | S |
| F-14 | P1 | ci | `.github/workflows/ci.yml:24-26` | CI runs `cargo test --workspace` + `cargo clippy --workspace -- -D warnings` (no `--all-targets`, no `fmt --check`, no GUI workspace tests/build-verify beyond release build). | `clippy --all-targets` failures and `fmt` drift (the tree's own docs prescribe `--all-targets -- -D warnings`) pass CI silently; GUI tests never run in CI. | Add `--all-targets`, `cargo fmt --check`, and a GUI-workspace test job (headless-safe subset). | S |
| F-15 | P2 | coverage | scanners (absence verified) | No `Winlogon`/`RunOnceEx`/`RunServices`/`Policies\Explorer\Run`/`BootExecute`/`Active Setup`/BHO/LSA-provider scanners (`grep` for those names in `core/src` hits only `services.rs:36` `ImagePath`). Run + RunOnce (HKCU/HKLM) ARE covered (`registry.rs:8-11`) — earlier "RunOnce missing" assumption is wrong. | Standard autorun persistence outside Run/RunOnce/Tasks/Startup/Services/WMI/IFEO/AppInit/COM goes unmentioned; report can read cleaner than the box is. | Add scanners in documented priority order, or render an explicit "Not checked" list per machine (report.rs already supports the states). | M/L |
| F-16 | P2 | deps | `gui/src-tauri/Cargo.lock:1321,2504` vs `Cargo.lock` | `hyper` + `reqwest` present transitively via `tauri` in the GUI lockfile (never invoked — updater unconfigured, no app imports: VERIFIED by grep); root `Cargo.lock` has neither. `cargo deny check advisories` = ok (VERIFIED); full `cargo deny check` fails spuriously — no deny config exists so default license allow-list rejects even MIT/Apache-2.0. `cargo audit` and `cargo outdated` are not installed (UNVERIFIED areas). | Reviewer `cargo deny check` looks red for a non-reason; unused HTTP client widens GUI supply-chain surface. | Commit a minimal `deny.toml` (allow MIT/Apache-2.0, advisories on); optionally strip Tauri default features pulling the updater client. | S |
| F-17 | P2 | robustness | `core/src/baseline.rs:24-29` | `create_dir_all` then `fs::write` — non-atomic overwrite of `baseline.json`, no backup, no fsync. | Crash/power-loss mid-`scan` corrupts baseline; `diff` then errors instead of diffing. | Write temp + rename (atomic on NTFS same-dir). | S |
| F-18 | P2 | dead code | `core/src/threat_intel.rs:56-71,160-165` | File-backed `ThreatIntel::{from_file,to_json,merge}` + `init_from_file` have no production callers (only the trait via `hash_intel.rs` is wired: `cli/src/main.rs:700`, `report.rs:14`, `gui …/main.rs:793`). The live IOC path is the compiled-in fixture map. | Two parallel threat-intel abstractions; README-adjacent "update path" expectations point at code nothing calls. | Either wire file-merge into startup or delete `threat_intel.rs` and document "rebuild to update IOCs". | S |
| F-19 | P2 | compat | `core/src/quarantine.rs:108-110` | Unscoped `undo()` retained for tests/power users while all callers use `undo_scoped`. A planted `records.json` + unscoped undo = arbitrary file plant on elevated runs (the scoped variant's doc :112-120 names exactly this). | Kept-compat escape hatch around the scoped-undo security boundary. | Gate bare `undo()` behind a CLI flag (`--allow-unscoped-undo`) or remove it. | S |
| F-20 | P2 | detection quality | `core/src/ransom_detect.rs:29-32,140-159`, `core/src/canary.rs:271` | Ransom-note keyword match is substring (`RECOVER/IMPORTANT/INSTRUCTION/RELEVANT`) — benign names match; canary burst grouping uses case-sensitive `folder ==` on Windows paths. | FP-prone note "family" labels; `C:\Docs` vs `c:\docs` splits one burst into two quiet halves. | Word-boundary/regex note match; reuse `normalize` for folder keys. | S |

Dropped from P-list (checked, fine): CLI `scan`/`diff`/`report`/`incident` are read-only apart from
writing their own output files (VERIFIED: `scan` writes only `baseline.json` to `--data-dir`);
overlay review requires the Start Rescue click and closes only per-window-confirmed
candidates (graceful `WM_CLOSE` default; termination only via explicit per-window
Force close); `kill_high_risk_processes` re-validates PID→name
(`:1023-1035`); Tauri allowlist is minimal (`capabilities/default.json` = `core:default` only);
frontend has no `eval`/`new Function` and all 32 `innerHTML` sites are clears/static/escaped
(`escHtml` at `gui/dist/app.js:53-55`); services `Manual`/`Disabled` exclusion and HKLM-COM exclusion
are documented design boundaries, not bugs.

## Needs VM validation (mutates the system — not run here)

- `cure quarantine <id>` + `cure undo <id>` round-trip (path, ACLs, occupied-target, double-quarantine).
- `cure cleanup run` (incl. `--include-downloads`, locked-file per-item failures) and `cleanup scan` byte-identity.
- DISM execution (`--dism`) — elevated, minutes-long.
- GUI `dismiss_overlays` close→`TerminateProcess` escalation path and `kill_high_risk_processes` on live PIDs.
- Watcher consent box, self-install to Startup, trigger launch, `.cure-trigger` spoof rejection behavior.
- `subst`-mounted and real-USB trigger timing; network-drive letter reuse races.
- Elevated run (token query + skipped-key accounting) and clean-VM baseline run.
- GUI workspace tests (`overlay_fixture_dismisses_fake_overlay_and_spares_notepad` drives real windows).
- Real-ransomware-triggered canary (synthetic-only by design — keep it that way).

## Uncommitted / unpushed work

- Unpushed commits: none — `main` == `origin/main` (`c0d9f63`), no stash, no untracked files.
- Uncommitted (working tree, all UI-only, 1007+/82- across 4 files):
  - `M gui/dist/app.js` — quarantine-receipt + premium reference UI logic (+348 lines).
  - `M gui/dist/index.dev.html`, `M gui/dist/index.html` — premium scan-map / cleanup-animation markup (+80 each).
  - `M gui/dist/style.css` — full premium theme pass (+581 lines).
  - No Rust changes uncommitted; `target/` artifacts and `*.exe`/`*.log` are git-ignored and untracked-clean.
  - Note: `gui/dist/*` (built frontend output) is tracked in git — every UI tweak dirties the tree. Consider ignoring `dist/` and building it in CI/release.

## Top 10 fixes (recommended order)

1. F-01: stop executing USB-supplied `cure-gui.exe` (prefer beside-watcher + signature/pin check). [M]
2. F-03: enable revocation checking (`WTD_REVOKE_WHOLECHAIN`). [S]
3. F-02: confirmation prompt on CLI `quarantine` (and print full paths). [S]
4. F-04: close the move-before-record orphan gap; document or preserve ACLs. [M]
5. F-05: surface WMI/COM failures as CHECK FAILED instead of empty-ok. [S]
6. F-07: remove Google Fonts remote load; set an explicit CSP. [S]
7. F-13/F-14: fix the test-count claim; harden CI (`--all-targets`, `fmt --check`, GUI tests). [S]
8. F-06: fix the services enumeration alignment cast. [S]
9. F-08/F-09: bound the hash override; component-wise path matching. [S+S]
10. F-10/F-12/F-10b: `ends_with` for `.lnk`, symlink-safe delete, common-Startup coverage. [S+S+S]

## Genuinely GOOD (keep and show)

- Quarantine is move-not-delete with copy-length verification, occupied-target refusal, record-preserving failure paths, and a scoped-undo guard against planted `records.json` — the core safety story is real, not aspirational.
- `is_file_backed` gate: registry/services/WMI/IFEO/AppInit/COM findings can never be relocated, only given backup-first manual guidance — the blast radius of a wrong click is bounded by construction.
- Fresh-scan ID resolution on every mutating path (CLI + Tauri ignore frontend-supplied paths entirely); cleanup re-scans and allowlist-filters before deleting.
- Root dependency closure has zero network crates (no reqwest/hyper/ureq in `Cargo.lock`); finance-grade `WinVerifyTrust` CLOSE discipline and WMI caps/timeouts show the FFI was written carefully.
- Fail-closed watcher consent (garbage marker re-asks), per-item cleanup failure collection, redaction-on-by-default reports, and a TESTING.md honesty rule — the project tells the truth about its limits, which is rarer than features.
- 267/267 tests green, clippy `--all-targets -D warnings` clean, `fmt --check` clean, both release workspaces build — the tree is in a shippable state; the fixes above are bounded, not architectural.

## Verification log (this session)

- `git status --porcelain=v1 --branch` → `## main...origin/main`, 4 modified `gui/dist/*` files; `git stash list` empty; `git ls-files --others --exclude-standard` empty.
- `git ls-files | grep -E '\.(exe)$|target/|node_modules'` → no hits (no committed binaries/artifacts); tracked PNG/GIF are deliberate docs/dev-screenshots reference shots; secret grep → only benign `token` substrings (CSS/design tokens, API names).
- `cargo test --workspace` → 224 + 7 + 36 + 0 = **267 passed, 0 failed** (~17 s, dirwatch live-FS tests included).
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; `cargo fmt --check` → clean.
- `cargo build --release -p cure_cli -p cure_watch` → ok (2.6 s, cached); `cargo build --release` in `gui/src-tauri` → ok (1 m 37 s).
- `cargo deny check advisories` → ok; full `cargo deny check` → license noise (no `deny.toml`); `cargo audit` / `cargo outdated` → not installed (UNVERIFIED).
- `cure --data-dir <temp> scan` → real findings render with evidence + ATT&CK IDs (read-only, VERIFIED); `cure … cleanup scan` → `20405 items | 7.4 GB` breakdown, "scan only, nothing is deleted" (VERIFIED).
- GUI tests (2) NOT run — they manipulate real desktop windows (see Needs VM validation).

## Fix status (audit-p0-p1 pass, branch `audit-p0-p1`, base `cdc289b`)

All P0 + all P1 fixed, one commit per finding ID. Nothing pushed, nothing
merged to main. P2s untouched.

| ID | Status | Commit | How verified (this session) |
|----|--------|--------|------------------------------|
| F-01 | Fixed | `317d776` | 20 new `pairing::tests` (wrong/empty/oversized/malformed token, legacy V1, missing host exe, hash mismatch, reparse host, USB-exe-never-launches) + `cargo test -p cure_watch` 51/51. `launch_gui` rewrite read back: only `host_gui_exe()` spawned, absolute path, no shell. Deviations disclosed: symmetric CSPRNG token (per design, not Ed25519); ACL = inherited `%LOCALAPPDATA%` profile DACL, no hand-rolled SDDL (documented in `pairing.rs`); `Command::new(abs)` = CreateProcess, no shell. |
| F-02 | Fixed | `7e3a7d2` | 5 CLI unit tests (all four plans + strict parser) + live temp-dir run: piped stdin → refusal error + non-zero exit + file untouched; `--dry-run` → preview + file untouched. Interactive Ask→Proceed branch UNVERIFIED live (no TTY in harness; same `confirm()` as cleanup + parser unit-tested). |
| F-03 | Fixed | `27b0b0b` | 10 new pure mapping/flag/mode tests incl. CERT_E_REVOKED, CRYPT_E_REVOKED, all three offline codes, expired/bad-digest/root controls; live `notepad.exe` + publisher tests pass under cache-only default; live `cure scan` still shows `-40 Valid Signature` on this (warm-cache) box. Real-revoked-cert validation → VM list. Design note: cache-only bit set via `dwProvFlags` (where WinTrust requires it), same zero-network effect. |
| F-04 | Fixed | `9b6943e` | 8 new quarantine tests (save-failure injection, locked-move rollback, both crash simulations, orphans, collision, tamper-refusal, legacy compat) + 3 acl tests (incl. SDDL roundtrip) + live temp-dir quarantine→undo roundtrip: record shows Committed/size/sha/SDDL/attrs/timestamps, undo restores bytes with empty notes. ACL control-flag normalization (`AI`) documented + canonicalized in test. |
| F-05 | Fixed | `e7e3a96` | 3 new `combine` tests (Available/AccessDenied/CheckFailed, partials always kept) + live WMI test asserts `Ok` + Available on this box. CLI/GUI/report rendering needed no change (already wired to `scan_report().status`). |
| F-06 | Fixed | `405b52e` | New live `scan_inner` test decodes all real SCM records on this box; `read_unaligned` copy (enum) + u64-backed aligned buffer (config) replace both casts; invariants documented at each site. |
| F-07 | Fixed | `9cd3252` | `gui/dist` grep: only remaining `http(s)` is the SVG xmlns constant; CI `frontend-net` job gates this (ran green locally). CSP set; no inline scripts exist so `script-src 'self'` holds (`style-src` keeps `'unsafe-inline'` for `style="--d:…"` attributes — documented). Chip rendering re-verified `textContent`-only with fixed tone allowlist. Deviation: system font stack instead of bundling Inter (Windows-only tool, zero license burden, identical offline guarantee). |
| F-08 | Fixed | `6c06876` | 8 precedence tests (hash>Valid with DEMO provenance, hash>Invalid, Invalid-alone High, Valid-alone discount, Unsigned-alone silent, service pairs, reason cap). Design note: hash stays precedence #1 per the fix-pass order (exact match beats signature); the defect fixed is missing provenance/description; publisher identity explicitly never affects precedence. |
| F-09 | Fixed | `0d855d9` | Component-matcher unit tests (whole-segment, consecutive-run, degenerate) + spoofed-dir + `service_needs_hash` + process-scan delegation tests; all 261 pre-existing core tests still pass (no reliance on substring quirks). Residual `Temp\system32` case documented in code (needs canonicalization, out of scope). |
| F-10 | Fixed | `7151739` | Shared `is_shortcut_name` + non-ASCII regression test (emoji/CJK, no panic, correct hits); CLI now calls it. |
| F-11 | Fixed | `16dff82` | Second `confirm()` naming DISM+elevation+duration before `run_dism_cleanup()`; gate is the strict `parse_confirmation`-tested prompt. Interactive run UNVERIFIED (would delete real temp files here) → VM list. |
| F-12 | Fixed | `1cfb637` | Pure `classify_delete_target` + real file-symlink and dir-junction refusal tests (targets intact; no skips on this box) + pre-existing normal-tree deletion test as regression guard. Residual check-then-delete TOCTOU documented in code. |
| F-10b | Fixed | `4a3e703` | Common-root shape test + live `scan_common()` test; live `cure scan` prints `common startup: C:\ProgramData\…\Startup`; undo scopes (CLI+GUI) extended so common files stay restorable; GUI crate compiles. |
| F-13/F-14 | Fixed | `9698a95` | Hardcoded "269" removed → CI badge; threat-model section added; overlay GUI test `#[ignore]`d with reason + `testing/run-gui-desktop-tests.bat`; CI now has `--all-targets`, `fmt --check`, GUI `cargo test`, and the URL gate. Deviation: the headless-safe `data_dir` GUI test stays enabled in CI ( ignoring it would cut coverage for no reason). GUI workspace: 1 passed, 1 ignored. |

Final measured state on `audit-p0-p1` @ `9698a95`: root `cargo test
--workspace` = **5 (cli) + 267 (core) + 7 (dirwatch) + 51 (watch) = 330
passed, 0 failed**; GUI `cargo test` = **1 passed, 1 ignored**;
`cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo fmt --check` clean. (Counts moved from the audit's 267 because
each fix added regression tests — the README no longer hardcodes a
number for exactly this reason.)

## Needs VM validation (fix pass — nothing below was run on this host)

- Watcher token flow end-to-end: consent → self-install → host pin →
  media stamp → arrival launch; spoofed-USB ignore; `pair` command.
- Watcher self-install to the Startup folder (writes `%APPDATA%`, Startup).
- `cure-watch --uninstall` live (incl. locked-file leftovers, decoy sweep,
  post-uninstall no-launch on USB insert).
- Real revoked-certificate validation (CERT_E_REVOKED path live).
- Quarantine/undo of a real file with ACLs (elevated + unelevated),
  occupied-target, double-quarantine, orphan surfacing in CLI output.
- `cure cleanup run` (incl. locked files, Downloads opt-in) and the new
  reparse-point refusals against real junctions.
- DISM execution (elevated, minutes-long) incl. the new second prompt.
- GUI overlay review: fullscreen-fixture card, graceful Close, explicit
  Force close, allowlist loop, signed-owner negative control;
  `kill_high_risk_processes`.
- GUI ignored overlay test via `testing/run-gui-desktop-tests.bat`.
- Elevated runs (skipped-key accounting, WMI/registry CHECK FAILED rows).
- Clean-VM baseline run; USB-passthrough trigger timing; cold-cache
  UNVERIFIED rendering in GUI chips.
- Real-ransomware-triggered canary (synthetic-only by design — keep it).
