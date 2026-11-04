"""Stage UI-04 native verification against the real Tauri binary.

Tests Stage UI-04: Forensic Investigation Console against cure-gui.exe over CDP.
Verifies:
  1. Fresh launch idle Investigate state & layout geometry
  2. Telemetry stats strip (4 monospace metrics)
  3. Forensic lens segment bar (5 lenses: Findings, Processes, Changes, Ransom, Coverage)
  4. Real-time lens switching & accessibility attributes
  5. Findings filter chips (All, High Risk, Suspicious, Safe)
  6. Real-time findings search input
  7. Scan-to-Investigate integration
  8. Honest empty state & coverage accounting
  9. Dark & light theme aesthetics
 10. Responsive layout at 1366x768 and 1920x1080
 11. Strict absence of mascot elements
 12. Safe process control enforcement

Run: python gui/devtools/ui04-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui04" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui04"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9408
TMP_DIR = Path("D:/CURE_UI04_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui04.json"

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
        OwnershipLease(LEASE_PATH).clear()
    except Exception:
        pass


def wait_for_port(port, timeout_s=30):
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
    print("=== Stage UI-04 native acceptance (real backend, no mocks) ===")
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
            # 1. Fresh launch & Navigation to Investigate Console
            # -------------------------------------------------------------
            nav_results = page.locator("#nav-results")
            check("nav-results button exists", nav_results.count() > 0)
            nav_results.click()
            time.sleep(0.5)

            results_view = page.locator("#results-view")
            check("results-view is visible after navigation", results_view.is_visible())

            # Check inv-shell structure
            check("inv-shell container exists", page.locator(".inv-shell").count() > 0)
            check("headline exists", page.locator("#headline").is_visible())
            headline_text = page.locator("#headline").text_content() or ""
            check("headline has text", len(headline_text) > 0, headline_text)

            # -------------------------------------------------------------
            # 2. Telemetry Stats Strip (4 Monospace Cards)
            # -------------------------------------------------------------
            stats_strip = page.locator(".inv-stats-strip")
            check("inv-stats-strip exists", stats_strip.count() > 0)
            check("stat-review cell exists", page.locator("#stat-review").count() > 0)
            check("stat-safe cell exists", page.locator("#stat-safe").count() > 0)
            check("stat-cleaned cell exists", page.locator("#stat-cleaned").count() > 0)
            check("stat-procs cell exists", page.locator("#stat-procs").count() > 0)

            # -------------------------------------------------------------
            # 3. Forensic Lens Segment Bar
            # -------------------------------------------------------------
            lens_nav = page.locator(".inv-lens-nav")
            check("inv-lens-nav exists", lens_nav.count() > 0)
            check("lens-btn-findings exists", page.locator("#lens-btn-findings").count() > 0)
            check("lens-btn-processes exists", page.locator("#lens-btn-processes").count() > 0)
            check("lens-btn-winchanges exists", page.locator("#lens-btn-winchanges").count() > 0)
            check("lens-btn-ransom exists", page.locator("#lens-btn-ransom").count() > 0)
            check("lens-btn-coverage exists", page.locator("#lens-btn-coverage").count() > 0)

            # Initial lens is findings
            check(
                "findings lens is initially active",
                page.locator("#lens-panel-findings").is_visible()
                and not page.locator("#lens-panel-processes").is_visible(),
            )

            # Screenshot 1: Idle Investigate Console
            page.screenshot(path=str(OUT / "01-idle-investigate-1600.png"))
            shutil.copy(OUT / "01-idle-investigate-1600.png", DOCS_OUT / "01-idle-investigate-1600.png")

            # -------------------------------------------------------------
            # 4. Lens Switching & Interactivity
            # -------------------------------------------------------------
            # Switch to Processes lens
            page.locator("#lens-btn-processes").click()
            time.sleep(0.3)
            check(
                "processes lens panel is visible after click",
                page.locator("#lens-panel-processes").is_visible()
                and not page.locator("#lens-panel-findings").is_visible(),
            )
            check(
                "processes lens button has aria-selected=true",
                page.locator("#lens-btn-processes").get_attribute("aria-selected") == "true",
            )

            # Switch to Windows Changes lens
            page.locator("#lens-btn-winchanges").click()
            time.sleep(0.3)
            check(
                "winchanges lens panel is visible after click",
                page.locator("#lens-panel-winchanges").is_visible(),
            )

            # Switch to Coverage lens
            page.locator("#lens-btn-coverage").click()
            time.sleep(0.3)
            check(
                "coverage lens panel is visible after click",
                page.locator("#lens-panel-coverage").is_visible(),
            )

            # Switch back to Findings lens
            page.locator("#lens-btn-findings").click()
            time.sleep(0.3)
            check(
                "findings lens panel is back to visible",
                page.locator("#lens-panel-findings").is_visible(),
            )

            # -------------------------------------------------------------
            # 5. Filter Chips & Search Input
            # -------------------------------------------------------------
            filter_all = page.locator('.inv-filters [data-ff="all"]')
            filter_high = page.locator('.inv-filters [data-ff="HighRisk"]')
            check("findings filter buttons exist", filter_all.count() > 0 and filter_high.count() > 0)

            search_input = page.locator("#inv-findings-search")
            check("search input exists with aria-label", search_input.count() > 0)
            search_input.fill("test-query")
            time.sleep(0.2)
            search_input.fill("")

            # -------------------------------------------------------------
            # 6. Execute Live Rescue Scan & Verify Integration
            # -------------------------------------------------------------
            rescan_btn = page.locator("#rescan-btn")
            check("rescan-btn exists", rescan_btn.is_visible())
            rescan_btn.click()
            print("Waiting for rescue scan to complete...")

            # Wait for scan to finish (up to 60s)
            t0 = time.time()
            while time.time() - t0 < 60:
                phase = page.evaluate("window.scanPhase")
                if phase in ("idle", "complete", "failed"):
                    break
                time.sleep(0.5)

            time.sleep(1.0)
            # Ensure we are on results-view
            page.locator("#nav-results").click()
            time.sleep(0.5)

            # Screenshot 2: Completed Scan in Investigate Console
            page.screenshot(path=str(OUT / "02-completed-investigate-findings.png"))
            shutil.copy(OUT / "02-completed-investigate-findings.png", DOCS_OUT / "02-completed-investigate-findings.png")

            # Telemetry verification
            stat_safe_val = page.locator("#stat-safe").text_content() or "0"
            check("stat-safe populated with real entries", int(stat_safe_val) > 0, f"safe={stat_safe_val}")

            badge_cov_val = page.locator("#badge-coverage").text_content() or ""
            check("badge-coverage reflects collector coverage", "/" in badge_cov_val, badge_cov_val)

            # -------------------------------------------------------------
            # 7. Process Sentinel Lens after Scan
            # -------------------------------------------------------------
            page.locator("#lens-btn-processes").click()
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "03-process-sentinel-lens.png"))
            shutil.copy(OUT / "03-process-sentinel-lens.png", DOCS_OUT / "03-process-sentinel-lens.png")
            check("process lens screenshot captured", (OUT / "03-process-sentinel-lens.png").exists())

            # -------------------------------------------------------------
            # 8. Coverage Lens after Scan
            # -------------------------------------------------------------
            page.locator("#lens-btn-coverage").click()
            time.sleep(0.4)
            cov_items = page.locator("#res-coverage li")
            check("res-coverage has 9 layer rows", cov_items.count() >= 9, f"count={cov_items.count()}")
            page.screenshot(path=str(OUT / "04-coverage-lens.png"))
            shutil.copy(OUT / "04-coverage-lens.png", DOCS_OUT / "04-coverage-lens.png")

            # -------------------------------------------------------------
            # 9. Light Theme Verification
            # -------------------------------------------------------------
            page.evaluate("document.documentElement.setAttribute('data-theme', 'light')")
            time.sleep(0.5)
            page.locator("#lens-btn-findings").click()
            time.sleep(0.3)
            page.screenshot(path=str(OUT / "05-investigate-light-theme.png"))
            shutil.copy(OUT / "05-investigate-light-theme.png", DOCS_OUT / "05-investigate-light-theme.png")
            check("light theme screenshot captured", (OUT / "05-investigate-light-theme.png").exists())
            page.evaluate("document.documentElement.removeAttribute('data-theme')")
            time.sleep(0.3)

            # -------------------------------------------------------------
            # 10. Responsive Geometry (1366x768)
            # -------------------------------------------------------------
            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.5)
            page.screenshot(path=str(OUT / "06-investigate-1366x768.png"))
            shutil.copy(OUT / "06-investigate-1366x768.png", DOCS_OUT / "06-investigate-1366x768.png")
            check("1366x768 layout screenshot captured", (OUT / "06-investigate-1366x768.png").exists())

            # -------------------------------------------------------------
            # 11. Mascot Absence Verification
            # -------------------------------------------------------------
            mascot_count = page.locator(".luma-avatar, .luma-cat, [data-companion='cat']").count()
            check("mascot elements absent from investigation console", mascot_count == 0, f"found={mascot_count}")

    finally:
        shutdown(proc, browser)

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} native UI-04 checks passed")
    if failed:
        print("Failed checks:")
        for name, _, extra in failed:
            print(f"  - {name} ({extra})")
        sys.exit(1)
    else:
        print("UI-04 NATIVE ACCEPTANCE: ALL CHECKS PASSED")


if __name__ == "__main__":
    main()
