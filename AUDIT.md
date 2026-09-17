# C.U.R.E Project Audit — V2.1 (2026-09-16, post-hardening working tree)

Re-verified against the working tree on this date. Supersedes the Phase-2
audit. This phase added startup-coverage breadth (services, WMI, IFEO,
AppInit, COM, `.lnk`, structured tasks), publisher intelligence, a
provider-shaped threat-intel abstraction, ATT&CK single-sourcing, report
export with redaction, and targeted UI additions — with no architecture
rewrite and no new auto-remediation anywhere.

Labels: **REAL** = implemented and exercised · **FIXTURE** = synthetic /
test-only, labeled as such · **EXPERIMENTAL** = implemented but limited,
labeled in UI/docs · **NOT IMPLEMENTED** = absent, no UI screen for it.

---

## 1. New Windows coverage (all REAL, detection-only)

| Source | Technique | How (read-only) | Live result on dev box |
|---|---|---|---|
| Auto-start services | T1543.003 | EnumServicesStatusEx + QueryServiceConfig (Automatic/Delayed/Boot/System; Manual/Disabled excluded by design) | 94 services, incl. a genuinely-missing-image LocalSystem service flagged Suspicious with start/account/state evidence |
| WMI subscriptions | T1546.003 | COM/WMI `ROOT\subscription` SELECTs (filters, consumers, bindings); forward-only enumerator, 5 s Next timeout, caps everywhere | 3 rows: the stock SCM Event Log filter/consumer/binding, all Safe |
| IFEO debuggers | T1546.012 | `HKLM\…\Image File Execution Options\*\Debugger` values | 0 rows (clean) |
| AppInit DLLs | T1546.010 | `AppInit_DLLs` + `LoadAppInit_DLLs` values | 0 rows (clean) |
| COM hijacks | T1546.015 | `HKCU\Software\Classes\CLSID` `InprocServer32`/`TreatAs` (HKLM skipped: admin-owned, too large — documented) | 2 rows, Safe after GUID-name FP fix |
| `.lnk` targets | (same as host entry) | MS-SHLLINK subset parser (StringData, LinkInfo path, env-block target); PIDLs never interpreted; nothing executed | CLI `lnk→` lines; drawer target/args/workdir + liveness |
| Task details | T1053.005 | quick-xml pull parser (no DTD/entities processing): all Exec actions + args + workdir, author, principal/run-level, triggers, enabled/hidden; 4 MiB + 20k-event caps | multi-action/author/level/triggers verified by fixtures |

Scoring: new sources flow through the same v2 weights (drop-zone,
trusted, sneaky-PS, signatures, hash force) plus service-specific
evidence (missing image +30, start/account/state as scoreless evidence).
Services hash only outside trusted locations (perf bound, documented).
Only `StartupFolder`/`ScheduledTask` (`is_file_backed`) are quarantinable
or auto-cleaned; everything else lands in review with backup-first manual
guidance and is rejected by the Tauri quarantine boundary.

Popup investigation (4–5 transient post-login windows): check Startup
Audit for IFEO debuggers first (per-exe launch hooks fire exactly this
way), then auto-start services with user-writable images, then Run keys /
Startup `.lnk` targets (drawer resolves them), then COM TreatAs and
WMI consumers. See the final report for the exact walkthrough.

## 2. Publisher / signature intelligence (REAL, lazy)

`signature_detail()` returns verdict + signer display name
(`CERT_NAME_SIMPLE_DISPLAY_TYPE`) from the embedded chain, falling back
to the verifying catalog's PKCS#7 signer for catalog-signed system
binaries (verified: notepad.exe → "Microsoft Windows"). Unsigned/
unverifiable files yield `None` — normal, never suspicion. Display
tokens: VALID / INVALID / UNSIGNED / UNKNOWN (UNKNOWN covers every
unverifiable case; there is no separate ERROR state by design).
Publisher is display-only: fetched lazily for drawers/reports/exports,
never in the hot scan loop. UI states "Unavailable (unsigned or
verdict-only check)" rather than inventing data. Signed ≠ trusted:
signed malware exists and the UI does not claim otherwise.

## 3. Threat intel (FIXTURE-backed store, provider-shaped)

`ThreatIntelProvider` trait (`provider_name`, `provider_label`,
`lookup_hash`) with a single implementation, `LocalFixtureProvider`,
labeled `DEMO / TEST DATA — synthetic fixture hashes, not a live threat
feed`. `check_hash` delegates to it (no behavior change). A future
signed feed would implement the trait against a verified local snapshot;
no network lookups exist or are planned — local-first is preserved.
Unit tests lock the DEMO label, fixture hits, and random misses.

## 4. ATT&CK single source (REAL)

