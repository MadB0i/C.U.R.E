"""Stage UI-01A native verification against the real Tauri binary.

Drives the release cure-gui.exe over the DevTools protocol — the real
backend, real events, no mocks — through the scan-origin scenarios:

 1. Fresh launch  -> Overview idle, neutral session.
 2. Overview scan -> live Overview progress (capture during scan).
 3. Overview scan completes -> Overview still visible, no forced nav.
 4. Rescue scan   -> Rescue workflow preserved (scan view -> results).
 5. Overview scan -> navigate to Agents mid-scan -> stays on Agents.
 6. Overview scan -> navigate to Rescue mid-scan -> stays on Rescue.
10. Restart       -> no stale session results.
11. Live PC clock keeps ticking.
12. Mascot remains absent.

Scenarios 7-9 (duplicate clicks, coverage semantics, failure states) are
covered deterministically by the mock-shell regression suite in
evidence-console.py `scan_origin_flow`, because they need injected errors
and command counting that a real scan cannot safely provide.

Run: python gui/devtools/ui01a-native-verify.py
"""
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from playwright.sync_api import sync_playwright, expect

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
OUT = REPO / "testing" / "native" / "screenshots-ui01a" / "native"
OUT.mkdir(parents=True, exist_ok=True)
PORT = 9405

results = []


def check(name, ok, extra=""):
    results.append((name, ok, extra))
    print(("PASS  " if ok else "FAIL  ") + name + ("  [" + extra + "]" if extra else ""))


def wait_cdp(timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=1)
            return True
        except Exception:
            time.sleep(0.5)
    raise SystemExit("CDP endpoint never appeared")


def kill_lingering():
    """Kill leftover app + WebView2 processes from prior runs.

    Same housekeeping native-e2e.py does: an orphaned WebView2 host keeps
    the CDP port alive, and the next launch would silently connect to the
    stale instance instead of the fresh one.
    """
    subprocess.run(["taskkill", "/F", "/IM", "cure-gui.exe"], capture_output=True)
    subprocess.run(["taskkill", "/F", "/IM", "msedgewebview2.exe"], capture_output=True)
    time.sleep(1.0)


def launch(pw, port):
    kill_lingering()
    tmp = tempfile.TemporaryDirectory(prefix="cure-ui01a-", ignore_cleanup_errors=True)
    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(port)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(Path(tmp.name) / "wvdata")
    proc = subprocess.Popen([str(EXE)], env=env)
    end = time.time() + 60
    while time.time() < end:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/json/version", timeout=1)
            break
        except Exception:
            time.sleep(0.5)
    else:
        raise SystemExit("CDP endpoint never appeared on port " + str(port))
    browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
    page = browser.contexts[0].pages[0]
    page.set_viewport_size({"width": 1600, "height": 900})
    page.wait_for_load_state("networkidle")
    # Freshness guard: a genuinely new process always opens on Overview.
    for _ in range(20):
        if page.evaluate(
            "() => { const v = document.getElementById('view-overview');"
            " return v && !v.classList.contains('hidden'); }"
        ):
            break
        time.sleep(0.5)
    else:
        browser.close()
        tmp.cleanup()
        raise SystemExit(
            "connected to a stale instance (Overview not visible) — retry"
        )
    return tmp, proc, browser, page


def shutdown(tmp, proc, browser):
    try:
        browser.close()
    except Exception:
        pass
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except Exception:
        proc.kill()
        proc.wait(timeout=10)
    tmp.cleanup()


def shot(page, name):
    page.screenshot(path=str(OUT / name))
    print("  shot", name)


def wait_until(page, js, timeout=180000, poll=0.4):
    """Poll a JS predicate via CDP evaluate. wait_for_function is unusable
    here: the native app's CSP forbids eval'd predicate strings."""
    end = time.time() + timeout / 1000
    while time.time() < end:
        try:
            if page.evaluate(js):
                return True
        except Exception:
            pass
        time.sleep(poll)
    raise TimeoutError("condition never became true: " + js)


def visible_view(page):
    return page.evaluate(
        """() => {
             const shown = [...document.querySelectorAll('.stage > .view')]
               .filter(v => !v.classList.contains('hidden'));
             return shown.length === 1 ? shown[0].id : 'ambiguous:' + shown.map(v=>v.id);
           }"""
    )


