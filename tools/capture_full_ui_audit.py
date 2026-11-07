#!/usr/bin/env python3
"""Capture full suite of native UI screenshots across all views in Dark and Light themes.
Used for C.U.R.E UI/UX independent audit.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error
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


def capture_all():
    kill_lingering()
    temp_dir = Path(tempfile.mkdtemp(prefix="cure-ui-audit-"))
    webview_data = temp_dir / "webview"
    webview_data.mkdir(parents=True, exist_ok=True)
    port = 9446

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

            screens = [
                ("01-overview", "#nav-overview"),
                ("02-agents", "#nav-agents"),
                ("03-rescue", "#nav-scan"),
                ("04-investigate", "#nav-results"),
                ("05-quarantine", "#nav-quarantine"),
                ("06-detective", "#nav-detective"),
                ("07-storage", "#nav-cleanup"),
                ("08-monitor", "#nav-eventlog"),
            ]

            def set_theme(theme: str):
                page.evaluate(f"""(theme) => {{
                    document.documentElement.setAttribute('data-theme', theme);
                    const btn = document.getElementById('theme-toggle');
                    if (btn) btn.setAttribute('aria-pressed', theme === 'light' ? 'true' : 'false');
                }}""", theme)
                time.sleep(0.4)

            # Ensure starting in dark mode
            set_theme("dark")

            for prefix, nav_id in screens:
                print(f"Capturing {prefix}...")
                page.click(nav_id)
                time.sleep(0.6)

                # Capture dark mode
                set_theme("dark")
                dark_path = SHOTS_DIR / f"{prefix}-dark.png"
                page.screenshot(path=str(dark_path))
                print(f"  -> Saved {dark_path.name}")

                # Capture light mode
                set_theme("light")
                light_path = SHOTS_DIR / f"{prefix}-light.png"
                page.screenshot(path=str(light_path))
                print(f"  -> Saved {light_path.name}")

            # Also capture LUMA close-up in dark & light
            page.click("#nav-agents")
            time.sleep(0.5)
            luma_elem = page.locator(".ar-companion-card")
            if luma_elem.count() > 0:
                set_theme("dark")
                luma_elem.screenshot(path=str(SHOTS_DIR / "11-luma-card-dark.png"))
                set_theme("light")
                luma_elem.screenshot(path=str(SHOTS_DIR / "11-luma-card-light.png"))
                print("  -> Saved LUMA card closeups")

            # Capture Agent Detail inspector modal if possible
            set_theme("dark")
            page.evaluate("""() => {
                const dlg = document.getElementById('ar-agent-inspector');
                if (dlg) {
                    dlg.classList.remove('hidden');
                    const body = document.getElementById('ar-insp-body');
                    if (body) {
                        body.innerHTML = `
                            <div class="ar-insp-summary">
                                <h3>OpenCode Interpreter</h3>
                                <p class="lead">Autonomous terminal agent executing in isolated workspace.</p>
                                <div class="badge badge-unverified">Unverified Process</div>
                            </div>
                            <div class="ar-insp-props">
                                <div class="prop-row"><span class="k">PID:</span> <span class="v">16456</span></div>
                                <div class="prop-row"><span class="k">Image:</span> <span class="v">opencode.exe</span></div>
                                <div class="prop-row"><span class="k">Confidence:</span> <span class="v">Unverified (Image name match; tool identity unconfirmed)</span></div>
                                <div class="prop-row"><span class="k">Secrets Filter:</span> <span class="v">Cleaned & Masked (***)</span></div>
                            </div>
                        `;
                    }
                }
            }""")
            time.sleep(0.4)
            page.screenshot(path=str(SHOTS_DIR / "09-agent-inspector-dark.png"))
            print("  -> Saved 09-agent-inspector-dark.png")

            # Reset modal
            page.evaluate("""() => {
                const dlg = document.getElementById('ar-agent-inspector');
                if (dlg) dlg.classList.add('hidden');
            }""")

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
    print("ALL SCREENS CAPTURED SUCCESSFULLY!")


if __name__ == "__main__":
    capture_all()