`attack::technique_for(&PersistenceSource)` is exhaustive over the enum
(new variants are a compile error until mapped); `ScoredEntry.attack`
carries `{id, name}` from the backend on every finding. The frontend
prefers `entry.attack` and keeps its map as fallback for stale payloads
only; ATT&CK IDs render only when the mapping exists (services T1543.003,
WMI T1546.003, IFEO T1546.012, AppInit T1546.010, COM T1546.015, ransom
T1486 in reports).

## 5. Canary review (EXPERIMENTAL — unchanged status, hardened)

- Acquisition buffer 4 KiB → 64 KiB (legit bursts overflowed it and
  silently lost events; residual loss documented — decoy tamper, not
  bursts, is the primary signal).
- New behavior-locking tests: save-storms on one file stay quiet;
  cross-extension renames stay quiet; build storms DO fire burst
  (documented FP tradeoff, cooldown-bounded); restart starts clean
  (in-memory state is intentionally forgotten).
- Shutdown: stop flag → dir threads exit within the 2 s wait quantum,
  handles closed on every path (no leaks added).
- Resource: 3 dir threads + 1 tripwire poller per guard, 64 KiB buffers,
  5 s poll cadence, bounded deques (4096 events) + 120 s alert cooldowns.
- No full-protection claims anywhere (UI banner, report limitations,
  TESTING §5).

## 6. Watcher reliability (measured, not replaced)

- Poll-cost guard test: 200 poll cycles (26 probes + set-diff each) run
  in ~0.02 s — the 1.5 s loop is I/O-idle; CPU impact negligible.
- Detection latency is the poll interval by design (≤ ~1.5 s + spawn).
- Races assessed: letter reuse across polls is handled by set-diff
  (arrival = in-current-not-in-previous); duplicate inserts within one
  interval still trigger once per letter; trigger re-check happens per
  arrival, so a trigger added after first sighting fires on the NEXT
  arrival only (documented); inaccessible/network drives read as absent
  (`Path::exists`), never errors.
- WM_DEVICECHANGE migration plan (NOT implemented — polling stays):
  to migrate, run the watcher message pump on a dedicated thread with an
  invisible window (`HWND_MESSAGE`), handle `DBT_DEVICEARRIVAL` /
  `DBT_DEVICEREMOVETYPE` volume flags, keep polling as fallback for
  `subst`/network mounts (which never raise device messages). Benefit:
  sub-second latency + zero polling. Cost: a windowed thread in a
  console app, plus keeping both paths tested. Revisit only with VM
  proof that polling misses a real insertion class.

## 7. Action safety review (all remediation explicit/confirmed)

- QUARANTINE: strict fresh-scan ids (CLI + Tauri); non-file-backed
  sources → backup-first manual guidance; registry/services/WMI/IFEO/
  AppInit/COM never relocated; undo preserved and E2E-proven.
- PROCESS TERMINATION: PID→name re-validation + per-action two-step
  confirm (results cards and sentinel rows).
- CLEANUP: explicit scan → arm → confirm; safe categories only; per-item
  failures never abort the batch.
- STARTUP REMEDIATION: explicit quarantine/undo for files; manual +
  backup-first for the rest.
- SERVICE REMEDIATION: NOT implemented — `sc.exe` guidance text only,
  disable-before-delete ordering, backup step first. No stop/disable/
  delete code exists anywhere (verified by grep for `SERVICE_CHANGE_CONFIG`
  / `DeleteService` / `ControlService`: zero hits outside tests).

## 8. Privacy / export

- `cure report [--format json|txt] [--redact]` (CLI) + Overview
  Export TXT/JSON + redact checkbox (GUI, `export_report` command).
- Redaction replaces the profile home dir (case-insensitive) with
  `%USERPROFILE%` and the username with `<user>`; verified zero
  username hits in a redacted export.
- Reports distinguish Checked / Not checked / Unavailable per area and
  never print "clean" for unperformed checks; SHA-256 digests appear for
  HighRisk findings with resolvable binaries only.

## 9. Performance observations (dev box, 114 findings incl. 94 services)

- Release CLI scan: **1.3 s wall** (collect ~136 ms, score ~1.1 s),
  peak working set **12.7 MB**. Debug build: ~4 s (score ~3.9 s).
- Cost is per-binary WinVerifyTrust + streaming SHA-256 (every resolved
  persistence exe; services additionally skip hashing inside trusted
  locations). Enumeration (registry/services/WMI/COM/tasks walk) is
  sub-second combined; the WMI COM query adds ~1 s.
- Bounded by construction: 4 MiB task cap, 20k XML events, 64 WMI
  objects/query, 128 WMI entries, 16-cert publisher cap, chunked
  deletes/hashing, batched GUI scoring with progress.
- No new optimization applied (per policy); the numbers say none is
  needed on a normal machine.

