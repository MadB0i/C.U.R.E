"""Regenerate gui/dist/index.dev.html from index.html.

The dev shell is the production shell plus two labelled differences: the
mock backend and the sample-data wordmark. Generating it removes a whole class
of drift bug; `evidence-console.py` asserts the two files stay in sync.
"""
import io
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "gui/dist/index.html"
DST = ROOT / "gui/dist/index.dev.html"

DEV_WORDMARK = '<div class="wordmark">C.U.R.E<span>Evidence Console · sample data</span></div>'
PROD_WORDMARK = '<div class="wordmark">C.U.R.E<span>Evidence Console</span></div>'
MOCK = '<script src="mock-tauri.js"></script>'


def build(html: str) -> str:
    if PROD_WORDMARK not in html:
        raise SystemExit("index.html wordmark marker not found")
    html = html.replace(PROD_WORDMARK, DEV_WORDMARK)
    if MOCK in html:
        raise SystemExit("index.html must never load the mock backend")
    marker = '<script src="luma.js"></script>'
    if marker not in html:
        raise SystemExit("index.html script marker not found")
    html = html.replace(
        marker,
        "<!-- Dev harness only: deterministic labelled mock backend. Never shipped. -->\n    "
        + MOCK
        + "\n    "
        + marker,
        1,
    )
    return html


def main() -> None:
    produced = build(io.open(SRC, encoding="utf-8").read())
    if not DST.exists() or io.open(DST, encoding="utf-8").read() != produced:
        io.open(DST, "w", encoding="utf-8", newline="").write(produced)
        print("regenerated index.dev.html")
    else:
        print("index.dev.html already in sync")


if __name__ == "__main__":
    main()