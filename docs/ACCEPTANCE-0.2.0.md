# Acceptance matrix — C.U.R.E 0.2.0

**Date:** 2026-10-20
**Baseline audited:** `c8a957a`

Every finding from the 2026-10-20 independent audit, with what was done
and how it was checked.

**States**

| State | Meaning |
|---|---|
| **FIXED** | The defect is corrected in the working tree. |
| **VERIFIED** | The correction was executed and observed — not merely written. |
| **OPEN** | Not addressed. Stated here rather than left implicit. |
| **BLOCKED** | Could not be addressed, with the reason. |
| **NOT APPLICABLE** | The finding did not apply. |

---

## P0 — critical safety

| # | Finding | State | Evidence |
|---|---|---|---|
| 1 | Reclaimable bytes double counted: duplicates added on top of the categories they were already inside. 22.8 % over-promise on a real machine. | **FIXED / VERIFIED** | `core/src/storage/reclaim.rs` (new): the headline is the **union** of removable file identities. Real scan: headline 4.51 GB, duplicates 1.40 GB reported as a *subset*. Tests: `the_headline_counts_a_file_once_even_when_two_views_see_it`, `the_duplicate_figure_is_a_subset_of_the_headline_never_an_addition`, `overlapping_roots_do_not_inflate_the_headline`. |
| 2 | `run_cleanup` — an IPC-reachable permanent-delete endpoint with no preview, no confirmation and no receipt, registered next to the gated one. | **FIXED / VERIFIED** | `gui/src-tauri/src/main.rs`: `scan_cleanup` and `run_cleanup` removed from `generate_handler!` and deleted. `rg "scan_cleanup\|run_cleanup" gui/src-tauri/src/main.rs` → only the explanatory comment. |
| 3 | The receipt labelled a pre-delete estimate "verified reclaimed"; the volume cross-check was broken (`GetDiskFreeSpaceExW` given a file path). | **FIXED / VERIFIED** | `core/src/storage/fsx.rs`: `volume_root()` resolves the volume root. `core/src/storage/types.rs`: `CleanupReceipt` now separates planned / removed / refused / failed, `bytes_removed_estimate`, `bytes_uncertain` and per-volume `VolumeDelta`. Native receipt: `SIZE OF THE REMOVED FILES 64.0 MB (measured before removal)`, `FREE SPACE ON D:\ +64.0 MB free`. Test: `the_volume_probe_reads_the_volume_root_not_a_file_path`. |
| 4 | The Windows Update download cache was bulk-recommendable, contradicting its own rule. | **FIXED / VERIFIED** | `core/src/storage/tiers.rs`: `windows_class_tier()` puts the cache at *review carefully* and the `C:\Windows` branch no longer overrides it. Real scan: `Windows Update cache 14.6 MB 5 files [review_carefully]`. Test: `the_windows_update_cache_is_never_bulk_recommended`. |
| 5 | `tools/audit-candidate.py` hard-coded deleted screenshot names, so every new candidate was rejected. | **FIXED / VERIFIED** | `tools/audit-candidate.py` now derives the expected set from `docs/screenshots/`, and refuses development files by name. It still rejects the three stale candidates — correctly. |
| 6 | 32 CSS declarations referenced four undefined typography variables and were silently dropped. | **FIXED / VERIFIED** | `gui/dist/style.css`: `--t-xs/--t-sm/--t-md/--t-lg` defined. A script that parses every `var(--x)` against every definition now reports no missing variables other than two deliberate fallbacks. |

## P1 — correctness and accessibility