## 10. Verification status

| Area | Result |
|---|---|
| `cargo test --workspace` | 224 core + 7 dirwatch + 36 watch = **267 passed, 0 failed** |
| GUI fixture test | 1 passed (real desktop: overlay closed, notepad spared) |
| Total | **268** |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| GUI clippy `--all-targets -- -D warnings` | clean |
| Release builds | cure.exe, cure-watch.exe, cure-gui.exe |
| GUI E2E run 8 (final V2.3 binaries) | PASS — cleanup, footer ×3, quarantine+undo, 5 views, canary ACTIVE, incident StrongCorrelation on 266 procs/20 windows, self-exit |
| Mock Playwright | verify, cleanupcheck, chipcheck, canarycheck, pixelcheck PASS; V2.1 probe (attack chips, drawer+lazy details, export, 11 coverage rows) PASS, no page errors |
| Live CLI spot checks | services/WMI/COM/IFEO/AppInit enumerate; WMI stock filter Safe; COM FP fixed; redacted export verified |

**Honestly not performed**: clean-VM run; TRIGGERED canary via real FS
event; watcher consent click-through (would persist on the dev box);
DISM execution; USB passthrough; `subst`-only trigger timing.

## 11. Remaining limitations

Services: Manual/Disabled out of scope by design; image env-expansion is
best-effort; delayed flag is best-effort. WMI: COM failures now surface as
CHECK FAILED / ACCESS DENIED via `scan_report` (fixed in V2.2 — a
broken-WMI box no longer reads as "0 entries clean"). COM: HKLM hive not
walked (documented boundary). Tasks: unelevated runs may see 0 system
tasks — counted as skipped (PARTIAL), never silent. Publisher: first
non-self-signed chain cert (approximation, documented). IOC: synthetic.
Processes in CLI reports hash every binary (same policy as GUI scan).

## 12. V2.2 — post-login incident investigation

New `core::incident` module: explicit 15/30/60/120 s observation windows
(rejected otherwise) combining WMI creation events (proven unelevated on
Win10/11 via live probe) with 500 ms ToolHelp snapshot diffs and
EnumWindows title/class polling. No injection, no hooks, no execution, no
screenshots, no keystrokes, no content inspection — metadata only, with
documented ±500 ms lifetime precision, PID-reuse handling, 4096/1024
tracking caps, and event-loss disclosure (sub-500 ms processes are caught
by events with pid-only records when snapshots miss them).

Correlation is deterministic (DIRECT path equality with service-PID
equality for shared images; STRONG command/args + timing; PARTIAL
name-only with both paths shown; WEAK folder/child proximity; NONE) and
covered by synthetic fixtures A–K. Verdict ladder CAUSE IDENTIFIED →
STRONG CORRELATION → REVIEW REQUIRED → NO DIRECT EVIDENCE →
INSUFFICIENT OBSERVATION; "Malware found" appears nowhere. Pre-existing
processes/windows are baseline context (no fabricated birth events).

WMI ambiguity fixed: `SourceStatus` (CHECKED/PARTIAL/UNAVAILABLE/CHECK
FAILED/ACCESS DENIED) with `scan_report()` for tasks (skip counts),
services (SCM errors classified), WMI (COM errors classified), registry
(skipped keys counted); elevation reported via token query; coverage rows
and exports render states instead of bare zeros. No reboot-capture flow
exists by design (it would require installing persistence).

Exports: incident section in JSON/TXT reports; redaction now ON by
default with `--full-paths` opt-in (CLI) and a checked box (GUI).
Privacy re-verified: zero network/socket/telemetry APIs in the tree.
New GUI commands `start_incident_observation` / `export_incident_report`
plus `cure incident`; E2E run 6 drives a live 15 s observation
(StrongCorrelation, 61 timeline events — correctly not CAUSE IDENTIFIED
on negative data). Observation cost: one snapshot (~250 procs, ms) +
one window poll per 500 ms tick — well under 1% CPU.

## 13. V2.3 — validation matrix (VERIFIED unless noted)

Labels: VERIFIED = executed live on hardware · SIMULATED = synthetic
fixtures/mocks · CODE-REVIEWED = inspected, no execution · NOT TESTED.

