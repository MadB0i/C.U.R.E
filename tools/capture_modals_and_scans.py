#!/usr/bin/env python3
"""Capture modals, drawers, and active scan states for UI audit."""

from __future__ import annotations

import json
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


def capture_modals():
    kill_lingering()
    temp_dir = Path(tempfile.mkdtemp(prefix="cure-modal-audit-"))
    webview_data = temp_dir / "webview"
    webview_data.mkdir(parents=True, exist_ok=True)
    port = 9447

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

            # 1. Trigger Confirmation Modal
            page.evaluate("""() => {
                const overlay = document.getElementById('confirm-overlay');
                document.getElementById('confirm-kicker').textContent = 'Safety Confirmation';
                document.getElementById('confirm-title').textContent = 'Quarantine Persistence Entry?';
                const facts = document.getElementById('confirm-facts');
                facts.innerHTML = '<dt>Target</dt><dd>C:\\\\Windows\\\\System32\\\\fake-persistence.dll</dd><dt>Mechanism</dt><dd>Registry Run Key (HKLM)</dd><dt>Confidence</dt><dd>Observed Persistence</dd>';
                document.getElementById('confirm-ok').textContent = 'Confirm Quarantine';
                overlay.classList.remove('hidden');
            }""")
            time.sleep(0.4)
            set_theme("dark")
            page.screenshot(path=str(SHOTS_DIR / "10-confirm-modal-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "10-confirm-modal-light.png"))
            page.evaluate("() => document.getElementById('confirm-overlay').classList.add('hidden')")
            print("Captured confirmation modal")

            # 2. Trigger Evidence Drawer
            page.evaluate(r"""() => {
                const overlay = document.getElementById('evidence-overlay');
                document.getElementById('evidence-title').textContent = 'Suspicious Background Run Key';
                document.getElementById('evidence-subtitle').textContent = 'Registry Autorun (HKCU/Software/Microsoft/Windows/CurrentVersion/Run)';
                const body = document.getElementById('evidence-body');
                body.innerHTML = `
                    <div class="evidence-section">
                        <h4>Observation Summary</h4>
                        <p>File referenced in user run key does not exist on disk or has unverified digital signature.</p>
                    </div>
                    <dl class="evidence-props">
                        <dt>Target Path</dt><dd><code>C:/Users/Operator/AppData/Local/Temp/suspicious_task.exe</code></dd>
                        <dt>Registry Value</dt><dd><code>"C:/Users/Operator/AppData/Local/Temp/suspicious_task.exe" --silent</code></dd>
                        <dt>Digital Signature</dt><dd><span class="badge badge-unverified">Unsigned / Unverified</span></dd>
                        <dt>Integrity Check</dt><dd>SHA256 unavailable (file missing)</dd>
                    </dl>
                `;
                overlay.classList.remove('hidden');
            }""")
            time.sleep(0.4)
            set_theme("dark")
            page.screenshot(path=str(SHOTS_DIR / "09-evidence-drawer-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "09-evidence-drawer-light.png"))
            page.evaluate("() => document.getElementById('evidence-overlay').classList.add('hidden')")
            print("Captured evidence drawer")

            # 3. Trigger Rescue Scan in Overview
            set_theme("dark")
            page.click("#nav-overview")
            time.sleep(0.5)
            # Click Start Rescue Scan
            scan_btn = page.locator("button:has-text('Start Rescue Scan')").first
            if scan_btn.count() > 0:
                scan_btn.click()
                time.sleep(1.0)
                page.screenshot(path=str(SHOTS_DIR / "12-overview-scanning-active.png"))
                print("Captured overview scanning active")
                # Wait for scan to complete
                time.sleep(3.0)
                page.screenshot(path=str(SHOTS_DIR / "13-overview-scan-completed-dark.png"))
                set_theme("light")
                page.screenshot(path=str(SHOTS_DIR / "13-overview-scan-completed-light.png"))
                print("Captured overview scan completed")

            # 4. View Investigate tab now that scan is done!
            set_theme("dark")
            page.click("#nav-results")
            time.sleep(0.8)
            page.screenshot(path=str(SHOTS_DIR / "14-investigate-with-findings-dark.png"))
            set_theme("light")
            page.screenshot(path=str(SHOTS_DIR / "14-investigate-with-findings-light.png"))
            print("Captured investigate with findings")

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
    print("ALL MODALS & SCANS CAPTURED SUCCESSFULLY!")


if __name__ == "__main__":
    capture_modals()
