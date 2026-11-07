#!/usr/bin/env python3
"""Capture active scanning and completed scan results with Holographic Security Radar."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path
import psutil
from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[1]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
SHOTS_DIR = REPO / "testing" / "native" / "screenshots-audit" / "ui-review"
SHOTS_DIR.mkdir(parents=True, exist_ok=True)


def kill_lingering():
    for proc in psutil.process_iter(["pid", "name"]):
        try:
            name = (proc.info["name"] or "").lower()
            if "msedgewebview2" in name or "cure-gui" in name:
                proc.kill()
        except Exception:
            pass
    time.sleep(1.0)


def wait_port(port: int, timeout: float = 60.0):
    start = time.time()
    url = f"http://127.0.0.1:{port}/json/version"
    while time.time() - start < timeout:
        try:
            with urllib.request.urlopen(url, timeout=1.0) as resp:
                if resp.status == 200:
                    return
        except Exception:
            pass
        time.sleep(0.5)
    raise TimeoutError(f"CDP port {port} not ready")


def capture_scan_views():
    kill_lingering()
    temp_dir = Path(tempfile.mkdtemp(prefix="cure-scan-audit-"))
    webview_data = temp_dir / "webview"
    webview_data.mkdir(parents=True, exist_ok=True)
    port = 9449

    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(port)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(webview_data)

    proc = subprocess.Popen(
        [str(EXE)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    try:
        wait_port(port)
        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
            context = browser.contexts[0]
            page = context.pages[0]
            page.set_viewport_size({"width": 1440, "height": 900})
            page.wait_for_url("http://tauri.localhost/", timeout=30000)
            page.wait_for_load_state("networkidle")
            time.sleep(1.5)

            def set_theme(theme: str):
                page.evaluate(f"""(theme) => {{
                    document.documentElement.setAttribute('data-theme', theme);
                    const btn = document.getElementById('theme-toggle');
                    if (btn) btn.setAttribute('aria-pressed', theme === 'light' ? 'true' : 'false');
                }}""", theme)
                time.sleep(0.3)

            set_theme("dark")

            # Click Overview -> Start Rescue Scan
            print("Triggering Rescue Scan...")
            page.click("#overview-rescue-btn")
            time.sleep(1.2)

            # Check if overlay review prompt appeared; if so, click continue
            page.evaluate("""() => {
                const cont = document.querySelector('.overlay-continue, #log button');
                if (cont) cont.click();
            }""")
            time.sleep(1.0)

            # Capture scanning in progress (Holographic Security Radar)
            page.screenshot(path=str(SHOTS_DIR / "12-scan-in-progress-dark.png"))
            print("Captured scanning in progress (dark)")

            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "12-scan-in-progress-light.png"))
            print("Captured scanning in progress (light)")

            set_theme("dark")

            # Wait for scan to finish (up to 30s)
            for _ in range(40):
                is_done = page.evaluate("() => document.getElementById('scan-view').classList.contains('hidden') || !window.CureSweep.getState().running")
                if is_done:
                    break
                time.sleep(0.5)

            time.sleep(1.5)
            # Switch to results view if not already there
            page.click("#nav-results")
            time.sleep(0.8)
            page.screenshot(path=str(SHOTS_DIR / "13-scan-results-dark.png"))
            print("Captured scan results (dark)")

            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "13-scan-results-light.png"))
            print("Captured scan results (light)")

            # Go to Overview to see completed posture dial
            set_theme("dark")
            page.click("#nav-overview")
            time.sleep(0.8)
            page.screenshot(path=str(SHOTS_DIR / "14-overview-post-scan-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "14-overview-post-scan-light.png"))
            print("Captured overview post-scan")

            browser.close()

    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except Exception:
                proc.kill()
        kill_lingering()
        shutil.rmtree(temp_dir, ignore_errors=True)
    print("ALL SCAN CAPTURES COMPLETE!")


if __name__ == "__main__":
    capture_scan_views()
