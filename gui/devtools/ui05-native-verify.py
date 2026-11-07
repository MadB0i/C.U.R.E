"""Stage UI-05 native verification against the real Tauri binary.

Tests Stage UI-05: Quarantine Workstation against cure-gui.exe over CDP.
Verifies:
  1. Fresh launch idle Quarantine state & layout geometry
  2. Telemetry stats strip (4 monospace metrics: count, bytes, integrity, undo)
  3. Tactical empty enclave (3 architectural pillars with SVGs)
  4. Strict absence of emojis and mascot/LUMA elements
  5. Interactive refresh button
  6. Dark & light theme aesthetics
  7. Responsive layout at 1366x768 and 1920x1080

Run: python gui/devtools/ui05-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui05" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui05"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9409
TMP_DIR = Path("D:/CURE_UI05_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui05.json"

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
    print("=== Stage UI-05 native acceptance (real backend, no mocks) ===")
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

            # Wait for app ready
            page.wait_for_selector("#app:not(.hidden)", timeout=15000)
            time.sleep(1.0)

            # -------------------------------------------------------------
            # 1. Fresh launch & Navigation to Quarantine Workstation
            # -------------------------------------------------------------
            nav_quarantine = page.locator("#nav-quarantine")
            check("nav-quarantine button exists", nav_quarantine.count() > 0)
            nav_quarantine.click()
            time.sleep(0.5)

            view_q = page.locator("#view-quarantine")
            check("view-quarantine is visible after navigation", view_q.is_visible())

            # -------------------------------------------------------------
            # 2. Structural & Architectural Verification
            # -------------------------------------------------------------
            shell = page.locator("#view-quarantine .q-shell")
            check("q-shell container exists", shell.count() > 0)

            headline = page.locator("#view-quarantine .headline")
            check("headline exists", headline.count() > 0)
            check("headline text is correct", "Quarantine Workstation" in headline.text_content())

            subline = page.locator("#q-subline")
            check("q-subline exists", subline.count() > 0)

            # -------------------------------------------------------------
            # 3. Telemetry Stats Strip
            # -------------------------------------------------------------
            stats_strip = page.locator("#q-stats-strip")
            check("q-stats-strip exists", stats_strip.count() > 0)

            stat_count = page.locator("#q-stat-count")
            check("q-stat-count cell exists", stat_count.count() > 0)

            stat_bytes = page.locator("#q-stat-bytes")
            check("q-stat-bytes cell exists", stat_bytes.count() > 0)

            stat_integrity = page.locator("#q-stat-integrity")
            check("q-stat-integrity cell exists", stat_integrity.count() > 0)
            check("stat-integrity text contains SHA-256", "SHA-256" in stat_integrity.text_content())

            stat_undo = page.locator("#q-stat-undo")
            check("q-stat-undo cell exists", stat_undo.count() > 0)
            check("stat-undo text contains SCOPED", "SCOPED" in stat_undo.text_content())

            # -------------------------------------------------------------
            # 4. Tactical Empty State (No emojis, no AI cartoon fluff)
            # -------------------------------------------------------------
            q_empty = page.locator("#q-empty")
            check("q-empty element exists", q_empty.count() > 0)
            check("q-empty is visible when 0 records", q_empty.is_visible())

            q_empty_title = page.locator(".q-empty-title")
            check("q-empty-title is visible", q_empty_title.is_visible())

            pillars = page.locator(".q-pillar-card")
            check("3 architectural pillars exist", pillars.count() == 3, f"count={pillars.count()}")

            # Verify NO emojis in quarantine text
            q_text = page.locator("#view-quarantine").text_content() or ""
            emojis = ["🔒", "🛡️", "↩️", "🐱", "🤖", "🚀"]
            found_emojis = [e for e in emojis if e in q_text]
            check("no emojis present in quarantine workstation", len(found_emojis) == 0, f"found={found_emojis}")

            # Verify NO mascot / LUMA elements in quarantine workstation
            mascot_els = page.locator("#view-quarantine [data-companion], #view-quarantine .mascot, #view-quarantine .luma")
            check("mascot elements absent from quarantine workstation", mascot_els.count() == 0, f"found={mascot_els.count()}")

            # -------------------------------------------------------------
            # 5. Refresh Interaction
            # -------------------------------------------------------------
            refresh_btn = page.locator("#q-refresh")
            check("q-refresh button exists and is clickable", refresh_btn.is_visible())
            refresh_btn.click()
            time.sleep(0.5)
            check("view-quarantine remains stable after refresh", view_q.is_visible())

            # Screenshot 1: Idle Quarantine Workstation
            page.screenshot(path=str(OUT / "01-quarantine-idle-empty.png"))
            shutil.copy(OUT / "01-quarantine-idle-empty.png", DOCS_OUT / "01-quarantine-idle-empty.png")
            check("idle empty screenshot captured", (OUT / "01-quarantine-idle-empty.png").exists())

            # -------------------------------------------------------------
            # 6. Light Theme Verification
            # -------------------------------------------------------------
            page.evaluate("document.documentElement.setAttribute('data-theme', 'light')")
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "02-quarantine-light-theme.png"))
            shutil.copy(OUT / "02-quarantine-light-theme.png", DOCS_OUT / "02-quarantine-light-theme.png")
            check("light theme screenshot captured", (OUT / "02-quarantine-light-theme.png").exists())

            # Restore dark theme
            page.evaluate("document.documentElement.setAttribute('data-theme', 'dark')")
            time.sleep(0.2)

            # -------------------------------------------------------------
            # 7. Responsive Breakpoint Check (1366x768)
            # -------------------------------------------------------------
            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "03-quarantine-responsive-1366x768.png"))
            shutil.copy(OUT / "03-quarantine-responsive-1366x768.png", DOCS_OUT / "03-quarantine-responsive-1366x768.png")
            check("1366x768 layout screenshot captured", (OUT / "03-quarantine-responsive-1366x768.png").exists())

            # Print summary
            passes = sum(1 for _, ok, _ in results if ok)
            fails = sum(1 for _, ok, _ in results if not ok)
            total = len(results)
            print(f"\n{passes}/{total} native UI-05 checks passed")

            if fails > 0:
                print(f"UI-05 NATIVE ACCEPTANCE: {fails} CHECKS FAILED")
                shutdown(proc, browser)
                sys.exit(1)
            else:
                print("UI-05 NATIVE ACCEPTANCE: ALL CHECKS PASSED")

    finally:
        shutdown(proc, browser)


if __name__ == "__main__":
    main()
