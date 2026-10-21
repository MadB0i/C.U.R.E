# Changelog — C.U.R.E

## 0.2.0 — Storage Workstation, a real companion, and a real release

This release finishes the Storage Workstation, replaces the companion with
a properly drawn and properly rigged character, and makes the release
processible: an installer, a blocking candidate audit, and a native
end-to-end test that drives the real application.

### Fixed — the headline could over-promise reclaimable space

The reclaimable figure added the duplicate bytes to the category bytes,
although the duplicate copies were *already inside* the categories. On a
real machine this promised 5.48 GB when the maximum was 4.23 GB — a 22.8 %
over-promise on the product's headline number.

The figure is now the **union** of the removable files: each path
contributes its bytes once, and the duplicate figure is reported as a
subset of it rather than an addition to it. Overlapping roots and a file
measured twice no longer inflate anything.

### Fixed — the receipt labelled an estimate "verified reclaimed"

The receipt reported the pre-delete allocated size as verified reclaimed,
and the volume cross-check that would have validated it was broken: the
free-space API was given a file path instead of a volume root, so it
failed every time and the volume row always read "not readable".

The receipt now separates four things: the files planned, the files
removed, the size those files occupied before they were removed, and the
volume's free-space change, which is labelled as an observation of the
whole drive rather than a per-file guarantee. Bytes whose recovery cannot
be promised — a hard-link count that could not be read — are counted
separately.

### Fixed — the Windows Update cache was bulk-recommendable

The classification rule said the Windows Update download cache needs
individual approval, but an earlier rule fired first and marked it safe to
review — so it could have been swept by a bulk action while the update
service was running. The rule order is fixed, the cache is now *review
carefully*, and the supported alternative is named in the explanation.

### Fixed — an unguarded permanent-delete endpoint

The legacy `scan_cleanup` and `run_cleanup` commands were still registered
with the Tauri handler. `run_cleanup` deleted files with no preview, no
confirmation and no receipt, and it sat next to the carefully gated
Storage Workstation where a single line of injected script could reach it.
Both are gone. The backend now enforces the safety, not the interface.

### Fixed — execution now runs the plan that was approved

Execution used to rebuild a plan from whatever path list the caller sent,
and compare only the scan id. The backend now holds the approved plan,
recomputes its digest, refuses a plan whose contents no longer match, and
removes each file through a handle it has verified — so a file that was
swapped between approval and execution is refused rather than deleted, and
a parent directory turned into a junction cannot redirect the removal.

### Fixed — the removal dialog let the keyboard escape

The "type REMOVE" dialog had no focus trap and made the page behind it
reachable. Both storage dialogs now trap the keyboard, make the background
inert, close on Escape and give the focus back to whatever opened them.
The storage scan's completion and failure are announced, and the progress
region no longer floods a screen reader with ten updates a second.

### Fixed — thirty-two CSS declarations were silently dropped

The Storage Workstation used four typography variables that were never
defined, so every `font-size` that referenced them was discarded and the
text fell back to its parent's size. The variables exist now, and the type
scale is the one the screen was written against.

### Added — discovery that reaches the whole machine

* **Large folders** are measured and ranked, not just large files.
* **Old downloads and old installers** are labelled by age and by
  extension — a label, never a verdict. Age alone still proves nothing.
* **Developer build output is recognised wherever it lives.** A project on
  `D:\Projects` counts now, not only one under the user's profile.
* **Firefox caches** were catalogued but could never be cleared, because
  the word "profiles" appeared in both the vendor list and the exclusion
  list.
* The catalogue's dead rules are either used or gone.

### Added — LUMA, drawn properly

The companion was a charcoal cat with two features that did not work: a
rim light that traced a zero-area path and rendered nothing, and a
rectangular eyelid that produced a black visor across the face when the
eye closed. It is now a **ginger tabby** with amber eyes, a violet tech
collar, cream belly, white paws and tabby markings, with a rim light that
paints and an eyelid that is shaped like an eyelid. The sleeping state is
a genuinely curled body rather than a sitting cat with closed eyes.

