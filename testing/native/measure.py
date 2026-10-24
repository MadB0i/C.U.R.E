#!/usr/bin/env python3
"""Measure the native application's CPU and memory, idle and under load.

Idle is measured on the Overview with nothing running. Under load is
measured during a real storage scan, which is the heaviest thing the
application does. The numbers are the operating system's own accounting of
the process, not a guess.

    python testing/native/measure.py
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
PORT = 9377


def sample(pid: int) -> tuple[float, int]:
    """CPU seconds and working set in MiB, from the OS."""
    import ctypes
    from ctypes import wintypes

    class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    kernel32 = ctypes.windll.kernel32
    psapi = ctypes.windll.psapi
    counters = PROCESS_MEMORY_COUNTERS()
    counters.cb = ctypes.sizeof(counters)
    psapi.GetProcessMemoryInfo(
        kernel32.OpenProcess(0x1000, False, pid),
        ctypes.byref(counters),
        counters.cb,
    )
    class FILETIME(ctypes.Structure):
        _fields_ = [("dwLowDateTime", wintypes.DWORD),
                    ("dwHighDateTime", wintypes.DWORD)]

        @property
        def value(self):
            return (self.dwHighDateTime << 32) | self.dwLowDateTime

    created, exited, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
    handle = kernel32.OpenProcess(0x1000, False, pid)
    kernel32.GetProcessTimes(
        handle,
        ctypes.byref(created),
        ctypes.byref(exited),
        ctypes.byref(kernel),
        ctypes.byref(user),
    )
    cpu = (kernel.value + user.value) / 10_000_000.0
    return cpu, counters.WorkingSetSize // (1024 * 1024)


def main() -> int:
    env = dict(os.environ)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={PORT}"
    user_data = tempfile.mkdtemp(prefix="cure-measure-")
    env["WEBVIEW2_USER_DATA_FOLDER"] = user_data
    app = subprocess.Popen([str(EXE)], env=env)
    results = {}
    try:
        time.sleep(12)
        from playwright.sync_api import sync_playwright

        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
            page = browser.contexts[0].pages[0]
            page.set_viewport_size({"width": 1440, "height": 900})
            time.sleep(3)

            # --- idle on the Overview -------------------------------------
            cpu0, ws0 = sample(app.pid)
            time.sleep(10)
            cpu1, ws1 = sample(app.pid)
            results["idle"] = {
                "seconds": 10,
                "cpu_seconds": round(cpu1 - cpu0, 3),
                "cpu_percent_of_one_core": round((cpu1 - cpu0) * 10, 1),
                "working_set_mib": ws1,
            }

            # --- the companion alone, with motion allowed ------------------
            page.evaluate("() => document.getElementById('nav-overview').click()")
            time.sleep(1.5)
            cpu0, _ = sample(app.pid)
            time.sleep(10)
            cpu1, _ = sample(app.pid)
            results["overview_with_companion"] = {
                "seconds": 10,
                "cpu_seconds": round(cpu1 - cpu0, 3),
                "cpu_percent_of_one_core": round((cpu1 - cpu0) * 10, 1),
            }

            # --- a real storage scan --------------------------------------
            page.evaluate("() => document.getElementById('nav-cleanup').click()")
            time.sleep(1.5)
            cpu0, ws0 = sample(app.pid)
            page.evaluate("() => document.getElementById('sw-scan').click()")
            deadline = time.time() + 300
            while time.time() < deadline:
                status = page.evaluate(
                    "() => (document.getElementById('sw-status-text')||{}).textContent"
                )
                if status == "Measured":
                    break
                time.sleep(0.5)
            cpu1, ws1 = sample(app.pid)
            elapsed = round(cpu1 - cpu0, 3)
            results["storage_scan"] = {
                "cpu_seconds": elapsed,
                "working_set_mib": ws1,
                "working_set_growth_mib": ws1 - results["idle"]["working_set_mib"],
            }

            # --- after the scan, idle again -------------------------------
            time.sleep(2)
            cpu0, _ = sample(app.pid)
            time.sleep(10)
            cpu1, _ = sample(app.pid)
            results["after_scan_idle"] = {
                "seconds": 10,
                "cpu_seconds": round(cpu1 - cpu0, 3),
                "cpu_percent_of_one_core": round((cpu1 - cpu0) * 10, 1),
            }
            browser.close()
    finally:
        try:
            app.terminate()
            app.wait(timeout=10)
        except Exception:
            app.kill()
        try:
            shutil.rmtree(user_data, ignore_errors=True)
        except Exception:
            pass
    print(json.dumps(results, indent=2))
    (REPO / "docs" / "native-captures" / "performance.json").write_text(
        json.dumps(results, indent=2) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