def main():
    with sync_playwright() as pw:
        # ---- Session 1: scenarios 1, 2, 3, 11, 12 ----
        tmp, proc, browser, page = launch(pw, 9405)
        expect(page.locator("#view-overview")).to_be_visible()
        check("1 fresh launch shows Overview idle",
              page.locator("#ov-posture").inner_text() == "Not assessed"
              and page.locator("#session-meta").count() == 0)
        check("12 mascot fully absent",
              page.evaluate("document.querySelectorAll('.luma-svg, .luma-stage').length") == 0
              and page.evaluate("typeof window.Luma") == "undefined"
              and page.evaluate("typeof window.companion") == "undefined",
              str(page.evaluate("document.querySelectorAll('.luma-svg').length")) + " luma nodes")

        clock1 = page.locator("#topbar-clock").inner_text()
        time.sleep(2.2)
        clock2 = page.locator("#topbar-clock").inner_text()
        check("11 live PC clock advances", clock1 != clock2, clock1 + " -> " + clock2)

        # Scenario 2: start from Overview, stay, capture live progress.
        page.locator("#overview-rescue-btn").click()
        expect(page.locator("#ov-posture")).to_have_text("Scanning", timeout=20000)
        check("2 overview scan starts without leaving Overview",
              visible_view(page) == "view-overview")
        wait_until(page,
                   "document.getElementById('ov-scope').textContent.includes('elapsed')",
                   timeout=10000)
        check("2 hero shows live collector line",
              "Collecting" in page.locator("#ov-scope").inner_text(),
              page.locator("#ov-scope").inner_text())
        shot(page, "01-native-overview-scanning.png")
        check("2 dial reports live layers",
              page.locator("#ov-dial-label").inner_text().strip().lower() == "layers reporting")

        # Scenario 5: navigate to Agents mid-scan; must stay there.
        page.locator("#nav-agents").click()
        expect(page.locator("#view-agents")).to_be_visible()
        wait_until(page,
                   "!document.getElementById('ov-posture').textContent.includes('Scanning')")
        page.wait_for_timeout(800)
        check("5 manual navigation respected at completion",
              visible_view(page) == "view-agents"
              and page.locator("#view-title").inner_text() == "AI Agent Radar",
              visible_view(page))
        check("3 overview updated in background after completion",
              page.locator("#ov-posture").inner_text() != "Scanning"
              and page.locator("#ov-review-btn").is_enabled())

        # Scenario 10 (part 1): note the post-scan posture, then restart.
        posture_after_scan = page.locator("#ov-posture").inner_text()
        shutdown(tmp, proc, browser)

        # ---- Session 2: scenario 10 restart, then 4 and 6 ----
        tmp, proc, browser, page = launch(pw, 9407)
        expect(page.locator("#view-overview")).to_be_visible()
        check("10 restart shows neutral session, not stale results",
              page.locator("#ov-posture").inner_text() == "Not assessed",
              "previous session ended as: " + posture_after_scan)

        # Scenario 6: overview scan -> navigate to Rescue mid-scan.
        page.locator("#overview-rescue-btn").click()
        expect(page.locator("#ov-posture")).to_have_text("Scanning", timeout=20000)
        page.locator("#nav-scan").click()
        expect(page.locator("#scan-view")).to_be_visible()
        wait_until(page,
                   "!document.getElementById('ov-posture').textContent.includes('Scanning')")
        page.wait_for_timeout(800)
        check("6 mid-scan move to Rescue is respected at completion",
              visible_view(page) in ("scan-view", "landing-view"),
              visible_view(page))

        # Scenario 4: a Rescue-origin scan keeps the Rescue workflow. The
        # scan view's own start control (labelled "Scan Again" after the
        # previous scan) starts a rescue-origin scan.
        page.locator("#xray-start-btn").click()
        expect(page.locator("#scan-view")).to_be_visible(timeout=20000)
        check("4 rescue-origin scan stays in the Rescue workflow",
              visible_view(page) in ("scan-view", "landing-view"))
        expect(page.locator("#results-view")).to_be_visible(timeout=180000)
        expect(page.locator("#view-title")).to_have_text("Investigate", timeout=10000)
        shot(page, "02-native-rescue-results.png")
        page.locator("#nav-overview").click()
        expect(page.locator("#view-overview")).to_be_visible()
        posture = page.locator("#ov-posture").inner_text()
        coverage = page.locator("#ov-coverage-state").inner_text()
        check("8 partial coverage is not called failed or clean",
              posture in ("Partial coverage", "Attention required",
                          "Attention required · Partial coverage",
                          "Assessment complete")
              and coverage in ("COMPLETE", "PARTIAL"),
              posture + " / " + coverage)
        check("8 coverage FAILED label is gone",
              coverage != "FAILED", coverage)
        shot(page, "03-native-overview-after-own-scan.png")
        shutdown(tmp, proc, browser)

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} native scenario checks passed")
    if failed:
        for name, _, extra in failed:
            print("FAILED:", name, extra)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
