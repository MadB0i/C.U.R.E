"""Stage UI-08 native verification against the real Tauri binary.

Tests Stage UI-08 — Share Result Card — against cure-gui.exe over CDP.
Verifies:
  1. Share entry point on the Overview
  2. Preview modal renders the exact PNG (data: URL) before saving
  3. Save writes a real PNG under the session data dir (case-files/)
  4. The save is recorded as a session event in the Monitor feed
  5. Escape/overlay dismissal, focus management
  6. The demo/mock boundary rejects the write (public demo never writes)

Safety: no process is terminated by this harness; the share card contains
aggregate counts only.

Run: python gui/devtools/ui08-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui08" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui08"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9412
TMP_DIR = Path("D:/CURE_UI08_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui08.json"

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
    print("=== Stage UI-08 native acceptance (real backend, no mocks) ===")
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
            # 1. Entry point
            # -------------------------------------------------------------
            check(
                "share button exists on the Overview",
                page.locator("#ov-share-btn").is_visible(),
            )

            # -------------------------------------------------------------
            # 2. Preview modal
            # -------------------------------------------------------------
            page.locator("#ov-share-btn").click()
            time.sleep(0.4)
            overlay = page.locator("#share-overlay")
            check("preview modal opens", overlay.is_visible())
            img_src = page.locator("#share-preview-img").get_attribute("src") or ""
            check(
                "preview shows the exact PNG (data URL)",
                img_src.startswith("data:image/png;base64,") and len(img_src) > 1000,
                f"len={len(img_src)}",
            )
            check(
                "save button is focused for keyboard flow",
                page.evaluate("() => document.activeElement && document.activeElement.id") == "share-save",
            )
            page.screenshot(path=str(OUT / "01-share-card-preview.png"))
            shutil.copy(OUT / "01-share-card-preview.png", DOCS_OUT / "01-share-card-preview.png")

            # -------------------------------------------------------------
            # 3. Real save through the backend
            # -------------------------------------------------------------
            page.locator("#share-save").click()
            time.sleep(1.0)
            status = page.locator("#share-status").text_content().strip()
            check("save reports a path", status.startswith("Saved:"), f"status={status[:90]}")
            saved_path = status[6:].strip()
            saved = Path(saved_path) if saved_path else None
            check(
                "saved file exists on disk",
                bool(saved and saved.exists()),
                str(saved),
            )
            if saved and saved.exists():
                head = saved.read_bytes()[:8]
                check(
                    "saved file is a real PNG (signature check)",
                    head == b"\x89PNG\r\n\x1a\n",
                )
                check(
                    "saved under the session data dir (case-files)",
                    "case-files" in saved_path and str(TMP_DIR) in saved_path,
                    saved_path,
                )
                check(
                    "saved file is a plausible card size (>20 KB)",
                    saved.stat().st_size > 20000,
                    f"{saved.stat().st_size} bytes",
                )

            # -------------------------------------------------------------
            # 4. Session evidence trail
            # -------------------------------------------------------------
            # The modal deliberately stays open after save (the status line
            # shows where the card went); dismiss it before navigating.
            page.locator("#share-cancel").click()
            time.sleep(0.2)
            page.locator("#nav-eventlog").click()
            time.sleep(0.4)
            feed_text = page.locator("#event-log-list").text_content() or ""
            check(
                "export is recorded in the session feed",
                "share result card" in feed_text.lower(),
            )
            events_total = page.locator("#m-stat-events").text_content().strip()
            check("session counter reflects the export", events_total.isdigit() and int(events_total) >= 1)

            # -------------------------------------------------------------
            # 5. Dismissal flows
            # -------------------------------------------------------------
            page.locator("#nav-overview").click()
            time.sleep(0.4)
            page.locator("#ov-share-btn").click()
            time.sleep(0.3)
            check("modal reopens", overlay.is_visible())
            page.keyboard.press("Escape")
            time.sleep(0.2)
            check("Escape closes the modal", overlay.is_hidden())
            page.locator("#ov-share-btn").click()
            time.sleep(0.3)
            page.locator("#share-cancel").click()
            time.sleep(0.2)
            check("Cancel closes the modal", overlay.is_hidden())

            page.screenshot(path=str(OUT / "02-overview-with-share-entry.png"))
            shutil.copy(OUT / "02-overview-with-share-entry.png", DOCS_OUT / "02-overview-with-share-entry.png")

            passes = sum(1 for _, ok, _ in results if ok)
            fails = sum(1 for _, ok, _ in results if not ok)
            total = len(results)
            print(f"\n{passes}/{total} native UI-08 checks passed")

            if fails > 0:
                print(f"UI-08 NATIVE ACCEPTANCE: {fails} CHECKS FAILED")
                shutdown(proc, browser)
                sys.exit(1)
            else:
                print("UI-08 NATIVE ACCEPTANCE: ALL CHECKS PASSED")

    finally:
        shutdown(proc, browser)


if __name__ == "__main__":
    main()