The companion also stops answering to global events on a screen that has
asked for its own persona, is pet by clicking the cat rather than a square,
can be operated from the keyboard, pauses when the window is not focused,
and can be destroyed without leaking a rig, a timer or a listener.

### Added — a release that can actually be installed

* A **Windows installer** (NSIS, current user, no administrator rights).
  It does not disable antivirus, does not bypass Windows security
  warnings and does not install the USB watcher.
* **Original application icons**, generated from source by
  `tools/make-icons.py` at every size Windows asks for.
* The **candidate audit is a blocking gate**, and it derives the expected
  file set from the repository instead of hard-coding it — which is why it
  used to reject every new candidate.
* The release workflow now runs the browser quality suite and publishes
  the installer.

### Added — native end-to-end verification

`testing/native/native-e2e.py` drives the **real release binary** through
the **real WebView2 webview** and the **real Rust backend** over the
DevTools protocol. It covers startup, navigation, a real storage scan with
real progress, coverage, cancellation, the preview, the typed
confirmation, the removal, the receipt, the companion's states, reduced
motion and the keyboard route. The only file it ever removes is one it
created itself, inside a directory it created itself, inside the system
temporary directory. It runs in CI.

### Changed

* The storage view is a **hairline zone** like the rest of the console
  rather than nine bordered cards.
* The interface no longer shows text below 12 px.
* A cleanup queue survives a trip to another screen instead of being
  discarded.
* The Coverage tab has an untouched state that says what will happen and
  offers the one action that starts it.
* "Show data folder" opens the folder, instead of only changing the
  companion's caption.

## 2026-10-19 — STORAGE WORKSTATION + LUMA

The Disk Cleanup screen is replaced by a **Storage Workstation** built on a
new read-only engine, and LUMA becomes an animated cat. Existing security
behaviour is unchanged; three pre-existing test failures were fixed and three
real engine defects were found and fixed by running against live data.

### Added — storage intelligence engine (`core/src/storage/`)

A measurement engine that reports evidence and never acts on its own. It is
exercised read-only from the CLI (`cure storage scan|duplicates|bench`) and
through six new Tauri commands.

- **Four safety tiers** — `safe_to_review`, `review_carefully`, `protected`,
  `unknown` — decided by provenance *plus* path, with every decision
  returning the clauses that produced it. Only `safe_to_review` may appear in
  a bulk action; `review_carefully` requires per-item confirmation;
  `protected` and `unknown` are refused outright.
- **Never auto-delete anything Windows needs.** `C:\Windows` is protected
  except for named cleanable subtrees; `WinSxS`, `DriverStore`, `Installer`,
  `System32\config`, `System Volume Information`, recovery, `Program Files`,
  `ProgramData` (outside named caches) and the Recycle Bin's `$I` index
  records are all refused.
- **Verified duplicates only.** Size bucket → 64 KiB partial hash → full
  SHA-256 of every member, with a size/mtime re-check so a file that changes
  mid-hash leaves the group. Hard-linked names are excluded, because removing
  a link frees no bytes. One copy is marked as kept; protected copies get no
  action at all.
- **Allocated size, not apparent size**, drives every reclaim figure, with
  OS-reported volume free space shown alongside.
- **Abandoned application data** is reported and always tiered `unknown`,
  carrying an explicit caveat. It is never a deletion recommendation.
- **A walker that does not follow links** (symlink *and* reparse point),
  reports unreadable locations instead of failing, honours cancellation from
  another thread mid-walk, and has explicit depth, entry-count and
  wall-clock limits that it reports rather than silently truncating.
- **Root overlap reconciliation** so a location added by the user inside an
  already-scanned root is measured once rather than twice.
- **A preview → confirm → receipt flow.** Every target is re-validated
  against the live filesystem immediately before removal: still present,
  still a regular file, still not a link, still under an authorised root,
  still permitted by its tier, and unchanged in size. Anything that fails a
  check is refused and listed. The receipt reports verified bytes, the
  pre-delete estimate, and the volume's free-space delta.

### Added — Storage Workstation UI