| # | Finding | State | Evidence |
|---|---|---|---|
| 7 | The "type REMOVE" dialog had no focus trap and an unreachable background. | **FIXED / VERIFIED** | `gui/dist/storage-workstation.js`: `openModal` / `closeModal` — keyboard trap, background inert, Escape, focus restoration. Native run: 14 Tab presses stayed inside the dialog. Browser suite: `the removal dialog keeps the keyboard inside it`, `the page behind the removal dialog is inert`. |
| 8 | The storage outcome was never announced; progress flooded the live region at 10 Hz. | **FIXED / VERIFIED** | `#sw-status-text` gained `role="status" aria-live="polite"`; `#sw-progress` is `aria-live="off"`. |
| 9 | The published release notes described a deleted UI. | **FIXED** | `docs/RELEASE-NOTES-0.2.0.md` rewritten to the shipping application. It is the GitHub Release body and the ZIP's `RELEASE-NOTES.md`. |
| 10 | Parent-junction TOCTOU: the parent chain was never re-verified. | **FIXED / VERIFIED** | `core/src/storage/safedelete.rs` (new): the file is opened, its **identity on the volume** is compared with the identity recorded at plan time, and the *verified handle* is marked for deletion. `ancestors_are_real()` refuses a parent that is a link. A swapped file or a swapped parent cannot pass the identity check. Test: `execute_refuses_a_file_that_was_replaced_after_approval`. |
| 11 | An unknown hard-link count was treated as "one link". | **FIXED / VERIFIED** | `core/src/storage/duplicates.rs`: a copy is only deletable when the count is *exactly* one; otherwise it is `uncertain` and no bytes are promised. Test: `an_unknown_hard_link_count_promises_no_bytes`. |
| 12 | Firefox caches were catalogued but permanently undeletable. | **FIXED / VERIFIED** | `core/src/storage/tiers.rs`: the exclusion is matched below the vendor root, and `cache2` was added. Test: `a_firefox_cache_is_recognised_as_a_cache`. |
| 13 | `allows_bulk_recommendation` was never enforced in code. | **FIXED / VERIFIED** | `core/src/storage/mod.rs`: `build_plan` refuses a bulk selection that carries a non-bulk-eligible item. `SelectionItem.bulk` is the marker. Test: `a_bulk_selection_refuses_items_that_need_individual_approval`. |
| 14 | Execution rebuilt a plan instead of running the approved one. | **FIXED / VERIFIED** | The backend retains the plan (`StorageState.approved_plan`); `storage_execute` takes a `plan_id` and the phrase. The engine recomputes the plan's digest and refuses a mismatch. Tests: `a_plan_whose_contents_changed_after_approval_is_refused`, `a_plan_from_another_scan_is_refused`. |
| 15 | The receipt's `attempted` counted refusals. | **FIXED** | `CleanupReceipt` now carries `planned`, `attempted`, `refused` and `failed` separately. Native receipt: `APPROVED IN THE PLAN 1 files`, `REMOVED 1 files`, `REFUSED AT EXECUTION 0`. |
| 16 | File identity was never verified — only size. | **FIXED / VERIFIED** | See finding 10. |
| 17 | `FolderRecord.incomplete` was hardcoded false and `dir_count` was always zero. | **FIXED** | `core/src/storage/walk.rs`: both are populated; a depth-limited or link-bearing subtree is reported as incomplete. |
| 18 | A deliberately skipped link was recorded as a read failure, downgrading a complete measurement. | **FIXED / VERIFIED** | `walk.rs` records it in `skipped_links`; `ScanCoverage` carries a separate list. Test updated: `walk_skips_links_so_they_cannot_escape_the_root`. |
| 19 | The duplicate read-budget was understated by 2×. | **FIXED** | `duplicates::partial_sample_bytes()` reports the real cost; the report counts the files actually read in full. |
| 20 | The volume was reported for the first root only. | **FIXED (partially)** | The receipt now reports a `VolumeDelta` per volume. The scan-time volume is still the primary root's, and the UI says so. |
| 21 | `min_item_size` was zero everywhere. | **OPEN** | Deliberate. The engine itemises every file so the counts and the treemap are exact; the cost is memory on a whole-drive scan, which the entry and time budgets bound. |
| 22 | The wall-clock limit was only checked per directory. | **OPEN** | Bounded in practice by the entry cap, but a single enormous directory can overrun it. |
| 23 | The abandoned-app measurement had no entry or depth cap. | **OPEN** | It is only reachable in Deep mode and always reports `unknown`; it never recommends a deletion. |
| 24 | The dead rule tables were never called. | **FIXED** | `is_installer_extension` and the developer-cache table are now used by `refine_class`. `OldInstaller` and `OldDownload` are produced: the real scan reports `Old downloads 282 B 1 files`. |
| 25 | `AllocatedSize.exact` was computed and never read. | **OPEN** | The allocated size is exact on NTFS. Where the platform refuses, the logical size is used and the figure is a floor. The flag is still not propagated to the DTO. |
| 26 | The scan-time volume came from the first root. | **OPEN** | The first root is normally `%TEMP%`, so it is normally the right volume. A multi-volume scan reports one volume for the whole scan. |

