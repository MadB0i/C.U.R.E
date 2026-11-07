"""Stage UI-06 native verification against the real Tauri binary.

Tests Stage UI-06 — Monitor Workstation (session feed), Canary Tripwire,
and the Login Observation chamber — against cure-gui.exe over CDP.
Verifies:
  1. Monitor Workstation shell, header, telemetry strip, rail navigation
  2. Session event feed: empty state, live counters, kind filters
  3. Login Observation end-to-end against the REAL backend (15 s window):
     verdict, coverage, timeline, inspect drawer, export buttons
  4. Windows Change Radar reachable as an Investigate lens (moved out of
     the removed secondary view) with a real capture/compare round-trip
  5. Ransom lens cross-link into the observation chamber
  6. Strict absence of emojis and mascot elements in the three views
  7. Dark & light theme aesthetics, responsive at 1366x768 and 1920x1080

Safety: this harness never terminates a process and never touches the
canary guard toggle (the real guard plants decoy files in user folders —
that path is exercised by the interactive suite against the mock shell
instead). The only processes involved are the app itself and the OS
process listing it already displays.

Run: python gui/devtools/ui06-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui06" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui06"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9410
TMP_DIR = Path("D:/CURE_UI06_TMP")

sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui06.json"

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


def open_monitor(page):
    page.locator("#nav-eventlog").click()
    time.sleep(0.4)


def main():
    print("=== Stage UI-06 native acceptance (real backend, no mocks) ===")
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
            # 1. Monitor Workstation structure
            # -------------------------------------------------------------
            nav = page.locator("#nav-eventlog")
            check("nav-eventlog button exists", nav.count() > 0)
            open_monitor(page)
            view_m = page.locator("#view-eventlog")
            check("view-eventlog is visible after navigation", view_m.is_visible())

            shell = page.locator("#view-eventlog .m-shell")
            check("m-shell container exists", shell.count() > 0)

            headline = page.locator("#view-eventlog .headline")
            check(
                "headline text is correct",
                "Monitor Workstation" in (headline.text_content() or ""),
            )

            strip = page.locator("#m-stats-strip")
            check("telemetry stats strip exists", strip.count() > 0)
            for cell in ("m-stat-events", "m-stat-canary", "m-stat-actions", "m-stat-last"):
                check(f"stat cell #{cell} exists", page.locator("#" + cell).count() > 0)

            check(
                "events counter starts at 0",
                page.locator("#m-stat-events").text_content().strip() == "0",
            )
            check(
                "last-event counter starts em-dash",
                page.locator("#m-stat-last").text_content().strip() == "—",
            )

            # -------------------------------------------------------------
            # 2. Rail navigation (Session events / Canary / Login observation)
            # -------------------------------------------------------------
            rail_btns = page.locator("#view-eventlog .m-rail-btn")
            check("monitor rail has 3 destinations", rail_btns.count() == 3)
            check(
                "rail routes are eventlog, canary, incident",
                sorted(
                    [
                        rail_btns.nth(i).get_attribute("data-route")
                        for i in range(rail_btns.count())
                    ]
                )
                == ["view-canary", "view-eventlog", "view-incident"],
            )
            check(
                "session-events rail tab is current",
                rail_btns.nth(0).get_attribute("aria-current") == "true",
            )

            # -------------------------------------------------------------
            # 3. Feed: empty state, filters, clear
            # -------------------------------------------------------------
            check(
                "feed empty state visible on fresh launch",
                page.locator("#event-log-empty").is_visible(),
            )
            filters = page.locator("#view-eventlog .filter-btn[data-ef]")
            check("4 feed filter chips exist", filters.count() == 4)
            check(
                "feed list is empty on fresh launch",
                page.locator("#event-log-list li").count() == 0,
            )
            check(
                "clear-view button exists",
                page.locator("#log-clear").is_visible(),
            )

            # -------------------------------------------------------------
            # 4. Login Observation — REAL 15 s backend observation
            # -------------------------------------------------------------
            page.locator("#view-eventlog [data-route='view-incident']").click()
            time.sleep(0.4)
            view_inc = page.locator("#view-incident")
            check("view-incident is visible via monitor rail", view_inc.is_visible())

            inc_headline = page.locator("#view-incident .headline")
            check(
                "observation headline is correct",
                "Login Observation" in (inc_headline.text_content() or ""),
            )
            durations = page.locator("#view-incident .filter-btn[data-dur]")
            check("4 observation durations offered", durations.count() == 4)
            start_btn = page.locator("#btn-incident-start")
            check("start-observation button exists", start_btn.is_visible())

            # Screenshot: idle observation chamber
            page.screenshot(path=str(OUT / "01-login-observation-idle.png"))
            shutil.copy(OUT / "01-login-observation-idle.png", DOCS_OUT / "01-login-observation-idle.png")

            page.locator("#view-incident .filter-btn[data-dur='15']").click()
            start_btn.click()
            # 15 s real observation; allow generous completion margin.
            expect_target = page.locator("#inc-results")
            deadline = time.time() + 40
            visible = False
            while time.time() < deadline:
                if expect_target.is_visible():
                    visible = True
                    break
                time.sleep(0.5)
            check("incident results render after real observation", visible)

            verdict = page.locator("#inc-verdict").text_content().strip()
            check("verdict badge is populated", verdict not in ("", "—"), f"verdict={verdict}")
            check(
                "verdict text is honest vocabulary",
                verdict
                in (
                    "CAUSE IDENTIFIED",
                    "STRONG CORRELATION",
                    "REVIEW REQUIRED",
                    "NO DIRECT EVIDENCE",
                    "INSUFFICIENT OBSERVATION",
                ),
                f"verdict={verdict}",
            )
            check(
                "scope coverage rows rendered",
                page.locator("#inc-scope li").count() >= 3,
            )
            check(
                "timeline rows rendered",
                page.locator("#incident-timeline li").count() > 0,
            )
            check(
                "export TXT button exists",
                page.locator("#exp-inc-txt").is_visible(),
            )
            check(
                "export JSON button exists",
                page.locator("#exp-inc-json").is_visible(),
            )

            # Inspect drawer over real observed process evidence.
            inspect = page.locator("#view-incident button").filter(has_text="Inspect").first
            if inspect.count() > 0:
                inspect.click()
                time.sleep(0.3)
                check(
                    "inspect drawer opens over observed evidence",
                    page.locator("#inc-drawer-overlay").is_visible(),
                )
                page.keyboard.press("Escape")
                time.sleep(0.2)
            else:
                check("inspect drawer button present when processes observed", False, "no Inspect buttons")

            # Screenshot: real observation results
            page.screenshot(path=str(OUT / "02-login-observation-results.png"))
            shutil.copy(OUT / "02-login-observation-results.png", DOCS_OUT / "02-login-observation-results.png")

            # -------------------------------------------------------------
            # 5. Feed live update + kind filters (events came from the run)
            # -------------------------------------------------------------
            open_monitor(page)
            events_total = int(page.locator("#m-stat-events").text_content().strip())
            check("session counter updated by the observation", events_total > 0, f"count={events_total}")
            actions_total = int(page.locator("#m-stat-actions").text_content().strip())
            check("actions counter updated by the observation", actions_total > 0, f"count={actions_total}")

            rows_all = page.locator("#event-log-list li").count()
            check("feed rows rendered", rows_all > 0, f"rows={rows_all}")

            page.locator("#view-eventlog .filter-btn[data-ef='canary']").click()
            time.sleep(0.2)
            check(
                "canary filter shows zero rows (no canary events occurred)",
                page.locator("#event-log-list li").count() == 0,
            )
            page.locator("#view-eventlog .filter-btn[data-ef='action']").click()
            time.sleep(0.2)
            rows_actions = page.locator("#event-log-list li").count()
            check("action filter shows the run's action rows", rows_actions > 0, f"rows={rows_actions}")
            check(
                "action rows carry the Action tag",
                page.locator("#event-log-list .m-tag-action").count() == rows_actions,
            )
            page.locator("#view-eventlog .filter-btn[data-ef='all']").click()
            time.sleep(0.2)
            check("all filter restores full feed", page.locator("#event-log-list li").count() == rows_all)

            page.screenshot(path=str(OUT / "03-monitor-session-feed.png"))
            shutil.copy(OUT / "03-monitor-session-feed.png", DOCS_OUT / "03-monitor-session-feed.png")

            # -------------------------------------------------------------
            # 6. Canary tripwire view (structure only — guard not started)
            # -------------------------------------------------------------
            page.locator("#view-eventlog [data-route='view-canary']").click()
            time.sleep(0.4)
            view_c = page.locator("#view-canary")
            check("view-canary is visible via monitor rail", view_c.is_visible())
            check(
                "canary headline is correct",
                "Canary Tripwire" in (page.locator("#view-canary .headline").text_content() or ""),
            )
            check(
                "guard state badge starts OFF",
                page.locator("#can-state").text_content().strip() == "OFF",
            )
            check(
                "guard toggle visible",
                page.locator("#can-toggle").is_visible(),
            )
            check(
                "experimental disclosure keeps honest wording",
                "not full ransomware protection" in (view_c.text_content() or ""),
            )
            check(
                "canary alerts feed present",
                page.locator("#can-alerts").count() > 0,
            )
            page.screenshot(path=str(OUT / "04-canary-tripwire.png"))
            shutil.copy(OUT / "04-canary-tripwire.png", DOCS_OUT / "04-canary-tripwire.png")

            # -------------------------------------------------------------
            # 7. Windows Change Radar — real capture/compare inside the lens
            # -------------------------------------------------------------
            page.locator("#nav-results").click()
            time.sleep(0.4)
            page.locator("#lens-btn-winchanges").click()
            time.sleep(0.3)
            check(
                "winchanges lens panel visible",
                page.locator("#lens-panel-winchanges").is_visible(),
            )
            check(
                "radar controls live inside the lens",
                page.locator("#lens-panel-winchanges #ar-win-capture").count() == 1
                and page.locator("#lens-panel-winchanges #ar-win-compare").count() == 1,
            )
            before = page.locator("#ar-winsnap-state").text_content()
            page.locator("#ar-win-capture").click()
            time.sleep(1.0)
            after = page.locator("#ar-winsnap-state").text_content()
            check(
                "real capture records a trusted snapshot",
                "stored yet" not in after and after != before,
                f"state={after.strip()[:80]}",
            )
            page.locator("#ar-win-compare").click()
            time.sleep(1.0)
            check(
                "real compare renders the diff summary",
                page.locator("#ar-win-summary").is_visible(),
            )
            check(
                "differences stay review leads, not verdicts",
                "not malware verdicts" in (page.locator("#ar-win-note").text_content() or ""),
            )

            # -------------------------------------------------------------
            # 8. Ransom lens cross-link into the observation chamber
            # -------------------------------------------------------------
            page.locator("#lens-btn-ransom").click()
            time.sleep(0.3)
            cta = page.locator("#ransom-observe-cta")
            check("ransom lens cross-link exists", cta.is_visible())
            cta.locator("button[data-route='view-incident']").click()
            time.sleep(0.4)
            check(
                "cross-link lands on the observation chamber",
                page.locator("#view-incident").is_visible(),
            )

            # -------------------------------------------------------------
            # 9. Purity gates: no emojis, no mascots, mono counters
            # -------------------------------------------------------------
            for vid in ("view-eventlog", "view-canary", "view-incident"):
                text = page.locator("#" + vid).text_content() or ""
                emojis = ["🔒", "🛡️", "↩️", "🐱", "🤖", "🚀", "✕", "✔", "⚠"]
                found = [e for e in emojis if e in text]
                check(f"no emojis present in {vid}", len(found) == 0, f"found={found}")
                mascots = page.locator(f"#{vid} [data-companion], #{vid} .mascot, #{vid} .luma")
                check(f"mascot elements absent from {vid}", mascots.count() == 0)

            open_monitor(page)
            font = page.locator("#m-stat-events").evaluate(
                "el => getComputedStyle(el).fontFamily"
            )
            check(
                "telemetry counters use monospace figures",
                "cascadia" in font.lower() or "consolas" in font.lower(),
                f"font={font}",
            )

            # -------------------------------------------------------------
            # 10. Light theme + responsive screenshots
            # -------------------------------------------------------------
            page.evaluate("document.documentElement.setAttribute('data-theme', 'light')")
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "05-monitor-light-theme.png"))
            shutil.copy(OUT / "05-monitor-light-theme.png", DOCS_OUT / "05-monitor-light-theme.png")
            page.evaluate("document.documentElement.setAttribute('data-theme', 'dark')")
            time.sleep(0.2)

            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "06-monitor-1366x768.png"))
            shutil.copy(OUT / "06-monitor-1366x768.png", DOCS_OUT / "06-monitor-1366x768.png")

            page.set_viewport_size({"width": 1920, "height": 1080})
            time.sleep(0.4)
            page.screenshot(path=str(OUT / "07-monitor-1920x1080.png"))
            shutil.copy(OUT / "07-monitor-1920x1080.png", DOCS_OUT / "07-monitor-1920x1080.png")

            passes = sum(1 for _, ok, _ in results if ok)
            fails = sum(1 for _, ok, _ in results if not ok)
            total = len(results)
            print(f"\n{passes}/{total} native UI-06 checks passed")

            if fails > 0:
                print(f"UI-06 NATIVE ACCEPTANCE: {fails} CHECKS FAILED")
                shutdown(proc, browser)
                sys.exit(1)
            else:
                print("UI-06 NATIVE ACCEPTANCE: ALL CHECKS PASSED")

    finally:
        shutdown(proc, browser)


if __name__ == "__main__":
    main()
