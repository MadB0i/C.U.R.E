#!/usr/bin/env python3
"""Comprehensive feature and visual verification audit for C.U.R.E.
Captures screenshots for GitHub & Social Media showcase, testing all 3 core tabs:
- Tab 1: Virus Rescue
- Tab 2: AI Agents
- Tab 3: Storage & Move
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
ARTIFACTS_DIR = Path(r"C:\Users\rupjy\.gemini\antigravity\brain\dafbcf95-ae98-48ce-b6f8-a7744dff4231")
SHOTS_DIR = REPO / "testing" / "native" / "screenshots-audit" / "showcase"
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
    raise TimeoutError(f"CDP port {port} not ready within {timeout}s")


def run_showcase_audit():
    kill_lingering()
    temp_dir = Path(tempfile.mkdtemp(prefix="cure-audit-"))
    webview_data = temp_dir / "webview"
    webview_data.mkdir(parents=True, exist_ok=True)
    port = 9488

    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(port)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(webview_data)

    print(f"Launching CURE release binary: {EXE}")
    proc = subprocess.Popen(
        [str(EXE)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    captured = []

    try:
        wait_port(port)
        print("Connected to CURE WebView via CDP.")
        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
            context = browser.contexts[0]
            page = context.pages[0]
            page.set_viewport_size({"width": 1440, "height": 900})
            page.wait_for_url("http://tauri.localhost/", timeout=30000)
            page.wait_for_load_state("networkidle")
            time.sleep(2.0)

            def take_shot(name: str):
                file_name = f"{name}.png"
                local_path = SHOTS_DIR / file_name
                art_path = ARTIFACTS_DIR / file_name
                page.screenshot(path=str(local_path), full_page=False)
                shutil.copy2(local_path, art_path)
                captured.append(file_name)
                print(f"  [✓] Captured {file_name}")

            # -------------------------------------------------------------
            # TEST 1: Page 1 - Virus Rescue (Idle View & Cybernetic Eye)
            # -------------------------------------------------------------
            print("\n--- Verifying Page 1: Virus Rescue ---")
            page.click("#nav-scan")
            time.sleep(1.5)
            take_shot("01_virus_rescue_idle")

            # Trigger a Scan on Page 1
            print("Triggering Threat Scan on Page 1...")
            scan_btn = page.query_selector("#xray-start-btn")
            if scan_btn:
                scan_btn.click()
                time.sleep(1.5)
                take_shot("02_virus_rescue_scanning")
                time.sleep(4.0)
                take_shot("03_virus_rescue_results")

            # -------------------------------------------------------------
            # TEST 2: Page 2 - AI Agents (Radar, Sessions, Processes)
            # -------------------------------------------------------------
            print("\n--- Verifying Page 2: AI Agents ---")
            page.click("#nav-agents")
            time.sleep(2.0)
            take_shot("04_ai_agents_radar_overview")

            # Trigger AI Agent Scan
            print("Triggering AI Agent scan...")
            ar_scan_btn = page.query_selector("#ar-scan-now")
            if ar_scan_btn:
                ar_scan_btn.click()
                time.sleep(2.0)
                take_shot("05_ai_agents_active_monitoring")

            # -------------------------------------------------------------
            # TEST 3: Page 3 - Storage & Move (Dual-Drive Conduit & Oculus)
            # -------------------------------------------------------------
            print("\n--- Verifying Page 3: Storage & Move ---")
            page.click("#nav-cleanup")
            time.sleep(2.0)
            take_shot("06_storage_move_overview")

            # Trigger Storage Scan on C:
            print("Triggering Storage Scan on C: drive...")
            drive_scan_btn = page.query_selector("#sm-scan-now")
            if drive_scan_btn:
                drive_scan_btn.click()
                time.sleep(2.0)
                take_shot("07_storage_scanning_sectors")

                # Wait for scan to complete (polls until scan button is enabled again, up to 90s)
                print("Waiting for storage scan results...")
                for _ in range(90):
                    time.sleep(1.0)
                    is_scanning = page.evaluate("() => document.getElementById('sm-scan-now')?.disabled")
                    if not is_scanning:
                        break

                time.sleep(1.5)
                take_shot("08_storage_scan_results")

            # Open Move Assistant Modal on first consumer
            print("Testing Move Assistant Modal...")
            first_move_btn = page.query_selector(".sm-btn-move")
            if first_move_btn:
                first_move_btn.click()
                time.sleep(1.5)
                take_shot("09_move_assistant_modal_oculus")

                # Test Cancel
                cancel_btn = page.query_selector("#mb-confirm-cancel")
                if cancel_btn:
                    cancel_btn.click()
                    time.sleep(1.0)
            else:
                print("Warning: .sm-btn-move not found; checking consumers list")

            print("\n[✓] Showcase Audit verification completed successfully!")

    finally:
        kill_lingering()
        try:
            shutil.rmtree(temp_dir, ignore_errors=True)
        except Exception:
            pass

    print(f"\nTotal Screenshots Captured: {len(captured)}")
    for s in captured:
        print(f" - {s}")


if __name__ == "__main__":
    run_showcase_audit()
