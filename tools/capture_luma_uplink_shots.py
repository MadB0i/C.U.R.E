#!/usr/bin/env python3
"""
Capture the 6 required native screenshots for C.U.R.E LUMA Uplink + Storage Rescue.
Connects directly to cure-gui.exe via CDP.
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
EXE = REPO / "gui" / "src-tauri" / "target" / "debug" / "cure-gui.exe"
OUTPUT_DIR = REPO / "testing" / "native" / "screenshots-luma-uplink"
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


def kill_lingering():
    for proc in psutil.process_iter(["pid", "name"]):
        try:
            name = (proc.info["name"] or "").lower()
            if "cure-gui" in name or "msedgewebview2" in name:
                proc.kill()
        except Exception:
            pass
    time.sleep(1.0)


def wait_port(port: int, timeout: float = 45.0):
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


def capture_all():
    kill_lingering()

    temp_dir = Path(tempfile.mkdtemp(prefix="cure-uplink-shots-"))
    webview_data = temp_dir / "webview"
    webview_data.mkdir(parents=True, exist_ok=True)
    port = 9475

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

    results = {}

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
            time.sleep(2.0)

            # Ensure dark theme
            page.evaluate("""() => {
                document.documentElement.setAttribute('data-theme', 'dark');
                document.documentElement.dataset.theme = 'dark';
                const btn = document.getElementById('theme-toggle');
                if (btn) btn.setAttribute('aria-pressed', 'false');
            }""")
            time.sleep(0.5)

            # -----------------------------------------------------------------
            # 1. Rescue with LUMA Uplink
            # -----------------------------------------------------------------
            print("Capturing 01-rescue-luma-uplink.png...")
            page.click("#nav-scan")
            time.sleep(1.0)
            page.evaluate("""() => {
                if (window.luma && window.luma.uplink) {
                    window.luma.uplink('radar', 'CYBER-FORENSIC UPLINK ACTIVE', 'Monitoring host persistence points & collectors');
                }
            }""")
            time.sleep(1.2)
            shot1 = OUTPUT_DIR / "01-rescue-luma-uplink.png"
            page.screenshot(path=str(shot1))
            results["01-rescue-luma-uplink.png"] = {"captured": True, "size": shot1.stat().st_size}

            # -----------------------------------------------------------------
            # 2. Agent Radar with scout feel
            # -----------------------------------------------------------------
            print("Capturing 02-agent-radar-scout.png...")
            page.click("#nav-agents")
            time.sleep(1.2)
            page.evaluate("""() => {
                if (window.luma && window.luma.uplink) {
                    window.luma.uplink('radar', 'SCOUT PROBE LOCK: AGENT RADAR', 'Broadcasting sweep across 14 supported AI agent signatures');
                }
            }""")
            time.sleep(1.2)
            shot2 = OUTPUT_DIR / "02-agent-radar-scout.png"
            page.screenshot(path=str(shot2))
            results["02-agent-radar-scout.png"] = {"captured": True, "size": shot2.stat().st_size}

            # -----------------------------------------------------------------
            # 3. Storage Rescue Overview
            # -----------------------------------------------------------------
            print("Capturing 03-storage-rescue-overview.png...")
            page.click("#nav-cleanup")
            time.sleep(1.0)
            page.click("#sw-tab-rescue")
            time.sleep(1.5)
            shot3 = OUTPUT_DIR / "03-storage-rescue-overview.png"
            page.screenshot(path=str(shot3))
            results["03-storage-rescue-overview.png"] = {"captured": True, "size": shot3.stat().st_size}

            # -----------------------------------------------------------------
            # 4. Storage Rescue Candidate Recommendation List
            # -----------------------------------------------------------------
            print("Capturing 04-storage-rescue-candidates.png...")
            # Scroll .stage to display candidate segmentation table clearly
            page.evaluate("""() => {
                const stage = document.querySelector('.stage');
                if (stage) stage.scrollTop = 320;
            }""")
            time.sleep(1.0)
            shot4 = OUTPUT_DIR / "04-storage-rescue-candidates.png"
            page.screenshot(path=str(shot4))
            results["04-storage-rescue-candidates.png"] = {"captured": True, "size": shot4.stat().st_size}

            # -----------------------------------------------------------------
            # 5. Storage Rescue Migration Plan Modal
            # -----------------------------------------------------------------
            print("Capturing 05-storage-rescue-plan-modal.png...")
            page.evaluate("""() => {
                const stage = document.querySelector('.stage');
                if (stage) stage.scrollTop = 0;
            }""")
            time.sleep(0.5)
            page.click("#sr-btn-build-plan")
            time.sleep(1.2)
            shot5 = OUTPUT_DIR / "05-storage-rescue-plan-modal.png"
            page.screenshot(path=str(shot5))
            results["05-storage-rescue-plan-modal.png"] = {"captured": True, "size": shot5.stat().st_size}

            # -----------------------------------------------------------------
            # 6. Storage Rescue Result / Recovery Screen
            # -----------------------------------------------------------------
            print("Capturing 06-storage-rescue-receipt.png...")
            # Render complete dual-integrity verified receipt modal
            page.evaluate("""() => {
                if (window.storageRescue && window.storageRescue.openReceiptModal) {
                    window.storageRescue.closePlanModal();
                    window.storageRescue.openReceiptModal({
                        receipt_id: "REC-20261030-C2D-88219",
                        completed_at: new Date().toISOString(),
                        source_drive: "C:",
                        target_drive: "D:",
                        total_files_copied: 1420,
                        total_bytes_copied: 26627964928,
                        duration_ms: 18450,
                        items: [{
                            candidate_id: "downloads",
                            source_path: "C:\\\\Users\\\\Operator\\\\Downloads",
                            target_path: "D:\\\\CURE_Relocated\\\\Downloads",
                            files_copied: 1420,
                            bytes_copied: 26627964928,
                            verification: {
                                source_file_count: 1420,
                                target_file_count: 1420,
                                source_total_bytes: 26627964928,
                                target_total_bytes: 26627964928,
                                sampled_hashes_verified: true,
                                integrity_status: "VerifiedBitExact",
                            },
                            source_preserved: true,
                        }],
                        recovery_notes: [
                            "Non-destructive migration: All original data remains untouched on C:\\\\Users\\\\Operator\\\\Downloads.",
                            "Dual-integrity verification passed: 1,420 files (24.8 GB) verified identical bit-for-bit.",
                            "Target directory established at D:\\\\CURE_Relocated\\\\Downloads.",
                            "Operator may manually clean source folder C:\\\\Users\\\\Operator\\\\Downloads once satisfied.",
                            "To revert: Simply copy D:\\\\CURE_Relocated\\\\Downloads back to C:\\\\Users\\\\Operator\\\\Downloads."
                        ]
                    });
                }
            }""")
            time.sleep(1.2)
            shot6 = OUTPUT_DIR / "06-storage-rescue-receipt.png"
            page.screenshot(path=str(shot6))
            results["06-storage-rescue-receipt.png"] = {"captured": True, "size": shot6.stat().st_size}

    finally:
        kill_lingering()
        try:
            shutil.rmtree(temp_dir, ignore_errors=True)
        except Exception:
            pass

    print("\nCapture results summary:")
    for k, v in results.items():
        print(f"  {k}: size={v['size']} bytes")
    return results


if __name__ == "__main__":
    capture_all()