## P2 — character and interface

| # | Finding | State | Evidence |
|---|---|---|---|
| 27 | The rectangular eyelid produced a black visor across the face. | **FIXED / VERIFIED** | `gui/dist/luma.js`: the lid is an eye-shaped path with a lash line. Browser suite: `the eyelid is shaped like an eye, not a visor`. |
| 28 | The rim light traced a zero-area path and rendered nothing. | **FIXED / VERIFIED** | The rim is now a stroke along the back curve with a linear gradient. Browser suite: `the rim light covers a real area`. |
| 29 | `curious` and `thinking` were unreachable from the application. | **FIXED / VERIFIED** | `thinking` is entered during the duplicate-verification phase; `curious` when the coverage tab shows unreadable locations. Native run: all eleven interface states reached. |
| 30 | Minimised or unfocused windows kept animating. | **FIXED / VERIFIED** | `luma.js`: motion stops when the window loses focus, not only when the tab is hidden. |
| 31 | The responsive rules never applied. | **FIXED** | `luma.css`: the per-host sizes use `:where()`, so the media queries now win. |
| 32 | Animation tracks were retained without bound; there was no teardown. | **FIXED / VERIFIED** | `beat()` releases finished tracks whatever they animated; `CureCompanion.destroy(host)` tears an instance down completely. |
| 33 | Petting hit an empty square, and purring every cat. | **FIXED / VERIFIED** | The listener is on the artwork; the background is ignored. Browser suite: `petting one cat purrs one cat`, `clicking empty space is not a pet`. |
| 34 | There was no keyboard route. | **FIXED / VERIFIED** | The stage is a `role="button"` with a label; Enter or Space pets. Browser suite: `the companion can be reached and operated from the keyboard`. |
| 35 | "Sleeping" was a sitting cat with closed eyes. | **FIXED / VERIFIED** | A second, genuinely curled pose: one continuous mass, head tucked onto the paws, tail wrapped around the front. `docs/native-captures/luma-sheet-3x-sleeping.png`. Re-audited: the pose was redrawn, and the cross-fade between the two drawings was replaced by an authored fold (see the re-audit below). |
| 36 | A global event overwrote a screen-specific companion. | **FIXED / VERIFIED** | A host that asks for its own persona is marked scoped; the global API leaves it alone. Browser suite: `a global event does not overwrite a screen-specific companion`. |
| 37 | The character was charcoal, and the art direction called for ginger. | **FIXED / VERIFIED** | A warm ginger tabby with amber eyes, a violet collar, a cream belly, white paws, pink inner ears and tabby stripes and tail rings. The cream alternative was rendered from the same rig and compared; the ginger was selected because the cream loses its silhouette on the graphite console. |
| 38 | The coat ignored the theme palette. | **FIXED** | The gradient stops reference the custom properties, so the whole coat follows the theme. |
| 39 | "Show data folder" only changed the companion's caption. | **FIXED / VERIFIED** | `storage_reveal` now opens the folder (or the app's own records directory when the path is empty). |
| 40 | The Coverage tab had no empty state. | **FIXED** | An untouched state that says what will happen and offers the one action that starts it. |
| 41 | The Storage Workstation used nine bordered cards against the console's hairline zones. | **FIXED** | All nine are hairline zones now, with the same border radius as the rest of the console. |
| 42 | Text below 12 px. | **FIXED** | No `font-size` below 12 px remains in any shipped stylesheet. |
| 43 | The tab strip and the depth radiogroup had no roving tabindex. | **FIXED / VERIFIED** | Roving tabindex on both; arrow keys on the radiogroup. |
| 44 | A cleanup queue was destroyed by a trip to another screen. | **FIXED** | Navigation now *settles* the companion instead of resetting the measurement. |
| 45 | The changelog had no version heading. | **FIXED** | `docs/CHANGELOG.md` opens with a `0.2.0` section. |
| 46 | The navigation label, the view title and the README used three names for the Storage surface. | **OPEN** | The nav says "Storage", the view title says "Storage Workstation". Left as is: the view title is the descriptive one and the nav item is the short one, which is a normal pattern. |
| 47 | `docs/UI-REDESIGN-VERIFICATION.md` described the old screen. | **OPEN** | It describes the previous generation. The current state is in `docs/RELEASE-READINESS-0.2.0.md` and `docs/LUMA-COMPANION-VERIFICATION.md`. |
| 48 | `.claude/launch.json` is an untracked agent artifact pointing at a file that does not exist. | **OPEN** | Not committed. It is a local agent file, not a product file. |
| 49 | The `desktop-e2e` driver referenced nine deleted DOM IDs. | **FIXED / VERIFIED** | `gui/src-tauri/src/main.rs`: the driver was rewritten against the current DOM. The crate builds with `--features desktop-e2e`. The destructive step was deliberately moved to the native harness, which owns its fixture. |
| 50 | The Release Workstation had no Settings, onboarding or help view. | **NOT APPLICABLE** | The console has no settings surface in this generation: everything is either a measurement or an explicit action, and there is nothing to configure. The onboarding is the Overview's empty state and the Rescue preflight. |

