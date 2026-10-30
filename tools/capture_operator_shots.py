#!/usr/bin/env python3
"""
Capture the 6 required native screenshots for C.U.R.E OPERATOR ENGINE V1.
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
OUTPUT_DIR = REPO / "testing" / "native" / "screenshots-operator-engine"
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

    temp_dir = Path(tempfile.mkdtemp(prefix="cure-operator-shots-"))
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
            console_logs = []
            page.on("console", lambda msg: (console_logs.append(f"[{msg.type}] {msg.text}"), print(f"PAGE CONSOLE [{msg.type}]: {msg.text}", flush=True)))
            page.on("pageerror", lambda err: print(f"PAGE ERROR: {err}", flush=True))
            page.on("dialog", lambda d: (print(f"CDP Dialog: {d.message}"), d.accept()))
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
            # 1. Professional Scanner with Operator Status & Threat Response
            # -----------------------------------------------------------------
            print("Capturing 01-rescue-operator-scanner.png...")
            page.click("#nav-scan")
            time.sleep(1.0)
            # Update operator node to show active forensic state
            page.evaluate("""() => {
                if (window.cureOperator && window.cureOperator.update) {
                    window.cureOperator.update({
                        state: "active",
                        heading: "FORENSIC RADAR & THREAT RESPONSE ACTIVE",
                        subheading: "Live persistence vector collector matrix: 9 vectors monitored · Host Response Level 1 (Review)",
                        risk: "level-1",
                        actions: [
                            { label: "Scan Persistence", action: "scan" },
                            { label: "Defender Sweep", action: "defender-sweep" }
                        ]
                    });
                }
            }""")
            time.sleep(1.0)
            shot1 = OUTPUT_DIR / "01-rescue-operator-scanner.png"
            page.screenshot(path=str(shot1))
            results["01-rescue-operator-scanner.png"] = {"captured": True, "size": shot1.stat().st_size}

            # -----------------------------------------------------------------
            # 2. Agent Radar with Operator Session Controls & Process Tree
            # -----------------------------------------------------------------
            print("Capturing 02-agent-radar-operator-sessions.png...")
            page.click("#nav-agents")
            time.sleep(1.2)
            page.evaluate("""() => {
                if (window.cureOperator && window.cureOperator.update) {
                    window.cureOperator.update({
                        state: "active",
                        heading: "AGENT SESSION CONTROL ENGINE",
                        subheading: "Tracking process trees, child workers, and session lifecycles for detected AI developer agents",
                        risk: "nominal",
                        actions: [
                            { label: "Refresh Sessions", action: "refresh-sessions" }
                        ]
                    });
                }
            }""")
            time.sleep(0.5)
            # Open agent inspector modal to display PID race protected session controls
            btn = page.query_selector(".ar-agent-btn-inspect")
            if btn:
                btn.click()
                time.sleep(1.0)
            shot2 = OUTPUT_DIR / "02-agent-radar-operator-sessions.png"
            page.screenshot(path=str(shot2))
            results["02-agent-radar-operator-sessions.png"] = {"captured": True, "size": shot2.stat().st_size}
            page.evaluate("""() => {
                const btn = document.getElementById("ar-insp-close");
                if (btn) btn.click();
                const m = document.getElementById("ar-inspector-modal");
                if (m) m.classList.add("hidden");
            }""")
            time.sleep(0.5)

            # -----------------------------------------------------------------
            # 3. Storage Rescue Overview
            # -----------------------------------------------------------------
            print("Capturing 03-storage-rescue-operator-overview.png...")
            page.click("#nav-cleanup")
            time.sleep(0.5)
            page.click("#sw-tab-rescue")
            page.wait_for_selector("#sr-safe-candidates-body tr", timeout=45000)
            time.sleep(1.0)
            shot3 = OUTPUT_DIR / "03-storage-rescue-operator-overview.png"
            page.screenshot(path=str(shot3))
            results["03-storage-rescue-operator-overview.png"] = {"captured": True, "size": shot3.stat().st_size}

            # -----------------------------------------------------------------
            # 4. Storage Rescue Candidate Recommendation List
            # -----------------------------------------------------------------
            print("Capturing 04-storage-rescue-candidates.png...")
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
            page.wait_for_selector("#sr-plan-modal:not(.hidden)", timeout=45000)
            time.sleep(1.0)
            shot5 = OUTPUT_DIR / "05-storage-rescue-plan-modal.png"
            page.screenshot(path=str(shot5))
            results["05-storage-rescue-plan-modal.png"] = {"captured": True, "size": shot5.stat().st_size}

            # -----------------------------------------------------------------
            # 6. Storage Rescue Verified Receipt & Recovery / Cleanup Controls
            # -----------------------------------------------------------------
            print("Capturing 06-storage-rescue-receipt-and-cleanup.png...")
            page.evaluate("""() => {
                if (window.storageRescue && window.storageRescue.openReceiptModal) {
                    window.storageRescue.closePlanModal();
                    const rec = {
                        receipt_id: "REC-20261030-C2D-OPERATOR-9140",
                        completed_at: new Date().toISOString(),
                        source_drive: "C:",
                        target_drive: "D:",
                        total_files_copied: 1845,
                        total_bytes_copied: 28456128000,
                        duration_ms: 22100,
                        items: [{
                            candidate_id: "workspace_demo",
                            source_path: "C:\\\\Projects\\\\webapp",
                            target_path: "D:\\\\CURE_Relocated\\\\webapp",
                            files_copied: 1845,
                            bytes_copied: 28456128000,
                            verification: {
                                source_file_count: 1845,
                                target_file_count: 1845,
                                source_total_bytes: 28456128000,
                                target_total_bytes: 28456128000,
                                sha256_checked: 1845,
                                sha256_matched: 1845,
                                sampled_hashes_verified: true,
                                integrity_status: "VerifiedBitExact",
                                metadata_preservation: {
                                    timestamps_preserved: true,
                                    readonly_flags_preserved: true,
                                    reparse_points_safely_skipped: 0
                                }
                            },
                            source_preserved: true,
                        }],
                        recovery_notes: [
                            "100% SHA-256 verified copy: 1,845 files (26.5 GB) verified bit-exact on target.",
                            "Non-destructive copy completed: Source files remain completely untouched on C:.",
                            "Target established at D:\\\\CURE_Relocated\\\\Downloads.",
                            "Known Folder redirection or toolchain environment switch can now be finalized.",
                            "User-approved source cleanup is optional and requires explicit confirmation token."
                        ]
                    };
                    window.storageRescue.state.receipt = rec;
                    window.storageRescue.openReceiptModal(rec);
                }
            }""")
            time.sleep(1.2)
            shot6 = OUTPUT_DIR / "06-storage-rescue-receipt-and-cleanup.png"
            page.screenshot(path=str(shot6))
            results["06-storage-rescue-receipt-and-cleanup.png"] = {"captured": True, "size": shot6.stat().st_size}

            # -----------------------------------------------------------------
            # 7. Storage Rescue Activated & Validation Passed (Cleanup Unlocked)
            # -----------------------------------------------------------------
            print("Capturing 07-storage-rescue-activated-cleanup-unlocked.png...")
            time.sleep(1.0)
            page.click("#sr-receipt-finalize")
            page.wait_for_function(
                "() => document.getElementById('sr-receipt-finalize').textContent.includes('Activated')",
                timeout=35000,
            )
            time.sleep(0.8)
            shot7 = OUTPUT_DIR / "07-storage-rescue-activated-cleanup-unlocked.png"
            page.screenshot(path=str(shot7))
            results["07-storage-rescue-activated-cleanup-unlocked.png"] = {"captured": True, "size": shot7.stat().st_size}

            page.click("#sr-receipt-done")
            time.sleep(0.5)

            # -----------------------------------------------------------------
            # 8. Windows Defender Native Verification & Threat Response Modal
            # -----------------------------------------------------------------
            print("Capturing 08-defender-threat-response-modal.png...")
            page.click("#nav-scan")
            time.sleep(0.8)
            page.click("#xray-defender-scan-btn")
            time.sleep(1.0)
            # Demonstrate journal cryptographic hash verification and Defender target input
            page.evaluate("""() => {
                if (window.openDefenderModal) {
                    window.openDefenderModal();
                }
                const inp = document.getElementById('xray-defender-path-input');
                if (inp) inp.value = 'C:\\\\Windows\\\\System32\\\\cmd.exe';
            }""")
            time.sleep(0.5)
            page.click("#xray-defender-verify-journal")
            page.wait_for_function(
                "() => !document.getElementById('xray-defender-output-box').classList.contains('hidden')",
                timeout=15000,
            )
            time.sleep(1.0)
            shot8 = OUTPUT_DIR / "08-defender-threat-response-modal.png"
            page.screenshot(path=str(shot8))
            results["08-defender-threat-response-modal.png"] = {"captured": True, "size": shot8.stat().st_size}

            close_def = page.query_selector("#xray-defender-modal-done")
            if close_def:
                close_def.click()
                time.sleep(0.5)

            print("\nRecent console logs:")
            for l in console_logs[-25:]:
                print("  ", l)

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
