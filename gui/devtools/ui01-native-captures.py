"""Stage UI-01 native acceptance captures from the real Tauri binary.

Launches cure-gui.exe with the DevTools protocol enabled, drives the real
backend (a real rescue scan — read-only), and captures the rebuilt Overview
in both themes, before and after a scan. No mocks.

Run: python gui/devtools/ui01-native-captures.py
"""
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from playwright.sync_api import sync_playwright, expect

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
OUT = REPO / "testing" / "native" / "screenshots-ui01" / "native"
OUT.mkdir(parents=True, exist_ok=True)
PORT = 9402


def wait_cdp(timeout=60):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=1)
            return True
        except Exception:
            time.sleep(0.5)
    raise SystemExit("CDP endpoint never appeared")


def shot(page, name):
    page.screenshot(path=str(OUT / name))
    print("captured", name)


def main():
    tmp = tempfile.TemporaryDirectory(prefix="cure-ui01-", ignore_cleanup_errors=True)
    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(PORT)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(Path(tmp.name) / "wvdata")
    print("launching", EXE)
    proc = subprocess.Popen([str(EXE)], env=env)
    try:
        wait_cdp()
        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
            page = browser.contexts[0].pages[0]
            page.set_viewport_size({"width": 1600, "height": 900})
            page.wait_for_load_state("networkidle")
            expect(page.locator("#view-overview")).to_be_visible()

            # Fresh launch: neutral session state, live PC clock, no header
            # scan summary, no fabricated results.
            assert page.locator("#ov-posture").inner_text() == "Not assessed"
            assert page.locator("#session-meta").count() == 0
            clock = page.locator("#topbar-clock").inner_text()
            print("native clock:", clock)
            assert ":" in clock and any(c.isdigit() for c in clock)
            page.wait_for_timeout(800)
            shot(page, "01-native-fresh-launch-dark.png")

            # Recommendation module is driven by the real operator_status.
            expect(page.locator("#ov-rec-panel")).to_be_visible(timeout=15000)
            page.locator("#ov-rec-panel").scroll_into_view_if_needed()
            page.wait_for_timeout(300)
            shot(page, "02-native-recommendation-dark.png")
            page.locator("#nav-overview").click()

            # Real rescue scan through the hero's primary action.
            page.locator("#overview-rescue-btn").click()
            expect(page.locator("#headline")).to_be_visible(timeout=180000)
            page.locator("#nav-overview").click()
            posture = page.locator("#ov-posture").inner_text()
            print("native post-scan posture:", posture)
            assert posture != "Not assessed"
            shot(page, "03-native-post-scan-dark.png")

            # Light theme.
            page.locator("#theme-toggle").click()
            page.wait_for_timeout(400)
            shot(page, "04-native-post-scan-light.png")
            browser.close()
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except Exception:
            proc.kill()
        tmp.cleanup()
    print("done ->", OUT)


if __name__ == "__main__":
    sys.exit(main())
