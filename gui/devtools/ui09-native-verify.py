"""Stage UI-09 native verification against the real Tauri binary.

Tests Stage UI-09 — Move Bot (one-click C: → D: relocation) and the
human-language navigation pass — against cure-gui.exe over CDP.

Native scope (read-only by design):
  1. Move Bot entries exist (Rescue action bar + Detective head)
  2. Opening the bot runs the REAL status + plan engines: the confirmation
     card lists the machine's real safe folders and sizes, in plain words
  3. Cancel closes the bot and moves nothing
  4. The manual multi-step flow still exists beside the bot
  5. Human-language titles: "Running programs", "System changes",
     "Ransomware signs", "What was checked", "Move it for me — one click"
  6. Dark + light aesthetics, responsive 1366x768

The native harness deliberately stops at the confirmation gate: executing
the move would relocate the real machine's personal folders. The full
copy → verify → switch → cleanup journey is verified end-to-end in the
labelled mock shell (ui09-mock-flow-verify.py) driving the exact same
engine commands the real backend exposes.

Safety: no process is terminated by this harness.

Run: python gui/devtools/ui09-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui09" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui09"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9413
TMP_DIR = Path("D:/CURE_UI09_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui09.json"

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
    print("=== Stage UI-09 native acceptance (real backend, no mocks) ===")
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
            # 1. Human-language navigation titles
            # -------------------------------------------------------------
            page.locator("#nav-results").click()
            time.sleep(0.5)
            body = page.locator("#results-view").text_content() or ""
            for title in ("Running programs", "System changes", "Ransomware signs", "What was checked"):
                check(f'human title "{title}" present in Investigate', title in body)
            check(
                "old jargon titles are gone",
                "Running Process Sentinel" not in body
                and "Nine-Layer Inspection Coverage" not in body
                and "Trusted Snapshot Comparison" not in body,
            )

            # -------------------------------------------------------------
            # 2. Move Bot entry in Rescue + real plan through the bot
            # -------------------------------------------------------------
            page.locator("#nav-cleanup").click()
            page.locator("#sw-tab-rescue").click()
            page.wait_for_timeout(500)
            time.sleep(0.6)
            bot_btn = page.locator("#sr-btn-move-bot")
            check("Rescue one-click bot button exists", bot_btn.is_visible())
            check(
                "bot button speaks human",
                "Move it for me" in (bot_btn.text_content() or ""),
            )
            check(
                "manual multi-step flow still available",
                page.locator("#sr-btn-build-plan").is_visible(),
            )
            check(
                "action bar speaks human",
                "Ready to move:" in (page.locator(".sr-action-bar").text_content() or ""),
            )

            bot_btn.click()
            time.sleep(0.3)
            overlay = page.locator("#mb-overlay")
            check("move bot overlay opens", overlay.is_visible())
            # Real engines decide; the card either lists real safe folders or
            # honestly says there is nothing safe to move.
            deadline = time.time() + 30
            ready = False
            while time.time() < deadline:
                if page.locator("#mb-confirm-go").is_enabled():
                    ready = True
                    break
                if "nothing I can safely move" in (page.locator("#mb-confirm-body").text_content() or ""):
                    ready = True
                    break
                time.sleep(0.4)
            check("bot finished its real drive check (plan ready or honest empty)", ready)

            confirm_text = page.locator("#mb-confirm-body").text_content() or ""
            has_items = page.locator("#mb-confirm-items .mb-item-row").count() > 0
            honest_empty = "nothing I can safely move" in confirm_text
            check(
                "confirmation card lists real folders or states honestly there are none",
                has_items or honest_empty,
            )
            if has_items:
                check(
                    "confirmation card explains the promise in plain words",
                    "Nothing is deleted" in confirm_text and "you can undo" in confirm_text,
                )
                check(
                    "cleanup option is offered and defaults to OFF",
                    page.locator("#mb-cleanup-box").is_visible()
                    and not page.locator("#mb-cleanup-tick").is_checked(),
                )
            page.screenshot(path=str(OUT / "01-move-bot-confirm-real.png"))
            shutil.copy(OUT / "01-move-bot-confirm-real.png", DOCS_OUT / "01-move-bot-confirm-real.png")

            # Cancel: the native run must not move anything.
            page.locator("#mb-confirm-cancel").click()
            time.sleep(0.3)
            check("cancel closes the bot", overlay.is_hidden())

            # -------------------------------------------------------------
            # 3. Detective entry
            # -------------------------------------------------------------
            page.locator("#nav-detective").click()
            time.sleep(0.6)
            dt_btn = page.locator("#dt-move-bot")
            check("Detective bot entry exists", dt_btn.is_visible())
            check(
                "Detective entry speaks human",
                "Free up C: drive" in (dt_btn.text_content() or ""),
            )
            dt_btn.click()
            time.sleep(0.4)
            check("Detective entry opens the same move bot", overlay.is_visible())
            page.locator("#mb-confirm-cancel").click()
            time.sleep(0.3)
            check("bot closes from Detective entry too", overlay.is_hidden())

            # -------------------------------------------------------------
            # 4. Aesthetics: light theme + responsive
            # -------------------------------------------------------------
            page.locator("#nav-cleanup").click()
            page.locator("#sw-tab-rescue").click()
            page.wait_for_timeout(500)
            time.sleep(0.5)
            page.evaluate("document.documentElement.setAttribute('data-theme', 'light')")
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "02-rescue-light-theme.png"))
            shutil.copy(OUT / "02-rescue-light-theme.png", DOCS_OUT / "02-rescue-light-theme.png")
            page.evaluate("document.documentElement.setAttribute('data-theme', 'dark')")
            time.sleep(0.2)

            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "03-rescue-1366x768.png"))
            shutil.copy(OUT / "03-rescue-1366x768.png", DOCS_OUT / "03-rescue-1366x768.png")

            passes = sum(1 for _, ok, _ in results if ok)
            fails = sum(1 for _, ok, _ in results if not ok)
            total = len(results)
            print(f"\n{passes}/{total} native UI-09 checks passed")

            if fails > 0:
                print(f"UI-09 NATIVE ACCEPTANCE: {fails} CHECKS FAILED")
                shutdown(proc, browser)
                sys.exit(1)
            else:
                print("UI-09 NATIVE ACCEPTANCE: ALL CHECKS PASSED")

    finally:
        shutdown(proc, browser)


if __name__ == "__main__":
    main()
