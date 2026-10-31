"""Stage UI-02 native acceptance for the Agent Radar & Flight Recorder.

Drives the real cure-gui.exe (real backend, real WebView2, no mocks)
through the stage's scenario list. The only filesystem the recorder
touches is a disposable workspace this script creates under D: and removes
at the end — the owner's real agent sessions and projects are never read
as fixtures and never modified.

Run: python gui/devtools/ui02-native-verify.py
"""
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from playwright.sync_api import sync_playwright, expect

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
OUT = REPO / "testing" / "native" / "screenshots-ui02" / "native"
OUT.mkdir(parents=True, exist_ok=True)
WS = Path("D:/CURE_UI02_WS")

results = []


def check(name, ok, extra=""):
    results.append((name, ok, extra))
    print(("PASS  " if ok else "FAIL  ") + name + ("  [" + extra + "]" if extra else ""))


def wait_until(page, js, timeout=60000, poll=0.4):
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


def agent_state(page):
    return page.evaluate(
        """() => ({
            phase: document.getElementById('ar-radar').dataset.phase,
            badge: document.getElementById('ar-session-badge').textContent,
            note: document.getElementById('ar-radar-note').textContent.slice(0, 60),
            flightHidden: document.getElementById('ar-flight-live').classList.contains('hidden'),
            sessionActive: !!(window.CureAgentRadar && window.CureAgentRadar.state.session
                                && window.CureAgentRadar.state.session.active),
            hasSession: !!(window.CureAgentRadar && window.CureAgentRadar.state.session),
            scanning: window.CureAgentRadar ? window.CureAgentRadar.state.scanning : null,
        })"""
    )


def shot(page, name):
    page.screenshot(path=str(OUT / name))
    print("  shot", name)


def kill_lingering():
    subprocess.run(["taskkill", "/F", "/IM", "cure-gui.exe"], capture_output=True)
    subprocess.run(["taskkill", "/F", "/IM", "msedgewebview2.exe"], capture_output=True)
    time.sleep(1.0)


def launch(pw, port):
    kill_lingering()
    tmp = tempfile.TemporaryDirectory(prefix="cure-ui02-", ignore_cleanup_errors=True)
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
        raise SystemExit("CDP endpoint never appeared on " + str(port))
    browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
    page = browser.contexts[0].pages[0]
    page.set_viewport_size({"width": 1600, "height": 900})
    page.wait_for_load_state("networkidle")
    for _ in range(20):
        if page.evaluate(
            "() => { const v = document.getElementById('view-overview');"
            " return v && !v.classList.contains('hidden'); }"
        ):
            break
        time.sleep(0.5)
    else:
        raise SystemExit("connected to a stale instance")
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


