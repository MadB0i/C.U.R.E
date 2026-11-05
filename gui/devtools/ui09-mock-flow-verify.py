"""Stage UI-09 — full Move Bot journey in the labelled mock shell.

The native harness (ui09-native-verify.py) proves the bot against the REAL
backend up to the confirmation gate — executing the move natively would
relocate the real machine's personal folders. This script completes the
FULL journey (copy → verify → switch over → free C:) in the labelled
deterministic mock shell, which drives the exact same commands and event
shapes the real engine uses:

  1. Bot opens, real-shaped mock candidates appear
  2. Operator ticks "also free the space on C:" and presses one button
  3. Bot copies (live progress), switches Windows over, frees C:
  4. Done card reports moved + freed bytes in plain words
  5. Escape during a running move is refused (never abandon a move)
  6. Failure path: engine error surfaces in plain words with originals safe

Run: python gui/devtools/ui09-mock-flow-verify.py
"""
import http.server
import functools
import socketserver
import threading
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[2]
DIST = REPO / "gui" / "dist"
PORT = 9461

results = []


def check(name, ok, extra=""):
    results.append((name, ok, extra))
    status = "PASS" if ok else "FAIL"
    print(f"[{status}] {name}" + (f" ({extra})" if extra else ""))


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(DIST), **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args):
        pass


def main():
    socketserver.ThreadingTCPServer.allow_reuse_address = True
    httpd = socketserver.ThreadingTCPServer(("127.0.0.1", PORT), Handler)
    httpd.daemon_threads = True
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()

    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page(viewport={"width": 1600, "height": 960})
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(f"http://127.0.0.1:{PORT}/index.dev.html")
        page.wait_for_selector("#app:not(.hidden)", timeout=15000)
        page.wait_for_timeout(800)

        # 1. Open the bot from the Rescue action bar.
        page.locator("#nav-cleanup").click()
        page.locator("#sw-tab-rescue").click()
        page.wait_for_timeout(500)
        page.wait_for_timeout(500)
        page.locator("#sr-btn-move-bot").click()
        page.wait_for_timeout(400)
        overlay = page.locator("#mb-overlay")
        check("bot overlay opens in the mock shell", overlay.is_visible())

        deadline = 30
        waited = 0
        while waited < deadline and not page.locator("#mb-confirm-go").is_enabled():
            page.wait_for_timeout(300)
            waited += 0.3
        check("confirmation card is ready", page.locator("#mb-confirm-go").is_enabled())
        items = page.locator("#mb-confirm-items .mb-item-row")
        check("3 safe folders listed", items.count() == 3, f"count={items.count()}")
        names = " ".join(items.all_text_contents())
        check("Downloads / Videos / Pictures listed", all(n in names for n in ("Downloads", "Videos", "Pictures")))

        # 2. One click — with "also free the space" ticked.
        page.locator("#mb-cleanup-tick").check()
        page.locator("#mb-confirm-go").click()
        page.wait_for_timeout(300)
        check("running view shows the step timeline", page.locator("#mb-run-body").is_visible())
        check(
            "Escape is refused while the bot is working",
            not page.evaluate(
                "() => { document.dispatchEvent(new KeyboardEvent('keydown', {key:'Escape'}));"
                " return document.getElementById('mb-overlay').classList.contains('hidden'); }"
            ),
        )

        # 3. Wait for the done card (mock: 3 candidates x ~1.6 s).
        deadline = 60
        waited = 0
        done_visible = False
        while waited < deadline:
            if page.locator("#mb-done-body").is_visible():
                done_visible = True
                break
            page.wait_for_timeout(500)
            waited += 0.5
        check("bot reaches the Done card", done_visible)

        done_text = page.locator("#mb-done-body").text_content() or ""
        check(
            "done card reports moved + freed bytes in plain words",
            "All moved and verified" in done_text and "freed" in done_text,
            done_text.strip()[:110],
        )
        rows = page.locator("#mb-done-detail .mb-item-row")
        check("done card lists every moved folder", rows.count() == 3, f"count={rows.count()}")
        detail_text = page.locator("#mb-done-detail").text_content() or ""
        check(
            "targets live on D:",
            detail_text.count("D:") >= 3,
        )
        page.screenshot(path=str(REPO / "testing" / "native" / "screenshots-ui09" / "mock-flow-done.png"))

        page.locator("#mb-done-finish").click()
        page.wait_for_timeout(400)
        check("finish closes the bot", overlay.is_hidden())

        # 4. Failure path: make the mock refuse the first copy and watch
        #    the bot stop safely in plain words.
        page.evaluate(
            "() => { window.__CURE_MOCK_FAIL_RESCUE = 'simulated disk error'; }"
        )
        page.locator("#sr-btn-move-bot").click()
        page.wait_for_timeout(500)
        waited = 0
        while waited < 20 and not page.locator("#mb-confirm-go").is_enabled():
            page.wait_for_timeout(300)
            waited += 0.3
        page.locator("#mb-confirm-go").click()
        deadline = 30
        waited = 0
        err_visible = False
        while waited < deadline:
            if page.locator("#mb-error-body").is_visible():
                err_visible = True
                break
            page.wait_for_timeout(400)
            waited += 0.4
        check("engine failure stops the bot safely", err_visible)
        err_text = page.locator("#mb-error-body").text_content() or ""
        check(
            "failure message is honest and human",
            "stopped" in err_text.lower() and "safe" in err_text.lower(),
            err_text.strip()[:110],
        )
        page.locator("#mb-error-close").click()
        page.wait_for_timeout(300)
        check("error card closes", overlay.is_hidden())

        check("no page errors during the whole journey", not errors, str(errors[:2]))
        browser.close()

    httpd.shutdown()
    httpd.server_close()

    passes = sum(1 for _, ok, _ in results if ok)
    fails = sum(1 for _, ok, _ in results if not ok)
    total = len(results)
    print(f"\n{passes}/{total} mock-flow UI-09 checks passed")
    if fails > 0:
        print("UI-09 MOCK-FLOW ACCEPTANCE: FAILED")
        sys.exit(1)
    print("UI-09 MOCK-FLOW ACCEPTANCE: ALL CHECKS PASSED")


if __name__ == "__main__":
    main()
