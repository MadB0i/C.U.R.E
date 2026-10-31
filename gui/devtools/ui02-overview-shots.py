"""Stage UI-02 screenshot capture against the labelled dev shell (mock data).

Captures the rebuilt Agent Radar: fresh idle state, scan in progress,
detected agents with an expanded row, an active recording, the timeline
with a selected observation, and the light theme.

Run: python gui/devtools/ui02-overview-shots.py
"""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[2]
URL = (ROOT / "gui/dist/index.dev.html").as_uri()
OUT = ROOT / "testing/native/screenshots-ui02"
OUT.mkdir(parents=True, exist_ok=True)


def shot(page, name):
    page.screenshot(path=str(OUT / name))
    print("captured", name)


def main():
    with sync_playwright() as p:
        browser = p.chromium.launch()
        ctx = browser.new_context(viewport={"width": 1600, "height": 900})
        page = ctx.new_page()
        page.goto(URL)
        page.wait_for_load_state("networkidle")
        page.locator("#nav-agents").click()
        page.wait_for_timeout(400)

        # 1. Fresh idle state.
        assert page.locator("#ar-radar").get_attribute("data-phase") == "idle"
        shot(page, "01-agents-fresh-idle.png")

        # 2. Scan in progress: the sweep runs while the mock scan resolves.
        page.locator("#ar-scan-now").click()
        page.wait_for_timeout(60)
        phase = page.locator("#ar-radar").get_attribute("data-phase")
        print("phase during scan:", phase)
        shot(page, "02-agents-scanning.png")
        expect(page.locator("#ar-detected-count")).to_have_text("1", timeout=15000)

        # 3. Detected agents with expanded row + session intel.
        page.locator("#ar-agent-list .ar-agent-card .ar-agent-row").first.click()
        expect(page.locator("#ar-agent-list .ar-agent-body").first).to_be_visible()
        page.wait_for_timeout(300)
        shot(page, "03-agents-detected.png")

        # 4. Recording: start monitoring on the demo workspace.
        page.locator("#ar-workspace").fill("C:/Projects/cure-demo-app")
        page.locator("#ar-start").click()
        expect(page.locator("#ar-session-badge")).to_have_text("Recording", timeout=10000)
        expect(page.locator("#ar-stat-total")).to_have_text("6")
        page.wait_for_timeout(400)
        shot(page, "04-recorder-active.png")

        # 5. Timeline + selected evidence.
        page.locator("#ar-event-list .ar-event").first.click()
        expect(page.locator("#ar-evidence-card")).to_be_visible()
        page.wait_for_timeout(300)
        shot(page, "05-evidence-selected.png")

        # 6. Light theme, stopped session.
        page.locator("#ar-stop").click()
        expect(page.locator("#ar-session-badge")).to_have_text("Stopped", timeout=10000)
        page.locator("#theme-toggle").click()
        page.wait_for_timeout(400)
        shot(page, "06-agents-light.png")

        ctx.close()
        browser.close()
    print("done ->", OUT)


if __name__ == "__main__":
    main()
