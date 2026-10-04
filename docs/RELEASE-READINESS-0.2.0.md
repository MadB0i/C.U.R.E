# C.U.R.E 0.2.0 release readiness

Prepared 2026-10-04. **Recommended version: v0.2.0. READY TO PUSH for PR CI;
publication still requires successful remote CI and the human screen-reader gate.**
The public v0.1.0 release and its tag remain untouched.

**Exact tested source/media/package commit:** `d225a9d0feee994880078bc7bbd3736416bddace`

Branch: `codex/evidence-console`. This report is a subsequent documentation
commit. It does not change the tested executable bytes. No push, merge, tag
or publication was performed. See [Rescue Console visual verification](UI-REDESIGN-VERIFICATION.md).

## Candidate replacement

The original `release-candidate/v0.2.0/` from
`267cbf9d42835763297f55e6119269413441c75a` is **REJECTED / SUPERSEDED**.
The final audit found embedded development assets/test drivers and builder
paths. That directory is preserved, not overwritten; do not distribute it.
The prior verified `release-candidate/v0.2.0-final/` is now **SUPERSEDED** by
the companion pass. It is preserved, not overwritten. Use only
`release-candidate/v0.2.0-companion/` for the current local candidate.

## Required release assets

| Asset | Local candidate SHA-256 |
| --- | --- |
| `cure-gui.exe` | `e991770dc0f398b9bd2c193cbe3772b55b5385927f9d586f8d9d28373b24b4ba` |
| `cure-watch.exe` | `7fe70d4d3c7b252a1f4c2f348f5ef98bf9866facbe594bc4a8eca03143912589` |
| `cure.exe` | `9338c30e85f9e4c3fc1e900ce33a9194f3dcf11bf8e461dc0b6c423e5210d28e` |
| `cure-v0.2.0.zip` | `c637ea02a094b5b6d9acf1460e3c2f00d81feda6f8c70318c538ac2dd4f4f82f` |
| `SHA256SUMS.txt` | External manifest covering three EXEs and the ZIP |

ZIP: **13,462,328 bytes**. The ignored candidate directory contains the ZIP,
external manifest and `portable/` staging directory. The GUI is byte-identical
to the production executable used for the final 50 native checks. All 19 ZIP entries
match staging byte-for-byte. All four external and three internal hashes pass
`python tools/audit-candidate.py release-candidate/v0.2.0-companion`.

These are local build hashes. Release-runner bytes may differ; publish
checksums generated from the actual uploaded artifacts, never reuse this
manifest for a different build.

## Portable ZIP contents

Exactly 19 files:

- `cure-gui.exe`, `cure.exe`, `cure-watch.exe`
- `README.md`, `LICENSE`, `SECURITY.md`, `RELEASE-NOTES.md`
- `BUILD-INFO.txt`: version, full source commit, unsigned disclosure
- `SHA256SUMS.txt`: three executable hashes
- `docs/screenshots/01-overview.png`
- `docs/screenshots/02-scan.png`
- `docs/screenshots/03-review-required.png`
- `docs/screenshots/04-evidence-inspector.png`
- `docs/screenshots/05-quarantine-confirm.png`
- `docs/screenshots/06-quarantine.png`
- `docs/screenshots/07-all-clear.png`
- `docs/screenshots/08-cleanup.png`
- `docs/screenshots/09-cleanup-result.png`
- `docs/media/demo.gif`

No source, mocks, test drivers, PDBs, temporary captures or installers are
packaged. Production Tauri embeds an explicit six-file frontend allowlist,
including the offline SVG companion script and stylesheet.
Artifact checks find no current builder workspace/profile paths or common
credential markers in EXEs. Windows 10/11 and host WebView2 are required.
Data defaults to `%LOCALAPPDATA%\CURE`. Watcher consent/pairing is separate.

## Integrity and unsigned disclosure

All three EXEs report **NotSigned**. GUI PE file/product version: 0.2.0.
`cure --version`: 0.2.0. CLI/watcher have no embedded PE version resource;
watcher version comes from workspace metadata and has no `--version` handler.
README and release notes disclose unsigned binaries and SmartScreen prompts.
SHA-256 verifies bytes against a trusted manifest; it does not authenticate
the publisher. Verify full values with `Get-FileHash -Algorithm SHA256`.

`tools/package-candidate.ps1` requires clean committed source, builds using
`tools/build-release.ps1` with path remapping, and refuses an existing output
directory. The release workflow uses the same production build script and
refuses an existing published release; only an explicit 404 permits creation.
**Do not overwrite v0.1.0.**

## Presentation and verification

README retains the evidence-first title/subtitle, the Rescue Console hero,
release link and labelled sample workflow. Nine 1440×900 PNGs were reviewed
individually. GIF: **1000×640, 146 frames, 20.85 seconds, 7 fps**; MP4 refreshed.
Blank recording lead-in removed. The sequence covers launch, scan, System
X-Ray, finding, inspector, confirmation, quarantine, undo and separately
confirmed cleanup. Luma remains presentation-only; technical records and
consequence copy remain serious. Mock cleanup counts/rescans match successful
selections.

The visual identity was replaced in a later pass — see
[rescue console redesign verification](UI-REDESIGN-VERIFICATION.md). It
changed no backend behaviour, and the checks below were re-run against it.

Local final gates: **364 Rust tests; 4 Tauri tests (1 ignored); formatting;
strict Clippy (GUI all features); production release builds; JS syntax;
428 browser checks (all); 423 verify; 153 layout; 97 pixel; 185 capture; demo
capture; real CLI baseline and quarantine/undo regressions; exact package
audit; production asset-allowlist parity.** These browser/layout/pixel counts
overlap and are not summed.

Remote CI for this branch is **NOT RUN / pending push and PR**. Workflow source
was reviewed; the release publishing job was not executed. Merge only after
successful CI on the PR head and the human accessibility pass below.

## Release-relevant limitations

- Human NVDA, live overlay close/force, elevated comparison, real OS
  reduced-motion toggle, and other physical monitor DPIs: **NOT VALIDATED**.
  Actual host 125% DPI and emulated 900×600/200% scaling were tested. The prior
  audit's physical window-resize gate was not repeated in the companion pass.
  Native permanent disk cleanup was not exercised against host files; browser
  mock checks and existing Rust tests cover that behavior. Its busy state has
  no invented incremental progress because the engine provides only a result.
- Only Registry, Tasks, Services and WMI report explicit coverage. Other
  collectors show coverage not reported. Watcher/USB status has no GUI API.
- Evidence metadata may be unavailable; legacy quarantine records can lack
  fidelity metadata. ACL/timestamp restoration remains best effort.
- Quarantine moves supported Startup/Task files after confirmation; it does
  not stop running malware or change Registry/Service/WMI persistence.
  Cleanup is a separate confirmed permanent deletion action.
- Heuristic findings are not proof of malware. Canary is experimental; local
  IOC provider remains a demo fixture. Same-user compromise is outside the
  enforcement model. Session timeline is not a persistent forensic log;
  export performs a fresh scan rather than freezing the last scan.
- Binaries are unsigned. Public release remains v0.1.0 until explicitly
  authorized publication of a new tag.
