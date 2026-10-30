"""Screenshot capture for the evidence-level UI states (TASK 1B follow-up).

Runs the LABELLED MOCK backend (gui/dist-demo/index.html) in headless
Chromium — no real scan, no host files touched. Captures:

  01 findings cards with the evidence badge + evidence text
  02 evidence drawer with a level per line
  03 process table (score chips + evidence column)
  04 copy-evidence output (drawer shot + the exact copied text as .txt)
  05/06 the "Legacy report: evidence not recorded" state (card + drawer)
  07 findings cards at 1920x1080

Output: docs/screenshots/evidence/
Run: python gui/devtools/evidence-state-shots.py
"""
from pathlib import Path
import json
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[2]
URL = (ROOT / "gui/dist-demo/index.html").as_uri()
OUT = ROOT / "docs/screenshots/evidence"
OUT.mkdir(parents=True, exist_ok=True)

CLIPBOARD_HOOK = """
window.__lastCopy = null;
if (navigator.clipboard && navigator.clipboard.writeText) {
  const orig = navigator.clipboard.writeText.bind(navigator.clipboard);
  navigator.clipboard.writeText = (text) => {
    window.__lastCopy = String(text);
    return orig(text).catch(() => {});
  };
}
"""


def open_page(browser, size=(1366, 768), **knobs):
    context = browser.new_context(
        viewport={"width": size[0], "height": size[1]},
        reduced_motion="reduce",
        timezone_id="Asia/Kolkata",
        locale="en-GB",
    )
    context.grant_permissions(["clipboard-read", "clipboard-write"])
    page = context.new_page()
    init = {"__CURE_MOCK_OVERLAY_HITS": 0, **{k: v for k, v in knobs.items()}}
    page.add_init_script("Object.assign(window," + json.dumps(init) + ")")
    page.add_init_script(CLIPBOARD_HOOK)
    page.goto(URL)
    page.wait_for_load_state("networkidle")
    expect(page.locator("#view-overview")).to_be_visible()
    return context, page


def run_scan(page):
    page.locator("#nav-scan").click()
    page.get_by_role("button", name="Start Rescue Scan", exact=True).click()
    expect(page.locator("#results-view")).to_be_visible(timeout=30000)
    expect(page.locator(".review-card .finding-reasons").first).not_to_have_text("", timeout=15000)


def main():
    with sync_playwright() as p:
        browser = p.chromium.launch()

        # 01 — findings cards with evidence badge and text (1366x768)
        ctx, page = open_page(browser)
        run_scan(page)
        page.screenshot(path=str(OUT / "01-findings-cards-evidence.png"))

        # 02 — evidence drawer, one level per line
        page.locator(".review-card .finding-select").first.click()
        expect(page.locator("#evidence-inspector")).to_be_visible()
        expect(page.locator("#evidence-body li").first).to_contain_text("[Observed]")
        page.screenshot(path=str(OUT / "02-evidence-drawer-levels.png"))

        # 04 — the exact "Copy evidence" output (hooked clipboard)
        page.locator("#evidence-body button", has_text="Copy evidence").click()
        expect(page.locator("#evidence-body button", has_text="Copied")).to_be_visible()
        copied = page.evaluate("window.__lastCopy")
        assert copied and "Observed:" in copied, f"copy output lost levels: {copied!r}"
        (OUT / "04-copy-evidence-output.txt").write_text(copied, encoding="utf-8")
        page.locator("#evidence-close").click()

        # 03 — process table (sweep mock), score chips + evidence column
        ctx, page = open_page(browser, __CURE_MOCK_SWEEP=True)
        run_scan(page)
        page.locator("#nav-processes").click()
        expect(page.locator("#proc-rows tr").first).to_be_visible(timeout=15000)
        page.screenshot(path=str(OUT / "03-process-table-evidence.png"))
        ctx.close()

        # 05/06 — legacy state: empty evidence list must be explicit
        ctx, page = open_page(browser, __CURE_MOCK_LEGACY_EVIDENCE=True)
        run_scan(page)
        legacy = page.locator(".review-card .finding-reasons").first
        expect(legacy).to_contain_text("Legacy report: evidence not recorded")
        page.screenshot(path=str(OUT / "05-legacy-state-card.png"))

        page.locator(".review-card .finding-select").first.click()
        expect(page.locator("#evidence-inspector")).to_be_visible()
        expect(page.locator("#evidence-body")).to_contain_text("Legacy report: evidence not recorded")
        page.screenshot(path=str(OUT / "06-legacy-state-drawer.png"))
        ctx.close()

        # 07 — findings cards at 1920x1080
        ctx, page = open_page(browser, size=(1920, 1080))
        run_scan(page)
        page.screenshot(path=str(OUT / "07-findings-cards-1920.png"))
        ctx.close()

        browser.close()
        print("captured:", sorted(f.name for f in OUT.iterdir()))


if __name__ == "__main__":
    main()