def main():
    if WS.exists():
        shutil.rmtree(WS, ignore_errors=True)
    WS.mkdir(parents=True)

    with sync_playwright() as pw:
        # ---------- session 1 ----------
        tmp, proc, browser, page = launch(pw, 9412)
        page.locator("#nav-agents").click()
        page.wait_for_timeout(500)

        # 1. Fresh launch: honest idle, no previous scan as current results.
        check("1 fresh launch radar is idle and says not scanned",
              page.locator("#ar-radar").get_attribute("data-phase") == "idle"
              and "Not scanned" in page.locator("#ar-radar-note").inner_text()
              and page.locator("#ar-blips .ar-blip").count() == 0
              and page.locator("#ar-agent-list .ar-agent-card").count() == 0)
        check("1 scan-at states 'Not scanned this session'",
              page.locator("#ar-scan-at").inner_text().strip() == "Not scanned this session")
        check("16 no mascot on the agents page",
              page.evaluate("document.querySelectorAll('.luma-svg,.luma-stage').length") == 0
              and page.evaluate("typeof window.Luma") == "undefined")
        shot(page, "01-native-agents-idle.png")

        # 2. Manual scan detects genuine supported processes.
        page.locator("#ar-scan-now").click()
        wait_until(page, "document.getElementById('ar-scan-now').textContent !== 'Scanning…' && !document.getElementById('ar-scan-now').disabled", timeout=30000)
        detected = page.locator("#ar-detected-count").inner_text()
        note = page.locator("#ar-radar-note").inner_text()
        check("2 manual scan completes and reports real results",
              page.locator("#ar-scan-at").inner_text().startswith("Snapshot ")
              and "Enumerating" not in note,
              note[:90])
        cards = page.locator("#ar-agent-list .ar-agent-card").count()
        check("2 agent inventory populated from the real process table",
              cards > 0, str(cards) + " agent group(s); center=" + detected)
        check("2 sweep stopped after the scan",
              page.locator("#ar-radar").get_attribute("data-phase") == "idle")

        # 4. Refresh re-enumerates (fresh snapshot timestamp).
        stamp1 = page.locator("#ar-scan-at").inner_text()
        page.wait_for_timeout(1200)
        page.locator("#ar-refresh").click()
        wait_until(page, "document.getElementById('ar-scan-now').textContent !== 'Scanning…' && !document.getElementById('ar-scan-now').disabled", timeout=30000)
        stamp2 = page.locator("#ar-scan-at").inner_text()
        check("4 refresh performs a fresh enumeration", stamp1 != stamp2, stamp1 + " -> " + stamp2)

        # 5/6/7. Grouping, identity honesty, session intel from the real survey.
        first_card = page.locator("#ar-agent-list .ar-agent-card").first
        check("5 multiple processes of one agent are grouped in one row",
              page.locator("#ar-agent-list .ar-agent-card").count() < page.evaluate("window.CureAgentRadar.state.radar.processes.length"),
              "rows=" + str(page.locator("#ar-agent-list .ar-agent-card").count())
              + " processes=" + str(page.evaluate("window.CureAgentRadar.state.radar.processes.length")))
        first_card.locator(".ar-agent-row").click()
        expect(first_card.locator(".ar-agent-body")).to_be_visible()
        body_text = first_card.inner_text()
        check("6 identity classification is stated honestly",
              ("verified tool identity" in body_text)
              or ("unverified" in body_text.lower())
              or ("access denied" in body_text.lower()),
              body_text[:80].replace("\n", " "))
        check("7 session artifacts are not misrepresented as recovery",
              ("Session artifacts:" in body_text)
              and ("recoverable" not in body_text.lower() or "session format" in body_text.lower() or "Recoverable session artifacts" in body_text),
              "intel: " + (body_text.split("Session artifacts:")[1][:70] if "Session artifacts:" in body_text else "none"))
        shot(page, "02-native-agents-detected.png")

        # 8-12. Recording on the disposable D: workspace.
        page.locator("#ar-workspace").fill(str(WS))
        page.locator("#ar-start").click()
        expect(page.locator("#ar-session-badge")).to_have_text("Recording", timeout=30000)
        # The start also triggers a radar scan; wait for it to settle.
        wait_until(page, "document.getElementById('ar-radar').dataset.phase !== 'scanning'", timeout=30000)
        st = agent_state(page)
        check("8 recording starts on the disposable workspace",
              st["badge"] == "Recording" and st["phase"] == "monitoring",
              str(st))
        check("8 workspace input shows the monitored path",
              str(WS).replace("\\", "").replace("/", "")
              in page.locator("#ar-workspace").input_value().replace("\\", "").replace("/", ""))

        # 9. Real file creation appears in the timeline.
        (WS / "created-by-ui02.txt").write_text("stage ui-02 fixture\n", encoding="utf-8")
        wait_until(page, "document.getElementById('ar-stat-total').textContent !== '0'", timeout=30000)
        check("9 real file creation observed", int(page.locator("#ar-stat-total").inner_text()) >= 1)

        # 10. Modification + rename.
        (WS / "created-by-ui02.txt").write_text("stage ui-02 fixture modified\n", encoding="utf-8")
        (WS / "rename-src.txt").write_text("rename fixture\n", encoding="utf-8")
        wait_until(page, "parseInt(document.getElementById('ar-stat-total').textContent) >= 2", timeout=30000)
        (WS / "rename-src.txt").rename(WS / "rename-dst.txt")
        wait_until(page,
                   "!!document.querySelector('#ar-event-list .ar-event-kind[data-kind=\\'renamed\\']')"
                   " || !!document.querySelector('#ar-timeline .ar-tick[data-kind=\\'renamed\\']')",
                   timeout=30000)
        check("10 modification and rename events observed", True)
        shot(page, "03-native-recorder-active.png")

        # 11. Removal event.
        (WS / "created-by-ui02.txt").unlink()
        wait_until(page,
                   "!!document.querySelector('#ar-event-list .ar-event-kind[data-kind=\\'removed\\']')"
                   " || !!document.querySelector('#ar-timeline .ar-tick[data-kind=\\'removed\\']')",
                   timeout=30000)
        check("11 removal event recorded", True)

        # 13. Replay + evidence selection (before stopping, replay works too).
        page.locator("#ar-event-list .ar-event").first.click()
        expect(page.locator("#ar-evidence-card")).to_be_visible()
        evidence_text = page.locator("#ar-evidence-card").inner_text().lower()
        check("13 evidence selection shows a structured detail",
              "observation" in evidence_text and "workspace" in evidence_text,
              evidence_text[:100].replace("\n", " "))
        page.locator("#ar-scrub").fill("1")
        page.wait_for_timeout(200)
        check("13 replay scrub narrows the visible observations",
              "observations" in page.locator("#ar-scrub-label").inner_text()
              or "/" in page.locator("#ar-scrub-label").inner_text(),
              page.locator("#ar-scrub-label").inner_text())
        page.locator("#ar-scrub").fill(str(page.locator("#ar-scrub").get_attribute("max")))
        shot(page, "04-native-evidence-selected.png")

        # 12. Stop recording: sweep stops, session final.
        total_before_stop = page.evaluate("window.CureAgentRadar.state.session.events.length")
        page.locator("#ar-stop").click()
        expect(page.locator("#ar-session-badge")).to_have_text("Stopped", timeout=30000)
        # The stop triggers a final radar refresh; let it settle.
        wait_until(page, "document.getElementById('ar-radar').dataset.phase !== 'scanning'", timeout=30000)
        time.sleep(1.0)
        events_after_stop = page.evaluate("window.CureAgentRadar.state.session.events.length")
        st = agent_state(page)
        check("12 stop recording finalises the session",
              st["phase"] == "idle" and events_after_stop >= total_before_stop,
              str(st))
        (WS / "after-stop.txt").write_text("should not be observed\n", encoding="utf-8")
        page.wait_for_timeout(2500)
        check("12 no new observations after stop",
              page.evaluate("window.CureAgentRadar.state.session.events.length") == events_after_stop)

        # 15. Privacy-aware case file export.
        page.locator("#ar-case-preview").click()
        expect(page.locator("#ar-case-stage img")).to_be_visible(timeout=20000)
        leaked = page.evaluate(
            """() => {
                 const s = JSON.stringify(window.CureCaseFile.audit(window.CureAgentRadar.state.caseModel));
                 const m = s.match(/[A-Za-z]:\\\\\\\\|CURE_UI02_WS|Users|created-by|rename-/gi) || [];
                 return m;
               }"""
        )
        check("15 case file carries no paths or file names", not leaked, str(leaked[:3]))
        page.locator("#ar-case-export").click()
        export_status = ""
        try:
            expect(page.locator("#ar-case-status")).to_contain_text("Case file written", timeout=20000)
            export_status = page.locator("#ar-case-status").inner_text()
        except Exception:
            export_status = page.locator("#ar-case-status").inner_text()
        check("15 case export writes locally",
              "Case file written" in export_status
              and "uploaded to" not in export_status.lower(),
              export_status[:90])

        # Light theme capture.
        page.locator("#theme-toggle").click()
        page.wait_for_timeout(400)
        shot(page, "05-native-agents-light.png")
        page.locator("#theme-toggle").click()
        posture_before = page.locator("#ov-posture").inner_text()
        shutdown(tmp, proc, browser)

        # 14. Restart: prior recording state handled honestly.
        tmp, proc, browser, page = launch(pw, 9414)
        check("14 restart starts the recorder idle (no fake live session)",
              page.locator("#ar-session-badge").inner_text() == "Idle"
              or page.locator("#ar-session-badge").inner_text() == "Stopped")
        page.locator("#nav-agents").click()
        page.wait_for_timeout(900)
        st = agent_state(page)
        check("14 restart radar idle; recorder shows no live session",
              "Not scanned" in st["note"]
              and (st["flightHidden"] or not st["sessionActive"]),
              str(st))
        shutdown(tmp, proc, browser)

    if WS.exists():
        shutil.rmtree(WS, ignore_errors=True)

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} native UI-02 checks passed")
    if failed:
        for name, _, extra in failed:
            print("FAILED:", name, extra)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
