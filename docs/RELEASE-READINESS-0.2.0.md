# Release readiness — 0.2.0

**Date:** 2026-10-20
**Commit at the time of writing:** the working tree this document describes
is uncommitted. No commit, tag, push or publication has been made.

This document records what has been verified, how, and what has not. It
does not claim readiness for anything that was not measured.

---

## What was verified, and how

| Area | How it was checked | Result |
|---|---|---|
| Rust workspace | `cargo test --workspace` | 413 passed, 0 failed |
| Storage engine | `cargo test -p cure_core --lib storage` | 63 passed, 0 failed |
| GUI crate | `cargo test --manifest-path gui/src-tauri/Cargo.toml` | 4 passed, 1 ignored (the ignored one spawns real windows) |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` and the same for the GUI crate | Clean |
| Formatting | `cargo fmt --check` for both workspaces | Clean |
| JavaScript | `node --check` on all five shipped scripts | Clean |
| Browser quality suite | `python gui/devtools/evidence-console.py all` | 499 checks passed |
| Native journey | `python testing/native/native-e2e.py` | 28 checks passed against the real binary |
| Real storage measurement | `cure.exe storage scan --depth quick` and `--depth deep --abandoned` on the build machine | 24,138 files / 2,528 folders in 20 s; 829,716 files in 466 s |
| Windows Update classification | the same real scan | reported as `review_carefully`, not `safe_to_review` |
| Volume probe | the same real scan | `DATA — 401.12 GB free of 931.51 GB`, volume root resolved correctly |
| Installer | `cargo tauri build --bundles nsis` | `CURE_0.2.0_x64-setup.exe`, 2.32 MiB |
| Install | silent install to an isolated directory | Exit 0; `cure-gui.exe` and `uninstall.exe` placed; Start Menu shortcut and uninstall registry entry created |
| Installed application | launched from the isolated directory | Window title `C.U.R.E — Evidence Console`, 35.4 MB working set |
| Uninstall | silent uninstall | Exit 0; install directory, Start Menu shortcut and registry entry all removed |
| Candidate audit | `python tools/audit-candidate.py <candidate>` | Passes on a correctly staged candidate; correctly **fails** the three stale candidates from earlier builds |

### The native journey, in detail

`testing/native/native-e2e.py` launches the real release binary with the
WebView2 DevTools port open and drives the real webview. It proves:

* the window starts and the real Tauri bridge is present (not the mock);
* every navigation target opens exactly one view;
* a real storage scan runs, shows real progress, and finishes;
* the coverage report lists the roots that were measured;
* a fixture file the test created is measured, listed and queued;
* the preview lists what will go, states the consequence in plain
  language, and cannot be confirmed without the exact word;
* the confirmation dialog keeps the keyboard inside it;
* the removal runs and the receipt separates approved, removed, refused and
  failed, and does not present an estimate as verified;
* the approved file is gone;
* a cancelled scan reports itself as cancelled;
* the companion reaches every state the interface can ask for;
* reduced motion stops every companion animation;
* nothing new is written next to the executable.

The only file it removes is one it created itself, inside a directory it
created itself, inside the operating system's temporary directory. It
removes that directory when it finishes, pass or fail.

---

## What is not verified

* **Signed binaries.** The executables and the installer are unsigned.
  Windows will show a SmartScreen warning. This is disclosed in the
  README, the release notes and `BUILD-INFO.txt`.
* **The full release publish path.** The workflow was updated but not
  executed, because publishing was out of scope.
* **The canary tripwire's triggered path.** The guard is exercised; the
  real filesystem event that trips it is still a manual test.
* **The USB watcher against real hardware.** Its unit tests pass; the
  paired-hardware run is manual and is not part of this release.
* **A light theme.** The console has no light theme. The companion's light
  palette exists and is untested against a light console.

---

## Known limitations that are disclosed, not hidden

* Storage cleanup is the one permanent deletion in the product. It is
  gated on a scan, a preview, a typed phrase, a plan digest and a
  per-item re-check.
* The companion is a secondary figure. It is authored for 96–158 px.
* Unelevated scans show INCOMPLETE rows rather than guessing.
* Last-access timestamps are recorded and never reasoned from.

---

## Before publishing

1. Commit the working tree, including the two new documentation
   screenshots that the README references.
2. Run `python tools/audit-candidate.py candidate` on a freshly staged
   candidate. The workflow does this as a required step.
3. Confirm the release notes describe the application that is actually in
   the archive.
4. Disclose the unsigned status prominently — it already is, in three
   places.

## The three stale candidates

`release-candidate/v0.2.0`, `v0.2.0-companion` and `v0.2.0-final` all
predate this work. Two of them fail the candidate audit for the right
reason (they carry the old screenshot names, and one still embeds the
development mock). **None of them should be published.** The audit now
refuses them, which is the point of the gate.
