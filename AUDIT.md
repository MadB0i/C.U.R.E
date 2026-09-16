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
| `cargo test --workspace` | 188 core + 4 dirwatch + 33 watch = **225 passed, 0 failed** |
| GUI fixture test | 1 passed (real desktop: overlay closed, notepad spared) |
| Total | **226** |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| GUI clippy `--all-targets -- -D warnings` | clean |
| Release builds | cure.exe, cure-watch.exe, cure-gui.exe |
| GUI E2E run 6 (expanded scan) | PASS — cleanup, footer ×3, quarantine+undo, 5 views, canary ACTIVE, self-exit |
| Mock Playwright | verify, cleanupcheck, chipcheck, canarycheck, pixelcheck PASS; V2.1 probe (attack chips, drawer+lazy details, export, 11 coverage rows) PASS, no page errors |
| Live CLI spot checks | services/WMI/COM/IFEO/AppInit enumerate; WMI stock filter Safe; COM FP fixed; redacted export verified |

**Honestly not performed**: clean-VM run; TRIGGERED canary via real FS
event; watcher consent click-through (would persist on the dev box);
DISM execution; USB passthrough; `subst`-only trigger timing.

## 11. Remaining limitations

Services: Manual/Disabled out of scope by design; image env-expansion is
best-effort; delayed flag is best-effort. WMI: COM failures degrade to
empty (a broken-WMI box reports "0 entries", indistinguishable from
clean — noted in coverage detail as "0 entries", never "clean"). COM:
HKLM hive not walked (documented boundary). Tasks: unelevated runs may
see 0 system tasks (elevation note in TESTING). Publisher: first
non-self-signed chain cert (approximation, documented). IOC: synthetic.
Processes in CLI reports hash every binary (same policy as GUI scan).
