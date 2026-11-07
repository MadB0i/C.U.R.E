from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[2]
URL = (ROOT / "gui/dist/index.dev.html").as_uri()

with sync_playwright() as p:
    b = p.chromium.launch()
    ctx = b.new_context(viewport={"width": 1366, "height": 768})
    page = ctx.new_page()
    page.goto(URL)
    page.wait_for_load_state("networkidle")
    page.locator("#nav-agents").click()
    page.wait_for_timeout(300)
    page.locator("#ar-scan-now").click()
    expect(page.locator("#ar-detected-count")).to_have_text("1", timeout=15000)
    page.locator("#ar-workspace").fill("C:/Projects/cure-demo-app")
    page.locator("#ar-start").click()
    expect(page.locator("#ar-session-badge")).to_have_text("Recording", timeout=10000)
    page.wait_for_timeout(500)
    page.screenshot(path=str(ROOT / "testing/native/screenshots-ui02/07-agents-1366.png"))
    print("ok")
    b.close()
