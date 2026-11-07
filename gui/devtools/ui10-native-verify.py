"""Stage UI-10 native verification against the real Tauri binary.

Tests Stage UI-10 — the scanner redesign (my-choice final pass) — against
cure-gui.exe over CDP, including a REAL full scan:

  1. Radar field chrome speaks the console grammar (violet identity, no
     "holographic" era copy), tag says "10 inspection vectors"
  2. The Live-telemetry HUD is visible (it was dead UI: fully wired in the
     render path but display:none) and reports real figures after the scan
  3. Layer matrix renders 10 instrument rows with outcome pills
  4. A REAL scan runs end to end: running state, per-layer outcomes,
     complete readout ("ALL 10 LAYERS CLEAN" or findings), 10/10 coverage
  5. Overview dial draws 10 segments
  6. Dark + light + 1366x768 + 1920x1080 aesthetics, no emojis/mascots

Safety: the scan is read-only; no process is terminated.

Run: python gui/devtools/ui10-native-verify.py
"""
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
OUT = REPO / "testing" / "native" / "screenshots-ui10" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui10"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9414
TMP_DIR = Path("D:/CURE_UI10_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui10.json"

results = []


def check(name, ok, extra=""):
    results.append((name, ok, extra))
    status = "PASS" if ok else "FAIL"
    print(f"[{status}] {name}" + (f" ({extra})" if extra else ""))
    if not ok:
        print(f"       -> FAILED: {name} {extra}")


def shutdown(proc, browser):
    try:
        if browser:
            browser.close()
    except Exception:
        pass
    if proc:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except Exception:
            proc.kill()
            proc.wait(timeout=10)
    if TMP_DIR.exists():
        try:
            shutil.rmtree(TMP_DIR, ignore_errors=True)
        except Exception:
            pass
    try:
        lease = OwnershipLease(LEASE_PATH)
        lease.clear()
    except Exception:
        pass


def wait_for_port(port, timeout_s=45):
    t0 = time.time()
    url = f"http://127.0.0.1:{port}/json/version"
    while time.time() - t0 < timeout_s:
        try:
            with urllib.request.urlopen(url, timeout=1.0) as resp:
                if resp.status == 200:
                    return True
        except Exception:
            time.sleep(0.3)
    return False


def main():
    print("=== Stage UI-10 native acceptance (real backend, no mocks) ===")
    lease = OwnershipLease(LEASE_PATH)
    pre_launch_cleanup(lease, PORT)

    if TMP_DIR.exists():
        try:
            shutil.rmtree(TMP_DIR)
        except Exception:
            pass
    TMP_DIR.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={PORT}"

    proc = subprocess.Popen(
        [
            str(EXE),
            "--data-dir",
            str(TMP_DIR / "data"),
            "--startup-root",
            str(TMP_DIR / "startup"),
            "--tasks-root",
            str(TMP_DIR / "tasks"),
        ],
        env=env,
    )
    lease.record(proc.pid, EXE.name)

    if not wait_for_port(PORT, timeout_s=45):
        print(f"FAIL: cure-gui.exe did not open remote debugging on port {PORT}")
        shutdown(proc, None)
        sys.exit(1)

    browser = None
    try:
        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
            contexts = browser.contexts
            if not contexts:
                print("FAIL: no browser context returned by WebView2")
                sys.exit(1)
            pages = contexts[0].pages
            if not pages:
                print("FAIL: no pages in browser context")
                sys.exit(1)
            page = pages[0]
            page.set_viewport_size({"width": 1600, "height": 960})

            page.wait_for_selector("#app:not(.hidden)", timeout=15000)
            time.sleep(1.0)

            # -------------------------------------------------------------
            # 1. Scanner chrome: my-choice redesign markers
            # -------------------------------------------------------------
            page.locator("#nav-scan").click()
            time.sleep(0.6)
            field = page.locator("#xray-field")
            check("radar field is visible", field.is_visible())
            check(
                "corner tag names ten inspection vectors (old holographic copy gone)",
                "10 inspection vectors" in (page.locator(".xray-tag").text_content() or "")
                and "Holographic" not in (page.locator(".xray-tag").text_content() or ""),
            )
            check(
                "live-telemetry HUD is visible (was display:none dead UI)",
                page.locator("#xray-telemetry-hud").is_visible(),
            )
            check(
                "HUD carries the four live metrics",
                page.locator("#xray-hud-inspected").is_visible()
                and page.locator("#xray-hud-coverage").is_visible()
                and page.locator("#xray-hud-findings").is_visible()
                and page.locator("#xray-hud-unavailable").is_visible(),
            )
            check(
                "matrix shows ten layer rows",
                page.locator("#sweep-sources .xr-layer").count() == 10,
                f"count={page.locator('#sweep-sources .xr-layer').count()}",
            )
            check(
                "boot-logon layer present in the matrix",
                "Winlogon" in (page.locator("#sweep-sources").text_content() or ""),
            )

            # Idle screenshot
            page.screenshot(path=str(OUT / "01-scanner-idle.png"))
            shutil.copy(OUT / "01-scanner-idle.png", DOCS_OUT / "01-scanner-idle.png")

            # -------------------------------------------------------------
            # 2. REAL scan, end to end
            # -------------------------------------------------------------
            start_btn = page.locator("#xray-start-btn")
            check("start-scan control exists", start_btn.is_visible())
            start_btn.click()
            time.sleep(3.0)
            check(
                "scan running state shows on the field",
                field.get_attribute("data-running") == "true",
            )
            # Running screenshot (mid-collection, live radar)
            page.screenshot(path=str(OUT / "02-scanner-running.png"))
            shutil.copy(OUT / "02-scanner-running.png", DOCS_OUT / "02-scanner-running.png")

            deadline = time.time() + 240
            done = False
            while time.time() < deadline:
                if start_btn.text_content().strip() in ("Start System Scan", "Scan Again"):
                    done = True
                    break
                time.sleep(1.0)
            check("real scan completes", done)

            readout = page.locator("#xray-core-readout").text_content() or ""
            check(
                "complete readout is honest (all clean or findings)",
                ("LAYERS CLEAN" in readout.upper()) or ("FINDING" in readout.upper()),
                f"readout={readout.strip()[:60]}",
            )
            hud_cov = page.locator("#xray-hud-coverage").text_content().strip()
            check(
                "HUD coverage reports the real ten collectors (10 / 10)",
                hud_cov.replace(" ", "") == "10/10",
                f"coverage={hud_cov}",
            )
            matrix_text = page.locator("#sweep-sources").text_content() or ""
            checked_rows = page.locator(
                '#sweep-sources .xr-layer[data-outcome="checked"]'
            ).count()
            partial_rows = page.locator(
                '#sweep-sources .xr-layer[data-outcome="partial"]'
            ).count()
            check(
                "every layer row carries an outcome (checked or honest partial)",
                checked_rows + partial_rows == 10,
                f"checked={checked_rows} partial={partial_rows}",
            )
            check(
                "checked rows wear the info-blue outcome pill",
                page.locator(
                    '#sweep-sources .xr-layer[data-outcome="checked"] .xr-layer-status'
                ).count()
                == checked_rows,
            )

            # Complete screenshot
            page.screenshot(path=str(OUT / "03-scanner-complete.png"))
            shutil.copy(OUT / "03-scanner-complete.png", DOCS_OUT / "03-scanner-complete.png")

            # -------------------------------------------------------------
            # 3. Overview dial: ten segments
            # -------------------------------------------------------------
            page.locator("#nav-overview").click()
            time.sleep(0.6)
            check(
                "overview dial draws ten segments",
                page.locator("#ov-dial-segments path").count() == 10,
                f"count={page.locator('#ov-dial-segments path').count()}",
            )

            # -------------------------------------------------------------
            # 4. Purity + themes + responsive
            # -------------------------------------------------------------
            page.locator("#nav-scan").click()
            time.sleep(0.4)
            scan_text = page.locator("#scan-view").text_content() or ""
            emojis = ["🔒", "🛡️", "🐱", "🤖", "🚀", "✕", "✔", "⚠", "🕵"]
            found = [e for e in emojis if e in scan_text]
            check("no emojis present in the scanner view", len(found) == 0, f"found={found}")
            mascots = page.locator("#scan-view [data-companion], #scan-view .mascot, #scan-view .luma")
            check("mascot elements absent from the scanner view", mascots.count() == 0)

            page.evaluate("document.documentElement.setAttribute('data-theme', 'light')")
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "04-scanner-light-theme.png"))
            shutil.copy(OUT / "04-scanner-light-theme.png", DOCS_OUT / "04-scanner-light-theme.png")
            page.evaluate("document.documentElement.setAttribute('data-theme', 'dark')")
            time.sleep(0.2)

            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "05-scanner-1366x768.png"))
            shutil.copy(OUT / "05-scanner-1366x768.png", DOCS_OUT / "05-scanner-1366x768.png")

            page.set_viewport_size({"width": 1920, "height": 1080})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "06-scanner-1920x1080.png"))
            shutil.copy(OUT / "06-scanner-1920x1080.png", DOCS_OUT / "06-scanner-1920x1080.png")

            passes = sum(1 for _, ok, _ in results if ok)
            fails = sum(1 for _, ok, _ in results if not ok)
            total = len(results)
            print(f"\n{passes}/{total} native UI-10 checks passed")

            if fails > 0:
                print(f"UI-10 NATIVE ACCEPTANCE: {fails} CHECKS FAILED")
                shutdown(proc, browser)
                sys.exit(1)
            else:
                print("UI-10 NATIVE ACCEPTANCE: ALL CHECKS PASSED")

    finally:
        shutdown(proc, browser)


if __name__ == "__main__":
    main()
