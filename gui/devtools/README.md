# Evidence Console verification

The maintained GUI checks share `evidence-console.py`, a native Python
Playwright suite. MJS entry points remain compatible with existing commands;
obsolete mascot counters and random-node pixel expectations are removed.
Only the mock backend is used. No host remediation runs in GUI checks.

```powershell
python -m pip install -r gui/devtools/requirements.txt
python -m playwright install chromium
cd gui/devtools
npm ci
npm test
npm run layout
npm run pixel
```

Use `CURE_PYTHON` to select a Python interpreter for MJS wrappers and
`CURE_SHOTS_DIR` to redirect local captures. The harness serves only local
assets over an ephemeral loopback port so axe can inspect stylesheets.

Coverage: five primary destinations; 390px fallback through 1920px; 900×600
supported minimum; scan counts and source states; reduced motion; explicit
confirmation, cancel, Escape and focus return; quarantine metadata and undo;
companion pause persistence, live reduced-motion changes, cleanup state and
result fidelity; partial, denied, unavailable and failed collectors; all-clear/no findings;
long hostile names/paths; cleanup, Canary and login observation; axe WCAG AA;
keyboard focus; no remote requests; pixel detail and restrained accent use.

`baseline-guard.mjs` and `quarantine-regression.mjs` retain their real CLI
tests in isolated temporary directories. They require a current release CLI.
The real-desktop overlay fixture is opt-in under `testing/` and is not run by
the mock suite. A human screen-reader pass remains part of release validation.

Native test drivers require a separate GUI build with `--features desktop-e2e`.
Normal release builds exclude those environment-triggered drivers and embed
only the production asset allowlist. Build distributables with
`tools/build-release.ps1` to remap the builder's local source paths; run
`python tools/audit-candidate.py <candidate-directory>` after packaging.

Public assets use labelled mock data:

```powershell
node tools/docs-capture/capture.mjs
node tools/docs-capture/demo.mjs
ffmpeg -y -ss 0.75 -i docs/media/demo.webm -an -c:v libx264 -crf 20 -pix_fmt yuv420p docs/media/demo.mp4
ffmpeg -y -ss 0.75 -i docs/media/demo.webm -vf "fps=8,scale=1100:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" docs/media/demo.gif
```

Screenshots fix time/timezone, disable entrance motion, and wait on actual
mock events. The sweep layout has no randomness. Local outputs are ignored;
the nine deliberate documentation screenshots are committed under `docs/`.

The current recording has a 0.75-second browser initialization lead-in; the
encoding commands trim it so the public loop opens on the loaded Overview.
Recheck the first frame after recapture rather than including blank frames.
