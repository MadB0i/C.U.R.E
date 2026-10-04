# C.U.R.E 0.2.0 release readiness

Prepared 2026-10-04. **Recommended version: v0.2.0. READY TO PUSH for PR CI;
publication still requires successful remote CI and the human screen-reader gate.**
The public v0.1.0 release and its tag remain untouched.

**Exact tested source/media/package commit:** `ee44f787749175fb50fd6069b1669314a08f309b`

Branch: `codex/evidence-console`. This report is a subsequent documentation
commit. It does not change the tested executable bytes. No push, merge, tag
or publication was performed. See [the final audit](FINAL-RELEASE-AUDIT-0.2.0.md).

## Candidate replacement

The original `release-candidate/v0.2.0/` from
`267cbf9d42835763297f55e6119269413441c75a` is **REJECTED / SUPERSEDED**.
The final audit found embedded development assets/test drivers and builder
paths. That directory is preserved, not overwritten; do not distribute it.
Use only `release-candidate/v0.2.0-final/` for this locally verified candidate.

## Required release assets

| Asset | Local candidate SHA-256 |
| --- | --- |
| `cure-gui.exe` | `9b5a868617b1cc82b3096b65d6e8d19add055f161cbe8b58394022941fff0bc6` |
| `cure-watch.exe` | `7fe70d4d3c7b252a1f4c2f348f5ef98bf9866facbe594bc4a8eca03143912589` |
| `cure.exe` | `9338c30e85f9e4c3fc1e900ce33a9194f3dcf11bf8e461dc0b6c423e5210d28e` |
| `cure-v0.2.0.zip` | `2811ec75dc7f2e29786b08a42fc5ec8e9aad59eccd42eeebcbe73ec19d1ad3cb` |
| `SHA256SUMS.txt` | External manifest covering three EXEs and the ZIP |

ZIP: **12,720,749 bytes**. The ignored candidate directory contains the ZIP,
external manifest and `portable/` staging directory. The GUI is byte-identical
to the production executable used for 45 native checks. All 17 ZIP entries
match staging byte-for-byte. All four external and three internal hashes pass
`python tools/audit-candidate.py release-candidate/v0.2.0-final`.

These are local build hashes. Release-runner bytes may differ; publish
checksums generated from the actual uploaded artifacts, never reuse this
manifest for a different build.

## Portable ZIP contents

Exactly 17 files:

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
- `docs/media/demo.gif`

No source, mocks, test drivers, PDBs, temporary captures or installers are
packaged. Production Tauri embeds an explicit four-file frontend allowlist.
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

README retains the evidence-first title/subtitle, Overview hero, three factual
values, release link and labelled sample workflow. Seven 1440×900 PNGs were
reviewed individually. GIF: **1100×688, 122 frames, 15.26 seconds, 8 fps**;
MP4 refreshed. Blank recording lead-in removed. The sequence covers launch,
scan, sweep, finding, inspector, confirmation, quarantine and undo.

Local final gates: **364 Rust tests; 4 Tauri tests (1 ignored); formatting;
strict Clippy (GUI all features); production release builds; JS syntax;
268 browser checks; 80 layout; 68 pixel; 108 capture; demo capture;
real CLI baseline and quarantine/undo regressions; 45 production native
checks; six native axe scans with zero violations; exact package audit.**
These browser/layout/pixel counts overlap and are not summed.

Remote CI for this branch is **NOT RUN / pending push and PR**. Workflow source
was reviewed; the release publishing job was not executed. Merge only after
successful CI on the PR head and the human accessibility pass below.

## Release-relevant limitations

- Human NVDA, live overlay close/force, elevated comparison, real OS
  reduced-motion toggle, and other physical monitor DPIs: **NOT VALIDATED**.
  Actual host 125% DPI and emulated 200% scaling were tested.
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