| # | Area | Result |
|---|---|---|
| A | Unelevated run (standard user, this box) | VERIFIED — full scan/report/incident/15 s observation; 1 task file skipped and REPORTED (`skipped: 1 task files`); WMI events deliver; redaction default-on verified by grep |
| B | Elevated run | CODE-REVIEWED — elevation query + skip paths implemented and unit-tested; live elevated run NOT TESTED (no spare admin context in this session) |
| C | Clean machine | VERIFIED (dev box, 0 high-risk) — posture REVIEW REQUIRED only when findings exist; empty scans render counts + states, never "100% safe" |
| D | Synthetic suspicious fixtures | VERIFIED — CURE-SYNTH suite across scoring/parsing/correlation; HighRisk seed auto-quarantined live in E2E |
| E | Missing/inaccessible paths | VERIFIED — ghost files → UNKNOWN verdicts; vanished cleanup targets → per-item failures; planted undo destinations → PermissionDenied; unreadable task file → counted skip (live: 1) |
| F | Busy box (~260 procs) | VERIFIED — 15/30/60/120 s observations: 27/42/72/132 s wall (linear), 29.8 MB peak, 257 procs/13–18 windows tracked, caps unhit |
| G | No WMI event delivery | SIMULATED — CheckFailed/Unavailable verdict tests + snapshot-only degradation path; live WMI works here so the fallback is tested by simulation only |
| H | Partial scanner access | VERIFIED — PARTIAL states render in CLI summary, GUI coverage, and exports (services/tasks/registry/WMI) |
| I | Empty result sets | VERIFIED — empty timeline = Started/Ended bookends; empty findings = counts, never "clean" claims |
| J | Locked files | VERIFIED — exclusive-lock self-update + cleanup tests; locked cleanup failures captured per-item live in E2E (os error 32) |
| K | PID reuse | VERIFIED (pure) — lifecycles independent; cmdline attach requires live-name match (unit-tested); live marker test |
| L | Very short-lived processes | VERIFIED — unique-name cmd-copy (exact path, multi-poll) + fake-overlay window (title, transient) live; instant `cmd /C exit` opportunistic only; `timeout.exe` dies headless (documented, ping used); copied notepad self-terminates (documented) |
| M | svchost sharing | VERIFIED live — SCM-reported PIDs give DIRECT matches (CDPUserSvc etc.); mismatched instances STRONG-with-caveat; HPSEU↔ChatGPT false class FIXED (truncated-token guard + regression test); E2E correlations 88→48 |
| N | App startup noise | VERIFIED — live runs show ordinary processes correlating NONE; save-storm/rename QUIET tests lock FP behavior |
| O | Watcher end-to-end | VERIFIED (isolated APPDATA + subst + cmd-as-gui): consent, self-install, 1.5 s detection, VALID trigger, spawn PID observed, real Startup untouched, full cleanup |
| P | Self-update failures | VERIFIED — locked target keeps old bytes + no tmp residue; stale tmp overwritten; same-file short-circuit; byte-compare decisions |
| Q | Quarantine boundaries | VERIFIED — fresh-scan IDs only; registry/manual-source rejection; file-backed gate; scoped undo (allow/deny/sibling-prefix/compat); occupied-target refusal; double-quarantine clean |
| R | Cleanup boundaries | VERIFIED — read-only scan proven byte-identical; Downloads opt-in + 30-day boundary; per-item failures; DISM fixed args (unit); dry scan modifies nothing |
| S | Signature/publisher | VERIFIED — MS binary → VALID + "Microsoft Windows" (catalog fallback proven); unsigned → none; missing/dir → UNKNOWN; tampered → Invalid/Unsigned |
| T | Threat-intel boundary | VERIFIED — no sockets/HTTP/telemetry in code; reqwest/hyper exist ONLY in Tauri's own dependency closure (updater unconfigured, never invoked); fixture label locked by tests |
| U | Report privacy | VERIFIED — redaction regression through real wiring (both formats); `--full-paths` opt-in; GUI checkbox matches backend |
| V | Canary real-FS | VERIFIED (tempdir) — modify/rename/delete raise tamper; noise without tamper; cross-extension renames quiet; restart re-alerts; shutdown prompt (see W) |
| W | Canary shutdown bug (FOUND+F FIXED) | VERIFIED — dir handle lacked FILE_FLAG_OVERLAPPED, making reads synchronous: the 2 s wait never fired and `join` hung forever on quiet dirs (latent in GUI/watcher too — nobody ever joined). Fixed + regression test; also added CancelIo-on-timeout |
| X | Static audit | VERIFIED — full-repo sweep: destructive strings only in printed user-run guidance + engine internals; spawns are fixed-arg/discovery-only/test-only; explorer direct-argv; no `cmd`; no DeleteService/ControlService/WMI-write/registry-write APIs |

NOT TESTED: elevated live run, clean-VM run, USB passthrough attach (subst stands in), DISM execution, TRIGGERED-canary-by-real-ransomware (by design — synthetic only), consent modal click (would persist on dev box).

Intentionally unresolved: Tauri's unused updater HTTP client stays linked (documented, never invoked — removing default features risks the GUI build for zero behavior gain); undo pre-existing-record compat kept via unscoped `undo()` (scoped variant used by all callers); command lines in exports carry third-party args by necessity (documented handling note).
