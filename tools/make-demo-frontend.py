"""Build the public demo frontend.

The demo is the **production interface** running against the
**deterministic labelled mock backend**. It contains no real machine's
paths, no real file names and no real detections, and it says so in three
places: the wordmark, the footer and the storage view's own heading.

The output is written to `gui/dist-demo/`, which is a separate frontend
bundle. A separate Tauri configuration builds it into its own binary, so
the shipped application and its asset allowlist are untouched.
"""
import io
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "gui" / "dist"
OUT = ROOT / "gui" / "dist-demo"

DEMO_WORDMARK = (
    '<div class="wordmark">C.U.R.E<span>Evidence Console · public demo</span></div>'
)
PROD_WORDMARK = '<div class="wordmark">C.U.R.E<span>Evidence Console</span></div>'

# The demo banner. It is deliberately impossible to mistake for a real
# finding: it is present for the whole recording.
BANNER = (
    '<div class="demo-banner" role="note">'
    "<strong>Public demo</strong> — every file, path and finding on this "
    "screen is deterministic sample data. Nothing here was read from a real "
    "machine, and nothing here is a threat detection."
    "</div>"
)

# A chip in the top bar, so every screen in the recording is marked.
CHIP = (
    '<span class="demo-chip" title="Deterministic sample data. '
    'Nothing here came from a real machine.">PUBLIC DEMO</span>'
)

# The storage view shows file names and folders, so it carries the long
# banner as well as the chip.
STORAGE_ANCHOR = (
    '<div class="results-stack">\n'
    '              <div class="results-head">\n'
    '                <div class="headline-wrap">\n'
    '                  <h1 class="headline">Storage Workstation</h1>'
)


def main() -> None:
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)

    html = io.open(DIST / "index.html", encoding="utf-8").read()
    if PROD_WORDMARK not in html:
        raise SystemExit("index.html wordmark marker not found")
    html = html.replace(PROD_WORDMARK, DEMO_WORDMARK)

    marker = '<script src="luma.js"></script>'
    if marker not in html:
        raise SystemExit("index.html script marker not found")
    # The demo replaces the Tauri API with the deterministic sample backend.
    # The flag has to be set before the mock is loaded, and it has to be
    # there: without it the mock stands aside when it finds the real API and
    # the demo quietly runs against the machine it is published from.
    html = html.replace(
        marker,
        '<!-- Public demo: deterministic labelled mock backend. Not shipped. -->\n'
        '    <script>window.__CURE_FORCE_MOCK = true;</script>\n'
        '    <script src="mock-tauri.js"></script>\n    ' + marker,
        1,
    )

    # A chip in the top bar, on every screen of the recording.
    chip_anchor = '<div class="session-meta" id="session-meta">'
    if chip_anchor not in html:
        raise SystemExit("index.html topbar anchor not found")
    html = html.replace(chip_anchor, CHIP + chip_anchor, 1)

    # The long banner goes into the findings view, above the view stack.
    shell_anchor = '<div class="results-stack">'
    if shell_anchor not in html:
        raise SystemExit("index.html shell anchor not found")
    html = html.replace(shell_anchor, BANNER + shell_anchor, 1)

    # And into the Overview, which is the first screen of any recording and
    # the one that would otherwise show a posture of "nothing found" as
    # though it were a statement about the machine.
    OVERVIEW_ANCHOR = (
        'id="view-overview"\n          >\n            <div class="results-stack">'
    )
    if OVERVIEW_ANCHOR not in html:
        raise SystemExit("index.html overview anchor not found")
    html = html.replace(
        OVERVIEW_ANCHOR,
        'id="view-overview"\n          >\n            ' + BANNER + "\n"
        '            <div class="results-stack">',
        1,
    )

    # And into the storage view, which is where names and folders appear.
    if STORAGE_ANCHOR not in html:
        raise SystemExit("index.html storage anchor not found")
    html = html.replace(
        STORAGE_ANCHOR,
        '<div class="results-stack">\n            ' + BANNER + "\n"
        '              <div class="results-head">\n'
        '                <div class="headline-wrap">\n'
        '                  <h1 class="headline">Storage Workstation</h1>',
        1,
    )

    # The detective board also names folders, so it carries the long banner.
    DETECTIVE_ANCHOR = (
        'id="view-detective"\n          >\n            <div class="dt-head">'
    )
    if DETECTIVE_ANCHOR not in html:
        raise SystemExit("index.html detective anchor not found")
    html = html.replace(
        DETECTIVE_ANCHOR,
        'id="view-detective"\n          >\n            ' + BANNER + "\n"
        '            <div class="dt-head">',
        1,
    )

    for name in (
        "style.css",
        "xray.css",
        "luma.css",
        "storage-workstation.css",
        "detective.css",
        "agent.css",
        "luma.js",
        "xray.js",
        "storage-workstation.js",
        "casefile.js",
        "detective.js",
        "agent-radar.js",
        "app.js",
        "mock-tauri.js",
    ):
        source = DIST / name
        if not source.exists():
            raise SystemExit("missing frontend file: " + name)
        shutil.copyfile(source, OUT / name)

    # The demo shell is written last, so the copy of the production shell
    # above cannot overwrite it.
    with io.open(OUT / "index.html", "w", encoding="utf-8", newline="") as handle:
        handle.write(html)

    css = io.open(OUT / "storage-workstation.css", encoding="utf-8").read()
    css += """
/* The public demo banner. It is the first thing on the screen and it is
   never hidden. */
.demo-banner {
  background: linear-gradient(
    90deg,
    rgba(224, 164, 88, 0.16),
    rgba(224, 164, 88, 0.05) 62%,
    transparent
  );
  border-left: 3px solid var(--warning);
  color: var(--text-2);
  font-size: var(--t-sm);
  line-height: 1.55;
  margin-bottom: var(--s5);
  padding: var(--s3) var(--s5);
}

.demo-banner strong {
  color: var(--warning);
  font-weight: 650;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
"""
    io.open(OUT / "storage-workstation.css", "w", encoding="utf-8", newline="\n").write(css)

    print("wrote", OUT)
    for name in sorted(p.name for p in OUT.iterdir()):
        print("  ", name)


if __name__ == "__main__":
    main()
