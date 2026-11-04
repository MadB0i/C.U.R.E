"""Stage UI-07 native verification against the real Tauri binary.

Tests Stage UI-07 — Detective Mode polish — against cure-gui.exe over CDP.
Verifies:
  1. Case telemetry strip (4 monospace cells: measured total, review
     candidates, files measured, discoveries), honest em-dashes at idle
  2. Mascot/operator-mount removal from the detective view
  3. Real quick storage investigation started from the board, strip and
     map populated from the same measurement
  4. Zero emojis, honest empty states, monospace figures
  5. Dark & light theme aesthetics, responsive at 1366x768 and 1920x1080

Safety: the storage scan is read-only by design (nothing under storage
removes a file). No process is terminated by this harness.

Run: python gui/devtools/ui07-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui07" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui07"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9411
TMP_DIR = Path("D:/CURE_UI07_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui07.json"

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
    print("=== Stage UI-07 native acceptance (real backend, no mocks) ===")
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
            # 1. Structure: head, strip at idle
            # -------------------------------------------------------------
            nav = page.locator("#nav-detective")
            check("nav-detective button exists", nav.count() > 0)
            nav.click()
            time.sleep(0.6)

            view = page.locator("#view-detective")
            check("view-detective is visible after navigation", view.is_visible())
            check(
                "headline text is correct",
                "Where did my storage go?" in (page.locator("#view-detective .headline").text_content() or ""),
            )
            check(
                "idle subline stays honest",
                "Nothing is measured" in (page.locator("#dt-subline").text_content() or ""),
            )

            strip = page.locator("#dt-stats-strip")
            check("case telemetry strip exists", strip.count() > 0)
            for cell in ("dt-stat-total", "dt-stat-candidates", "dt-stat-files", "dt-stat-leads"):
                el = page.locator("#" + cell)
                check(f"stat cell #{cell} exists", el.count() > 0)
                check(
                    f"#{cell} shows an honest em-dash before any measurement",
                    el.text_content().strip() == "—",
                )

            # Screenshot: idle board
            page.screenshot(path=str(OUT / "01-detective-idle.png"))
            shutil.copy(OUT / "01-detective-idle.png", DOCS_OUT / "01-detective-idle.png")

            # -------------------------------------------------------------
            # 2. Purity: mascot removed, no emojis
            # -------------------------------------------------------------
            mascots = page.locator("#view-detective [data-companion], #view-detective .mascot, #view-detective .luma")
            check("mascot elements absent from detective view", mascots.count() == 0, f"found={mascots.count()}")
            text = view.text_content() or ""
            emojis = ["🔒", "🛡️", "🐱", "🤖", "🚀", "✕", "✔", "⚠", "🕵"]
            found = [e for e in emojis if e in text]
            check("no emojis present in detective view", len(found) == 0, f"found={found}")

            # -------------------------------------------------------------
            # 3. Real investigation from the board (read-only quick scan)
            # -------------------------------------------------------------
            btn = page.locator("#dt-investigate")
            check("start-investigation button exists", btn.is_visible())
            btn.click()
            deadline = time.time() + 240
            done = False
            while time.time() < deadline:
                if page.locator("#dt-rescan").is_visible():
                    done = True
                    break
                time.sleep(1.0)
            check("real investigation completes (Measure again appears)", done)

            total_val = page.locator("#dt-stat-total").text_content().strip()
            check("measured-total stat populated from the real scan", total_val not in ("", "—"), f"total={total_val}")
            files_raw = page.locator("#dt-stat-files").text_content().strip()
            check("files-measured stat is a number", files_raw.isdigit(), f"files={files_raw}")
            leads_raw = page.locator("#dt-stat-leads").text_content().strip()
            check("discoveries stat is a number", leads_raw.isdigit(), f"leads={leads_raw}")
            cand_raw = page.locator("#dt-stat-candidates").text_content().strip()
            check(
                "candidates stat populated or honest 'None found'",
                cand_raw not in ("", "—"),
                f"candidates={cand_raw}",
            )
            check(
                "map core total matches the strip",
                page.locator("#dt-map-total").text_content().strip() == total_val,
            )
            check(
                "board rendered discovery list or honest empty",
                page.locator("#dt-leads li").count() > 0 or page.locator("#dt-empty").is_visible(),
            )
            check(
                "strip figures use monospace tabular numerals",
                "cascadia" in page.locator("#dt-stat-total").evaluate(
                    "el => getComputedStyle(el).fontFamily"
                ).lower()
                or "consolas" in page.locator("#dt-stat-total").evaluate(
                    "el => getComputedStyle(el).fontFamily"
                ).lower(),
            )

            # Screenshot: measured board
            page.screenshot(path=str(OUT / "02-detective-measured.png"))
            shutil.copy(OUT / "02-detective-measured.png", DOCS_OUT / "02-detective-measured.png")

            # -------------------------------------------------------------
            # 4. Theme + responsive
            # -------------------------------------------------------------
            page.evaluate("document.documentElement.setAttribute('data-theme', 'light')")
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "03-detective-light-theme.png"))
            shutil.copy(OUT / "03-detective-light-theme.png", DOCS_OUT / "03-detective-light-theme.png")
            page.evaluate("document.documentElement.setAttribute('data-theme', 'dark')")
            time.sleep(0.2)

            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "04-detective-1366x768.png"))
            shutil.copy(OUT / "04-detective-1366x768.png", DOCS_OUT / "04-detective-1366x768.png")

            page.set_viewport_size({"width": 1920, "height": 1080})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "05-detective-1920x1080.png"))
            shutil.copy(OUT / "05-detective-1920x1080.png", DOCS_OUT / "05-detective-1920x1080.png")

            passes = sum(1 for _, ok, _ in results if ok)
            fails = sum(1 for _, ok, _ in results if not ok)
            total = len(results)
            print(f"\n{passes}/{total} native UI-07 checks passed")

            if fails > 0:
                print(f"UI-07 NATIVE ACCEPTANCE: {fails} CHECKS FAILED")
                shutdown(proc, browser)
                sys.exit(1)
            else:
                print("UI-07 NATIVE ACCEPTANCE: ALL CHECKS PASSED")

    finally:
        shutdown(proc, browser)


if __name__ == "__main__":
    main()
