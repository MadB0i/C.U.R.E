#!/usr/bin/env python3
"""Native visual verification and screenshot capture for C.U.R.E Futuristic UI.
Connects directly to the native Windows Tauri desktop application (cure-gui.exe) via CDP.
"""

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


def run_visual_verification():
    kill_lingering()
    temp_dir = Path(tempfile.mkdtemp(prefix="cure-futuristic-verify-"))
    webview_data = temp_dir / "webview"
    webview_data.mkdir(parents=True, exist_ok=True)
    port = 9455

    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(port)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(webview_data)

    print(f"Launching native application: {EXE}")
    proc = subprocess.Popen(
        [str(EXE)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    try:
        wait_port(port)
        print("Connected to native Tauri CDP endpoint.")
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
                time.sleep(0.4)

            # -------------------------------------------------------------
            # 1. Overview with LUMA
            # -------------------------------------------------------------
            print("Capturing 01: Overview with LUMA...")
            page.click("#nav-overview")
            time.sleep(0.8)
            set_theme("dark")
            page.screenshot(path=str(SHOTS_DIR / "01-overview-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "01-overview-light.png"))

            # -------------------------------------------------------------
            # 2. Rescue Before Scan (Holographic Radar Standby)
            # -------------------------------------------------------------
            print("Capturing 03: Rescue Before Scan (Holographic Radar Standby)...")
            set_theme("dark")
            page.click("#nav-scan")
            time.sleep(0.8)
            page.screenshot(path=str(SHOTS_DIR / "03-rescue-before-scan-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "03-rescue-before-scan-light.png"))

            # -------------------------------------------------------------
            # 3. Rescue During Scan (Holographic Phosphor Sweep in Progress)
            # -------------------------------------------------------------
            print("Triggering Rescue Scan...")
            set_theme("dark")
            page.click("#xray-start-btn")
            time.sleep(1.0)
            # Dismiss overlay review if prompted
            page.evaluate("""() => {
                const cont = document.querySelector('.overlay-continue, #log button');
                if (cont) cont.click();
            }""")
            time.sleep(1.2)

            print("Capturing 03: Rescue During Scan...")
            page.screenshot(path=str(SHOTS_DIR / "03-rescue-during-scan-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "03-rescue-during-scan-light.png"))

            # -------------------------------------------------------------
            # 4. Wait for scan completion & capture Rescue After Scan
            # -------------------------------------------------------------
            print("Waiting for rescue scan to fully complete (up to 120s)...")
            set_theme("dark")
            scan_finished = False
            for i in range(120):
                is_done = page.evaluate("() => !window.CureSweep.getState().running && window.CureSweep.getState().items > 0")
                if is_done:
                    scan_finished = True
                    print(f"Rescue scan finished in ~{i}s.")
                    break
                time.sleep(1.0)

            if not scan_finished:
                print("Warning: scan timed out waiting for completion")

            time.sleep(2.0)
            # Return to scan-view to capture the post-scan debrief
            page.click("#nav-scan")
            time.sleep(1.0)
            print("Capturing 03: Rescue After Scan (Completed Debrief)...")
            page.screenshot(path=str(SHOTS_DIR / "03-rescue-after-scan-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "03-rescue-after-scan-light.png"))

            # -------------------------------------------------------------
            # 5. Investigate / Persistence Findings (Post-Scan)
            # -------------------------------------------------------------
            print("Capturing 04: Investigate / Findings...")
            set_theme("dark")
            page.click("#xray-investigate-btn")
            time.sleep(1.0)
            page.screenshot(path=str(SHOTS_DIR / "04-investigate-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "04-investigate-light.png"))

            # -------------------------------------------------------------
            # 6. AI Agent Radar V3
            # -------------------------------------------------------------
            print("Capturing 02: AI Agent Radar V3...")
            set_theme("dark")
            page.click("#nav-agents")
            time.sleep(1.0)
            # Click Scan Now to sample local processes
            page.click("#ar-scan-now")
            time.sleep(1.5)
            page.screenshot(path=str(SHOTS_DIR / "02-agents-radar-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "02-agents-radar-light.png"))

            # Capture Inspector dialog if an agent was detected
            has_agent = page.evaluate("() => document.querySelectorAll('#ar-agent-list .ar-agent-card').length > 0")
            if has_agent:
                print("Opening Agent Detail Inspector modal...")
                set_theme("dark")
                page.click("#ar-agent-list .ar-agent-btn-inspect")
                time.sleep(0.6)
                page.screenshot(path=str(SHOTS_DIR / "02-agent-inspector-dark.png"))
                page.click("#ar-insp-close")
                time.sleep(0.4)

            # -------------------------------------------------------------
            # 7. Quarantine Secure Vault Enclave Empty State
            # -------------------------------------------------------------
            print("Capturing 05: Quarantine Secure Vault Enclave...")
            set_theme("dark")
            page.click("#nav-quarantine")
            time.sleep(0.8)
            page.screenshot(path=str(SHOTS_DIR / "05-quarantine-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "05-quarantine-light.png"))

            # -------------------------------------------------------------
            # 8. Storage Workstation
            # -------------------------------------------------------------
            print("Capturing 07: Storage Workstation...")
            set_theme("dark")
            page.click("#nav-cleanup")
            time.sleep(0.8)
            page.screenshot(path=str(SHOTS_DIR / "07-storage-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "07-storage-light.png"))

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

    print("ALL NATIVE SCREENSHOTS CAPTURED SUCCESSFULLY!")


if __name__ == "__main__":
    run_visual_verification()
