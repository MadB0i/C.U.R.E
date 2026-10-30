from playwright.sync_api import sync_playwright
from pathlib import Path
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import threading
from functools import partial

ROOT = Path("D:/Projects/CURE")
DIST = ROOT / "gui/dist"
SHOTS = ROOT / "gui/dev-screenshots"
SHOTS.mkdir(parents=True, exist_ok=True)

class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_): pass
    def end_headers(self):
        self.send_header("Cache-Control", "no-store, max-age=0")
        super().end_headers()

server = ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=str(DIST)))
threading.Thread(target=server.serve_forever, daemon=True).start()
url = f'http://127.0.0.1:{server.server_port}/index.dev.html'

with sync_playwright() as p:
    browser = p.chromium.launch()
    ctx = browser.new_context(viewport={'width': 1440, 'height': 900})
    page = ctx.new_page()
    page.goto(url)
    page.wait_for_selector("#view-overview")
    page.wait_for_timeout(400)
    
    # Capture LUMA in each state on overview
    luma_box = page.locator('[data-companion="overview"]')
    states = [
        ("idle", "window.Luma.idle('Ready and watching')"),
        ("scanning", "window.Luma.scanning('Scanning running processes…')"),
        ("excited", "window.Luma.excited('3 agents discovered!')"),
        ("curious", "window.Luma.curious('Monitoring workspace metadata')"),
        ("warning", "window.Luma.warning('Access denied to process token')"),
        ("success", "window.Luma.success('Case file saved safely')"),
        ("sleepy", "window.Luma.sleepy('Resting between scans')"),
    ]
    for name, script in states:
        page.evaluate(script)
        page.wait_for_timeout(400)
        luma_box.screenshot(path=str(SHOTS / f"luma-state-{name}.png"))
        print(f"Captured luma-state-{name}.png")
        
    browser.close()

server.shutdown()
server.server_close()
print("Done capturing LUMA states successfully")
