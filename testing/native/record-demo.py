"""Record the official C.U.R.E demo video (MVP2 TASK 8 media).

Records a genuine desktop capture (ffmpeg gdigrab, the real screen) of the
production interface running against the labelled deterministic mock
backend — exactly the demo flow MVP2 prescribes: the labelled shell makes
the recording safe to publish, with no real machine's paths on screen.

Journey shown (~50 s):
  Overview -> System X-Ray scan (live ten-layer radar) -> Investigation
  Console lenses -> Monitor -> Storage Rescue with the Move Bot doing a
  one-click C: -> D: relocation -> the Share result card.

Outputs (after transcode, done by tools/make-demo-media.py):
  docs/media/cure-demo-raw.mp4

Run: python testing/native/record-demo.py
"""
from __future__ import annotations

import ctypes
import ctypes.wintypes
import functools
import http.server
import os
import socketserver
import subprocess
import sys
import threading
import time
from ctypes import wintypes
from pathlib import Path

from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[2]
DIST = REPO / "gui" / "dist"
OUT = REPO / "docs" / "media"
OUT.mkdir(parents=True, exist_ok=True)
PORT = 9470
FFMPEG = os.environ.get("FFMPEG", "ffmpeg")

sys.path.insert(0, str(REPO / "testing" / "native"))
import record  # noqa: E402  (capture helpers; main guarded)

user32 = record.user32


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(DIST), **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args):
        pass


def screen_size():
    record.become_dpi_aware()
    return user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)


def wait_visible(page, selector, timeout_s=20):
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        if page.locator(selector).first.is_visible():
            return True
        time.sleep(0.15)
    return False


def hwnd_by_title(fragment: str, timeout_s: float = 30.0) -> int:
    """The top-level window whose title contains `fragment` (the browser
    window carries the page title)."""
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        found: list[int] = []

        def callback(hwnd, _lparam):
            if user32.IsWindowVisible(hwnd):
                title = ctypes.create_unicode_buffer(256)
                user32.GetWindowTextW(hwnd, title, 256)
                if fragment.lower() in title.value.lower():
                    found.append(hwnd)
                    return False
            return True

        user32.EnumWindows(
            ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.wintypes.HWND, ctypes.wintypes.LPARAM)(
                callback
            ),
            0,
        )
        if found:
            return found[0]
        time.sleep(0.4)
    raise RuntimeError(f"window with title fragment {fragment!r} never appeared")


def main() -> int:
    if not record.ffmpeg_available():
        print("ffmpeg not available")
        return 1
    w, h = screen_size()
    print(f"screen: {w}x{h}")

    socketserver.ThreadingTCPServer.allow_reuse_address = True
    httpd = socketserver.ThreadingTCPServer(("127.0.0.1", PORT), Handler)
    httpd.daemon_threads = True
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    with sync_playwright() as p:
        browser = p.chromium.launch(
            headless=False, args=["--start-maximized", "--window-position=0,0"]
        )
        context = browser.new_context(no_viewport=True)
        page = context.new_page()
        page.goto(f"http://127.0.0.1:{PORT}/index.dev.html")
        page.wait_for_selector("#app:not(.hidden)", timeout=15000)
        page.wait_for_timeout(1200)

        pid = 0  # window found by title; isolation skips only this hwnd
        hwnd = hwnd_by_title("Rescue Console")
        record.maximize(hwnd)
        record.ensure_foreground(hwnd)
        hidden = record.isolate(hwnd, pid)
        time.sleep(0.8)

        raw = OUT / "cure-demo-raw.mp4"
        capture = subprocess.Popen(
            [
                FFMPEG,
                "-y",
                "-f",
                "gdigrab",
                "-framerate",
                "30",
                "-offset_x",
                "0",
                "-offset_y",
                "0",
                "-video_size",
                f"{w}x{h}",
                "-i",
                "desktop",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "23",
                "-pix_fmt",
                "yuv420p",
                str(raw),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            # 1. Overview, idle.
            time.sleep(4.0)

            # 2. System X-Ray scan, live radar.
            page.locator("#nav-scan").click()
            time.sleep(1.6)
            page.locator("#xray-start-btn").click()
            page.wait_for_timeout(9000)  # staged collector events

            # 3. Investigation Console: findings then coverage.
            page.locator("#nav-results").click()
            page.wait_for_timeout(3200)
            page.locator("#lens-btn-coverage").click()
            page.wait_for_timeout(2400)
            page.locator("#lens-btn-ransom").click()
            page.wait_for_timeout(2200)

            # 4. Monitor workstation feed.
            page.locator("#nav-eventlog").click()
            page.wait_for_timeout(2400)

            # 5. Move Bot: one-click C: -> D: (labelled mock data).
            page.locator("#nav-cleanup").click()
            page.wait_for_timeout(600)
            page.locator("#sw-tab-rescue").click()
            page.wait_for_timeout(900)
            page.locator("#sr-btn-move-bot").click()
            page.wait_for_timeout(1800)
            page.locator("#mb-cleanup-tick").check()
            page.wait_for_timeout(900)
            page.locator("#mb-confirm-go").click()
            page.wait_for_timeout(6500)  # copy -> switch -> free -> done card
            page.locator("#mb-done-finish").click()
            page.wait_for_timeout(900)

            # 6. Share result card preview.
            page.locator("#nav-overview").click()
            page.wait_for_timeout(700)
            page.locator("#ov-share-btn").click()
            page.wait_for_timeout(3200)
            page.keyboard.press("Escape")
            page.wait_for_timeout(1200)
        finally:
            try:
                capture.stdin.write(b"q")
                capture.stdin.flush()
            except Exception:
                pass
            try:
                capture.wait(timeout=20)
            except Exception:
                capture.kill()
            try:
                record.restore(hidden)
            except Exception:
                pass
            browser.close()
        httpd.shutdown()
        httpd.server_close()

    size = raw.stat().st_size if raw.exists() else 0
    print(f"raw capture: {raw} ({size} bytes)")
    return 0 if size > 100000 else 1


if __name__ == "__main__":
    raise SystemExit(main())