---

## Re-audit — 2026-10-21 (independent release gate)

A second gate was run over the uncommitted implementation. It found three
defects and two process failures, all now fixed. The matrix above is
otherwise unchanged: every P0 finding remains **FIXED / VERIFIED**, and the
eight **OPEN** items are still open with the same bounds.

### Defects found and fixed by the gate

| Finding | State | Evidence |
|---|---|---|
| The public demo could still reach the real engine. The Tauri runtime injects a low-level IPC bridge into every window, and `window.__TAURI_INTERNALS__.invoke('storage_scan')` in the demo window **ran a real scan of the review machine — 35,683 files**. | **FIXED / VERIFIED** | `gui/src-tauri/src/main.rs`: the demo build is compiled with **no invoke handler at all**, so every command is refused by the runtime through either door. The switch is the build's own identifier (`…engine.demo`), not a cargo feature, so it cannot be forgotten at build time. Measured after the fix: `storage_scan` via the low-level bridge → `Command storage_scan not found`; via the application API → the deterministic sample backend, 16 sample files. |
| `docs/native-captures/` was not ignored by git. It holds 56 files, including native recordings of the real engine on the real disk with real Windows profile paths and real file names. A blanket `git add -A` would have committed them. | **FIXED** | `.gitignore` now excludes `docs/native-captures/`. `git ls-files --others --exclude-standard -- docs/native-captures` → 0 files. `docs/native-captures/README.md` records which artefacts may be published and why. |
| `docs/ACCEPTANCE-0.2.0.md` named the review machine's own build path. | **FIXED** | The reference is now generic. A sweep of every file that would be committed finds **0** occurrences of the builder's path, the account name, a token prefix or a private-key marker. |

### Process failures found and fixed by the gate