Five tabs over one scan: **Map** (squarified treemap plus a tier-filtered
category rail), **Largest files**, **Duplicates**, **Cleanup queue**, and
**Coverage**. Plus a per-file inspector, live progress with cancel, a preview
dialog that requires typing `REMOVE`, and a receipt. Before a scan the
headline is an em dash and the map is an empty well — nothing is drawn or
summed that the engine did not measure.

### Replaced — LUMA

The status node is replaced by an original rigged SVG cat: charcoal-violet
fur, amber eyes with two highlights each, soft ears, a four-joint tail, two
front paws, whiskers, and a lid rig that blinks. Ten states — `idle`, `scan`,
`discovering`, `review`, `cleanup`, `quarantine`, `success`, `failure`,
`cancelled`, `sleeping` — each with its own prop and pose. She reacts only to
numbers the engine actually produced: `discovering` is unreachable with a
zero count, and `success` quotes the receipt verbatim. One motion switch
governs every animation in the app; reduced motion yields a static pose, not
a broken one. She is withheld from the findings screen so she never competes
with evidence, and from the confirmation dialog entirely.

### Fixed — found by running against live data

- **Overlapping roots double-counted.** Adding a folder inside an
  already-scanned root walked it twice and inflated every byte total. Roots
  are now reconciled, and the dropped count is reported.
- **Browser caches were classified `unknown`** for Brave, because the vendor
  token was written `brave-software` while the directory is
  `BraveSoftware\Brave-Browser`. 736 MB on the test machine was being
  withheld from the reclaimable figure. The catalogue and the classifier now
  share one vendor list.
- **`C:\Windows\Logs` was classified `protected`** because the cleanable-class
  list omitted `DiagnosticLogs`. The list is now derived from a named helper
  so a new category cannot be forgotten there again.
- **`dirs_seen` was reporting a file count.** It now reports folders walked.
- **`coverage.duration_ms` stopped before duplicate verification**, so it
  under-reported the cost of the phase that reads every byte.

### Fixed — pre-existing test failures

Three `cure_core` tests failed on any machine where `%TEMP%` is not under
`AppData\Local\Temp` (CI sandboxes, relocated profiles, containers): they
asserted a drop-zone score using the ambient `%TEMP%`. They now build their
own drop-zone fixture, plus a new test proving the expansion seam itself
still behaves. Risk logic is unchanged.

### Removed

`storage-core.js`, `storage.css` and the segmented-ring Disk Cleanup screen,
superseded by the Workstation. `cleanup.rs` and `cure cleanup scan|run` remain
and are unchanged — the CLI cleanup flow is a separate, still-tested surface.

### Fixed — found while animating

Running the rig's own time-slice harness surfaced seven defects that no
static capture would show, all fixed in the two Luma files: orphaned scan
HUD paths that never rendered, props re-interpolating from the previous
state's anchor on every change (replaced with a per-change entrance pop), a
pet-pulse class that never released and permanently replaced her breathing
loops, a mixed-shape beat table that crashed `pickBeat`, scoped `onHost`
apply that left the host's `data-state` stale, wrap-jump and anchor
misplacements in the success/quarantine props, and gradient mapping that
painted her face black. Full list and choreography contract:
[`docs/LUMA-COMPANION-VERIFICATION.md`](LUMA-COMPANION-VERIFICATION.md).

### Verification

413 Rust tests (48 new for the storage engine), 485 browser checks (up from
428), clippy and fmt clean in both workspaces, `node --check` on every
shipped script. Native smoke test: the release `cure-gui.exe` launches,
opens a window titled `C.U.R.E — Evidence Console`, runs at ~27 MB working
set, and writes only under `%LOCALAPPDATA%\CURE`.
Native WebView2 validation additionally drove the real Storage
Workstation end-to-end inside the rebuilt binary — live-machine scan with
honest partial coverage, SHA-256 duplicate verification, confirmed removal
with a plain-language no-undo receipt, and post-change invalidation — with
Luma mapping every observed event and no console or page errors. Measured
animation cost: ~102% of one core while fully animating at the 144 Hz
bench, ~1.8% under the pause blanket, effectively zero when hidden or
occluded.

Benchmarks, with the exact machine and commands:
[`docs/benchmarks/storage-engine.md`](benchmarks/storage-engine.md).

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
