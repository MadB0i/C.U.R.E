# C.U.R.E 0.2.0 release readiness

Prepared 2026-10-04. **Recommended version: v0.2.0.** The redesign is a
substantial pre-1.0 GUI update with additive evidence/coverage metadata.
The public v0.1.0 release and its tag remain untouched.

**Exact implementation/media/package candidate commit:**
`267cbf9d42835763297f55e6119269413441c75a`

Branch: `codex/evidence-console`. This report is a subsequent documentation
commit; it does not change the tested application or candidate artifacts.
No branch was pushed, tag created, or release published by this task.

## Required release assets

| Asset | Local candidate SHA-256 |
| --- | --- |
| `cure-gui.exe` | `ba007e5d7f2616693c3258ed0cdf5118c5341b9f2de2f958045b57ee63993de4` |
| `cure.exe` | `54d4c6847a25b5024061005f7fd482dc3fd3d9429c3dcd2323fa5ebb2d034beb` |
| `cure-watch.exe` | `ed06ec5cdfa366be737c4890679cb1aa1a5a59aec807f65650ced9eacc6a0c88` |
| `cure-v0.2.0.zip` | `86d4c82d5040b7ac7b76765363c1728f831dac5ef50fa2072aff4eceafe1c805` |
| `SHA256SUMS.txt` | External manifest covering all three EXEs and the ZIP |

The locally prepared ZIP is **11,879,615 bytes** at
`release-candidate/v0.2.0/cure-v0.2.0.zip`. The directory is intentionally
ignored by Git. EXEs are in its `portable/` directory; the external manifest
is beside the ZIP. ZIP entries and hashes of archived EXEs were verified
against the manifest, as were all four external hashes.

These hashes describe this local build. Release-runner builds may differ;
publish the checksums generated from the actual uploaded artifacts.

## Portable ZIP contents

17 files, no installer or external frontend runtime dependency:

- `cure-gui.exe`, `cure.exe`, `cure-watch.exe`
- `README.md`, `LICENSE`, `SECURITY.md`, `RELEASE-NOTES.md`
- `BUILD-INFO.txt`: version, exact source commit and unsigned disclosure
- `SHA256SUMS.txt`: three executable hashes
- `docs/screenshots/01-overview.png` through `07-all-clear.png`
- `docs/media/demo.gif`

Windows 10/11 and the host's WebView2 runtime are required. GUI data defaults
to `%LOCALAPPDATA%\CURE`, not the rescue drive or executable directory.
Watcher installation/pairing remains separately consented.

## Integrity and unsigned disclosure

All three local EXEs were inspected with `Get-AuthenticodeSignature` and
reported **NotSigned**. The GUI PE file/product version reports 0.2.0;
`cure --version` reports 0.2.0. The watcher has no `--version` handler;
its version is checked from workspace Cargo metadata.

README and release notes disclose unsigned binaries, possible SmartScreen
prompts and the WebView2 requirement. SHA-256 validates bytes against a
trusted checksum source; it does not authenticate a publisher. The external
manifest covers the ZIP as well as independent EXE downloads; the internal
manifest covers the EXEs only. Verify full values with
`Get-FileHash -Algorithm SHA256`.

`tools/package-candidate.ps1` stages committed source and refuses an existing
output directory. The release workflow generates equivalent assets,
includes source metadata, and refuses an existing published release; lookup
errors other than an explicit 404 fail closed. **Do not overwrite v0.1.0.**

## Presentation status

README opens with C.U.R.E, the evidence-first subtitle, Overview screenshot,
three factual values, latest-release link and short demo. It explicitly
distinguishes the prepared candidate from the existing public v0.1.0.

Seven refreshed PNGs are 1440×900 and visually reviewed: Overview, live
Evidence Sweep, review required, evidence inspector, quarantine confirmation,
quarantine and no findings. The demo GIF is 1100×688, 113 frames, about 14
seconds; the MP4 is also refreshed. Both use labelled sample data and show
launch → scan → sweep → finding → inspector → confirmation → quarantine →
undo. The social preview now uses the Overview screen.

## Verification and CI status

See [the verification report](EVIDENCE-CONSOLE-VERIFICATION.md) for commands,
coverage and limits. Local results: 364 Rust workspace tests; four
headless-safe Tauri tests; formatting and strict Clippy in both workspaces;
release builds; JavaScript syntax and offline checks; 262 browser checks;
80 layout, 68 pixel and 102 capture checks; real isolated CLI baseline and
quarantine/undo regressions. Existing verification command also passed 259
checks (the full suite adds three pixel checks).

Baseline CI at `4bcdbf85ab52c2027397aa365d4c8081dc9b578d`
[completed successfully](https://github.com/MadB0i/C.U.R.E/actions/runs/36026040878).
**Remote CI for this candidate is pending:** the branch has not been pushed.
The updated CI adds the Python Playwright accessibility/layout/state/pixel
gate and GUI formatting/Clippy. The updated release workflow has been
inspected but not executed; no publishing operation was performed.

Before publishing, require successful remote CI on the chosen release
commit and native Windows/WebView2 plus human screen-reader validation.
If code changes during that validation, rebuild and regenerate checksums;
do not reuse this manifest for different artifacts.

## Known limitations

- Mock Chromium tests cannot prove native WebView2 behavior, elevated access,
  real collector results, or a human NVDA experience. The opt-in real desktop
  overlay fixture and native E2E driver were not run on this host.
- Only Registry, Tasks, Services and WMI expose explicit coverage rows. Other
  completed collectors are labelled “Coverage not reported”. Watcher/USB
  connectivity has no GUI status API and is shown as unavailable.
- Signature/publisher/hash/target metadata is on demand and can be absent.
  Legacy quarantine records may lack integrity/security metadata; that
  absence is visible. ACL/timestamp restore remains best effort.
- Quarantine moves supported file-backed Startup/Task artifacts only after
  confirmation. It does not terminate a process or automatically remove
  Registry/Service/WMI/IFEO/AppInit/COM persistence. Cleanup is a separate,
  explicitly confirmed permanent deletion action.
- Findings are heuristic evidence, not proof of malware. Canary is an
  experimental tripwire; the local IOC provider remains a demo fixture.
- Session timeline is not a persistent forensic audit log. Report export
  performs a fresh read-only scan rather than exporting a frozen snapshot.
- Binaries are unsigned and the public release is still v0.1.0.