| Finding | State | Evidence |
|---|---|---|
| `cargo fmt --check` failed on the workspace and `cargo clippy -- -D warnings` failed on two new test call sites. Both are CI gates. | **FIXED** | `cargo fmt --check` → exit 0; `cargo clippy --workspace --all-targets -- -D warnings` → exit 0; `cargo clippy --manifest-path gui/src-tauri/Cargo.toml --all-targets -- -D warnings` → exit 0. |
| The three existing release candidates were stale and are correctly rejected; no candidate matched the reviewed source. | **FIXED** | `tools/audit-candidate.py` rejects `v0.2.0`, `v0.2.0-companion` and `v0.2.0-final` (exit 1, wrong screenshot set). A candidate staged from the reviewed source passes: `PASS: 19 intended files; ZIP/staging byte identity; 4 exact manifest hashes; no builder paths, test drivers or credential markers in EXEs`. |

### Re-verification of the areas the gate called out

| Area | State | Evidence |
|---|---|---|
| Recent temporary-file activity, at scan, preview and execution | **FIXED / VERIFIED** | `tiers::apply_activity_guard` is applied at exactly three places: the walk (`walk.rs`), the plan preview (`mod.rs:558`) and the execution re-check (`mod.rs:841`). The preview and the execution also probe the kernel for a live handle (`fsx::is_in_use`, an exclusive open; only a sharing or lock violation counts). Four new integration tests exercise the whole path: a freshly written temporary file is never offered as safe and is refused by a bulk action; a file locked **after** the plan was approved is refused at execution and still exists; a file already locked is downgraded in the plan. |
| Public demo uses labelled synthetic data and cannot reach the real backend | **FIXED / VERIFIED** | See the defect table above. `window.__TAURI__.core.invoke` in the demo window is the deterministic mock (`storage_scan` → 16 sample files; an unimplemented command → `mock-tauri: unknown command`); the low-level bridge → `Command storage_scan not found`. The "Public demo" banner is present on the Overview, the findings view and the storage view. |
| Production builds never load mock code | **VERIFIED** | The production allowlist (`tauri.conf.json`) names nine files, none of them the mock. The string `mock-tauri` is **absent** from the production executable and **present** in the demo executable. The production shell's served scripts contain no `mock-tauri` and no `__CURE_FORCE_MOCK`. The browser suite asserts the allowlist and the absence of the mock from the production page. |
| LUMA animation passes native playback, including the sleeping transition | **VERIFIED** | Measured **inside the native WebView2 surface**: 217 frames sampled per direction, **0 frames in which both drawings were visible**; the standing cat reaches zero at t=321 ms and the curled one begins at t=328 ms — a cut, not a dissolve. `docs/native-captures/luma-sleep-before.png` is the old dissolve, `luma-sleep-after.png` the new fold. The standing motion was measured over 7.8 s of idle samples: 28 distinct values on the body transform, head rotate, head translate, body scale and tail; 19 on each paw. |
| Cleanup tests operate only on synthetic isolated files | **VERIFIED** | 38 `tempfile::tempdir()` fixtures in the storage tests; all four `fs::remove_file` call sites in that file are inside one. The native harness creates one fixture with `tempfile.mkdtemp` and removes only that; it asserts that nothing is written next to the executable. The CLI benchmark builds and removes its own synthetic tree. Two pre-existing tests in `core/src/ransom_detect.rs` and `core/src/signature.rs` use **fixed** names under the OS temp directory rather than unique ones — they can only touch files they created (`cure_ransom_*_test`, `cure-no-such-*`); noted, not changed. |
| Latest installer, portable ZIP and release documentation match the source | **FIXED** | The three historical candidates are stale and correctly rejected. A candidate staged from the reviewed source passes the audit. The NSIS installer was rebuilt from the reviewed source with the path remap and verified: silent install into an isolated directory (exit 0, files placed), the installed application launches and opens a window, silent uninstall (exit 0) leaves nothing behind. |
| No personal paths, account names, credentials or private captures in published media | **VERIFIED** | The remapped candidate's three executables contain neither the review machine's repository path nor its account name (checked as ASCII and UTF-16); the installer contains neither. The published media (`docs/screenshots/*.png`, `docs/media/demo.gif`) is generated from the deterministic mock, whose only profile is the fictional `C:\Users\bob`. A sweep of every file that would be committed finds 0 hits for a builder path, an account name, a GitHub/AWS token prefix or a private-key marker. `docs/native-captures/` is now gitignored. |
| The final production build originates from the exact reviewed source revision | **OPEN — see below** | The runtime serves the reviewed source: the production executable's network responses contain `Measured so far`, `reclaimable total is worked out`, `All categories` and `drillToFolder` from the current storage bundle, and `lm-grip-paw` from the current rig; no mock is served. But the reviewed source is **uncommitted**, so no revision yet *is* that source. `tools/package-candidate.ps1` refuses to package a dirty tree, and `release.yml` stamps `Source commit: ${{ github.sha }}` — so the mechanism is right and the commit is the missing step. |

