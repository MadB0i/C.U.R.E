"""Stage UI-01 screenshot capture against the labelled dev shell (mock data).

Captures the rebuilt Overview in dark/light themes, before/after a scan,
and at common Windows desktop sizes. All data is deterministic sample data
from mock-tauri.js; nothing on the host is read beyond this repository.

Run: python gui/devtools/ui01-overview-shots.py
"""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[2]
URL = (ROOT / "gui/dist/index.dev.html").as_uri()
OUT = ROOT / "testing/native/screenshots-ui01"
OUT.mkdir(parents=True, exist_ok=True)


def shot(page, name):
    page.screenshot(path=str(OUT / name), full_page=False)
    print("captured", name)


def capture(browser, width, height, suffix):
    ctx = browser.new_context(viewport={"width": width, "height": height})
    page = ctx.new_page()
    page.goto(URL)
    page.wait_for_load_state("networkidle")
    expect(page.locator("#view-overview")).to_be_visible()
    page.wait_for_timeout(600)

    # Fresh launch: neutral session, no stale results.
    assert page.locator("#ov-posture").inner_text() == "Not assessed"
    assert page.locator("#ov-review-btn").is_disabled()
    assert page.locator("#session-meta").count() == 0, "header must carry no scan summary"
    clock = page.locator("#topbar-clock").inner_text()
    assert any(ch.isdigit() for ch in clock) and ":" in clock, "clock must show live time: " + clock
    print("clock:", clock)
    shot(page, f"01-fresh-launch-{suffix}.png")

    # Recommendation module from the mock operator posture.
    expect(page.locator("#ov-rec-panel")).to_be_visible(timeout=10000)
    rec = page.locator("#ov-rec-panel").inner_text()
    assert "Storage" in rec, "storage recommendation should be visible in dev shell"

    # Run the scan through the hero's primary action.
    page.locator("#overview-rescue-btn").click()
    expect(page.locator("#headline")).to_be_visible(timeout=60000)
    page.locator("#nav-overview").click()
    expect(page.locator("#ov-posture")).not_to_have_text("Scanning")
    posture = page.locator("#ov-posture").inner_text()
    print("post-scan posture:", posture)
    assert posture in (
        "Assessment complete",
        "Attention required",
        "Attention required · Partial coverage",
        "Partial coverage",
    )
    assert not page.locator("#ov-review-btn").is_disabled()
    shot(page, f"02-post-scan-{suffix}.png")

    # Light theme.
    page.locator("#theme-toggle").click()
    page.wait_for_timeout(300)
    shot(page, f"03-post-scan-light-{suffix}.png")
    page.locator("#nav-overview").click()
    page.wait_for_timeout(300)
    shot(page, f"04-overview-light-{suffix}.png")

    ctx.close()


def main():
    with sync_playwright() as p:
        browser = p.chromium.launch()
        capture(browser, 1920, 1080, "1920")
        capture(browser, 1366, 768, "1366")
        browser.close()
    print("done ->", OUT)


if __name__ == "__main__":
    main()
