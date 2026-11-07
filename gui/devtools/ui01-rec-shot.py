from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[2]
URL = (ROOT / "gui/dist/index.dev.html").as_uri()

with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_context(viewport={"width": 1600, "height": 900}).new_page()
    pg.goto(URL)
    pg.wait_for_load_state("networkidle")
    expect(pg.locator("#ov-rec-panel")).to_be_visible(timeout=10000)
    pg.locator("#ov-rec-panel").scroll_into_view_if_needed()
    pg.wait_for_timeout(400)
    pg.screenshot(path=str(ROOT / "testing/native/screenshots-ui01/05-recommendation-dark.png"))
    print(pg.locator("#ov-rec-panel").inner_text())
    b.close()
print("ok")