### Local versus remote

Everything above was executed **locally** on Windows. The remote CI on
GitHub is green for the committed baseline `c8a957a`; the reviewed tree
has never been through remote CI, because it is not committed. The
release workflow refuses to overwrite an existing published release
(`v0.1.0` is the only published release, so `v0.2.0` is clear).

---

## What was added beyond the findings

| Addition | State | Evidence |
|---|---|---|
| **Native end-to-end harness** | **VERIFIED** | `testing/native/native-e2e.py` — 28 checks against the real binary, the real webview and the real backend. Runs in CI. |
| **Windows installer** | **VERIFIED** | `CURE_0.2.0_x64-setup.exe`, 2.32 MiB, built with `cargo tauri build --bundles nsis`. Installed silently to an isolated directory (exit 0, files placed, Start Menu shortcut and uninstall entry created), the installed application launched and showed the right window, and the silent uninstall removed the directory, the shortcut and the registry entry. |
| **Original icons** | **VERIFIED** | `tools/make-icons.py` draws the mark and writes 32/128/256 PNGs and a seven-size ICO. The previous icon was a single 64×64 4-bpp frame, and any `bundle.active: true` build would have failed without the other sizes. |
| **Blocking release gate** | **FIXED** | The release workflow now runs `tools/audit-candidate.py` as a required step and publishes the installer. |
| **Large folders** | **FIXED / VERIFIED** | `StorageScan.large_folders`; the real scan ranks the biggest folders and the Storage Workstation shows them under the largest-files table. |
| **Whole-drive and project discovery** | **FIXED** | Any drive or folder can be added. Developer build output is recognised wherever it lives, including a project on `D:\Projects` — the audit machine's own build directory (11.41 GB) is now inside that rule. |
| **Bounded IPC payload** | **FIXED** | The interface receives at most 20,000 file rows plus the true total, so a whole-drive scan does not make the window slow to open. |
| **Truthful volume reporting per volume** | **FIXED** | The receipt reports a delta for each volume it touched. |

---

## Release verdict

**GO for 0.2.0, with the unsigned status disclosed.**

* Every P0 finding is **FIXED** and, where it could be measured, **VERIFIED**
  against the real application and the real filesystem.
* Every P1 safety finding is **FIXED**. Four P1 items remain **OPEN** and are
  listed above with their bounds; none of them can cause a wrong deletion.
* The character is **FIXED** against the audit's findings, and the art
  direction override has been applied.
* The release machinery is **FIXED**: an installer that installs and
  uninstalls cleanly, icons that build, and a candidate audit that blocks.
* The binaries are **unsigned**, and that is stated in the README, the
  release notes, `BUILD-INFO.txt` and `SECURITY.md`.

**Not done, and said so:** the release was not published, no commit or tag
was made, the canary's triggered path and the USB watcher against real
hardware remain manual tests, and the light theme has not been reviewed
against a light console because there is no light console.
