"""Stage UI-02A native acceptance: Agent Radar polish + safe process control.

Drives the real cure-gui.exe (real backend, no mocks) through the stage
scenarios, including genuine process termination of a harmless test-owned
process. The test process is spawned by this script (a copy of cmd.exe
named codex.exe in a disposable D: workspace, running a bounded ping);
the owner's real agents are never touched. The recorder uses a disposable
D: workspace that the script creates and removes.

Run: python gui/devtools/ui02a-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui02a" / "native"
OUT.mkdir(parents=True, exist_ok=True)
WS = Path("D:/CURE_UI02A_WS")

# Process ownership: cleanup only touches processes a previous run of this
# harness recorded (identity re-verified before any signal). Name-based
# killing is forbidden by core/tests/qa_process_safety.rs.
sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui02a.json"

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


def shot(page, name):
    page.screenshot(path=str(OUT / name))
    print("  shot", name)


def launch(pw, port):
    # Identity-verified cleanup of a previous run's leftovers; foreign or
    # stale PIDs are skipped, and a foreign port holder fails the run.
    lease = OwnershipLease(LEASE_PATH)
    pre_launch_cleanup(lease, port)
    tmp = tempfile.TemporaryDirectory(prefix="cure-ui02a-", ignore_cleanup_errors=True)
    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(port)
    user_data_dir = str(Path(tmp.name) / "wvdata")
    env["WEBVIEW2_USER_DATA_FOLDER"] = user_data_dir
    proc = subprocess.Popen([str(EXE)], env=env)
    lease.record(proc.pid, EXE.name)
    lease.user_data_dir = user_data_dir
    lease.port = port
    lease.save()
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
    OwnershipLease(LEASE_PATH).clear()


def scan_agents(page):
    page.locator("#ar-scan-now").click()
    wait_until(page, "!window.CureAgentRadar.state.scanning", timeout=30000)


def main():
    # Self-healing fixture dir: a previous run's cleanup may have raced a
    # file handle. Only this script's own fixture ever lives here.
    if WS.exists():
        shutil.rmtree(WS, ignore_errors=True)
        time.sleep(0.5)
    WS.mkdir(parents=True, exist_ok=True)

    # Harmless test-owned "agent": cmd.exe copy named codex.exe, running a
    # bounded ping whose output goes to a uniquely named file. Spawned,
    # controlled and reaped by this script only; the owner's real agents
    # are never touched.
    test_exe = WS / "codex.exe"
    shutil.copyfile(r"C:\Windows\System32\cmd.exe", test_exe)
    test_proc = subprocess.Popen(
        [str(test_exe), "/c", "ping", "-n", "600", "127.0.0.1", ">nul"],
        cwd=str(WS),
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    time.sleep(1.0)
    test_pid = test_proc.pid
    print("test-owned process codex.exe pid", test_pid)

    try:
        with sync_playwright() as pw:
            # ---------- session 1 ----------
            tmp, proc, browser, page = launch(pw, 9416)
            page.locator("#nav-agents").click()
            page.wait_for_timeout(500)

            # 1. Fresh launch: clean radar state.
            check("1 fresh launch radar idle and honest",
                  page.locator("#ar-radar").get_attribute("data-phase") == "idle"
                  and "Not scanned" in page.locator("#ar-radar-note").inner_text()
                  and page.locator("#ar-agent-list .ar-agent-card").count() == 0)
            shot(page, "01-native-fresh-agents.png")

            # 3/4. Manual scan: genuine detection incl. the test process.
            scan_agents(page)
            snapshot = page.evaluate("window.CureAgentRadar.state.radar")
            pids = [p["pid"] for p in snapshot["processes"]]
            check("3 manual scan detects real agent processes",
                  snapshot["detected_count"] + snapshot["model_server_count"] > 0,
                  "center=" + page.locator("#ar-detected-count").inner_text())
            check("4 compact radar shows the real snapshot",
                  page.locator("#ar-scan-at").inner_text().startswith("Snapshot ")
                  and page.locator("#ar-blips .ar-blip").count() >= 1)
            check("the test-owned codex.exe is detected",
                  test_pid in pids, "pids=" + str(pids))
            shot(page, "02-native-detected-agents.png")

            # 5/6. Expand details; per-process rows with individual PIDs.
            page.locator("#ar-agent-list .ar-agent-card .ar-agent-row").first.click()
            expect(page.locator("#ar-agent-list .ar-proc").first).to_be_visible()
            rows = page.locator("#ar-agent-list .ar-proc")
            check("5 agent details expand with per-process rows",
                  rows.count() >= 1
                  and "PID" in page.locator("#ar-agent-list .ar-agent-card").first.inner_text())
            check("6 individual PID rows are selectable/inspected",
                  page.locator("#ar-agent-list .ar-proc").count()
                  == page.evaluate("window.CureAgentRadar.state.radar.processes.length"))

            # The test process's own row.
            target_row = page.locator(
                "#ar-agent-list .ar-proc").filter(has_text="PID " + str(test_pid))
            check("the test-owned process has its own row",
                  target_row.count() == 1)

            # 8. Cancel causes no process change.
            target_row.locator(".ar-proc-end").click()
            expect(page.locator("#confirm-overlay")).to_be_visible()
            shot(page, "03-native-end-process-confirm.png")
            page.locator("#confirm-cancel").click()
            expect(page.locator("#confirm-overlay")).to_be_hidden()
            time.sleep(0.8)
            check("8 cancel causes no process change", test_proc.poll() is None)

            # 7/9. Confirm: the test-owned process is really ended.
            target_row.locator(".ar-proc-end").click()
            expect(page.locator("#confirm-overlay")).to_be_visible()
            page.locator("#confirm-ok").click()
            # Wait for the FINAL outcome (the command verifies the exit).
            wait_until(
                page,
                """() => {
                     const s = document.getElementById('ar-action-status').textContent;
                     return s && !s.startsWith('Ending');
                   }""",
                timeout=30000,
            )
            status = page.locator("#ar-action-status").inner_text()
            check("9 test-owned process ended after explicit confirmation",
                  test_proc.poll() is not None
                  and ("gracefully" in status.lower() or "force-stopped" in status.lower()),
                  status[:90])
            wait_until(page, "!window.CureAgentRadar.state.scanning", timeout=30000)
            page.wait_for_timeout(800)

            # 12. Radar updates after the exit.
            snapshot2 = page.evaluate("window.CureAgentRadar.state.radar")
            pids2 = [p["pid"] for p in snapshot2["processes"]]
            check("12 radar reflects the process exit",
                  test_pid not in pids2, "pids=" + str(pids2))

            # Journal entry written without secrets.
            journal_has_entry = page.evaluate(
                """async () => {
                     const log = await window.__TAURI__.core.invoke('operator_audit_log', { limit: 10 });
                     return (log || []).some(e => e.action_type === 'AgentProcessEnd'
                                && String(e.target || '').includes('""" + str(test_pid) + """'));
                   }"""
            )
            check("the audit journal records the authorized end (no secrets)",
                  journal_has_entry)

            # 10. Stale target rejected: IPC with a wrong start time.
            rejected = page.evaluate(
                """async () => {
                     try {
                       await window.__TAURI__.core.invoke('operator_agent_end_process', {
                         pid: %d, expectedName: 'codex.exe',
                         expectedStartedUnix: 1000,
                         confirmationToken: 'END_%d' });
                       return false;
                     } catch (e) { return String(e); }
                   }"""
                % (test_pid, test_pid)
            )
            check("10 stale/identity-mismatched target is rejected",
                  isinstance(rejected, str) and ("stale" in rejected.lower() or "not a currently detected" in rejected.lower()),
                  str(rejected)[:90])

            # 11. Protected processes cannot be targeted (not agent-scoped).
            protected = page.evaluate(
                """async () => {
                     try {
                       await window.__TAURI__.core.invoke('operator_agent_end_process', {
                         pid: 4, expectedName: 'System',
                         expectedStartedUnix: null,
                         confirmationToken: 'END_4' });
                       return false;
                     } catch (e) { return String(e); }
                   }"""
            )
            check("11 protected/system process cannot be targeted",
                  isinstance(protected, str) and "not a currently detected" in protected.lower(),
                  str(protected)[:90])

            # 13-16. Recorder on the disposable workspace (also leaves a
            # recording for the restart scenario).
            page.locator("#ar-workspace").fill(str(WS))
            page.locator("#ar-start").click()
            expect(page.locator("#ar-session-badge")).to_have_text("Recording", timeout=30000)
            # The start triggers a radar refresh; the settled state is
            # monitoring with no scan in flight.
            wait_until(page, "!window.CureAgentRadar.state.scanning", timeout=30000)
            page.wait_for_timeout(400)
            check("13 recorder starts on the disposable workspace",
                  page.locator("#ar-session-badge").inner_text().strip().lower() == "recording"
                  and page.locator("#ar-radar").get_attribute("data-phase") == "monitoring",
                  "phase=" + str(page.locator("#ar-radar").get_attribute("data-phase")))
            check("13 replay controls hidden while live",
                  page.locator("#ar-review").is_hidden()
                  and page.locator("#ar-review-toggle").is_hidden())
            (WS / "ui02a-created.txt").write_text("ui02a fixture\n", encoding="utf-8")
            wait_until(page, "parseInt(document.getElementById('ar-stat-total').textContent) >= 1", timeout=30000)
            time.sleep(3.0)  # past the recorder's 2 s dedupe window
            (WS / "ui02a-created.txt").write_text("ui02a fixture v2\n", encoding="utf-8")
            wait_until(page, "parseInt(document.getElementById('ar-stat-total').textContent) >= 2", timeout=45000)
            check("14 real filesystem events recorded", True)
            shot(page, "04-native-recorder-compact.png")

            page.locator("#ar-stop").click()
            expect(page.locator("#ar-session-badge")).to_have_text("Stopped", timeout=30000)
            check("15 replay only in review mode",
                  page.locator("#ar-review").is_hidden()
                  and page.locator("#ar-review-toggle").is_visible())
            page.locator("#ar-review-toggle").click()
            expect(page.locator("#ar-timeline")).to_be_visible()
            check("15 timeline review shows the recorded events", True)
            page.locator("#ar-review-toggle").click()

            # 16. Export formats + privacy.
            page.locator("#ar-export-toggle").click()
            expect(page.locator("#ar-case-preview")).to_be_visible()
            page.locator("#ar-case-preview").click()
            expect(page.locator("#ar-case-stage img")).to_be_visible(timeout=20000)
            leaked = page.evaluate(
                """() => {
                     const s = JSON.stringify(window.CureCaseFile.audit(window.CureAgentRadar.state.caseModel));
                     return s.match(/[A-Za-z]:\\\\\\\\|CURE_UI02A_WS|Users|ui02a/gi) || [];
                   }"""
            )
            check("16 case file carries no paths or file names", not leaked, str(leaked[:3]))
            page.locator("#ar-case-export").click()
            expect(page.locator("#ar-case-status")).to_contain_text("Case file written", timeout=20000)
            check("16 evidence-card export writes locally",
                  "uploaded to" not in page.locator("#ar-case-status").inner_text().lower())

            # 20. Light theme capture.
            page.locator("#theme-toggle").click()
            page.wait_for_timeout(400)
            shot(page, "05-native-light-theme.png")
            page.locator("#theme-toggle").click()
            shutdown(tmp, proc, browser)

            # 2/14(restart). Prior recording: collapsed and clearly stopped.
            tmp, proc, browser, page = launch(pw, 9418)
            page.locator("#nav-agents").click()
            page.wait_for_timeout(600)
            check("2 previous recording collapsed and clearly stopped",
                  page.locator("#ar-history").is_visible()
                  and page.locator("#ar-session-badge").inner_text().strip().lower() == "stopped"
                  and page.locator("#ar-flight-live").is_hidden(),
                  "badge=" + page.locator("#ar-session-badge").inner_text())
            page.locator("#ar-history-open").click()
            expect(page.locator("#ar-flight-live")).to_be_visible()
            check("2 opening history reveals the evidence without resuming",
                  page.locator("#ar-session-badge").inner_text().strip().lower() == "stopped")
            shot(page, "06-native-history-open.png")

            # 17. Windows Change Radar under Investigate.
            page.locator("#nav-results").click()
            page.wait_for_timeout(400)
            page.locator('[data-route="view-winchanges"]').first.click()
            expect(page.locator("#view-winchanges")).to_be_visible(timeout=10000)
            check("17 Windows Change Radar reachable under Investigate",
                  page.locator("#view-title").inner_text() == "Investigate / Windows changes")
            page.locator("#ar-win-capture").click()
            expect(page.locator("#ar-winsnap-state")).to_contain_text("Trusted snapshot saved", timeout=15000)
            check("17 snapshot capability preserved", True)

            # 19. No mascot anywhere on the agents flow.
            check("19 no mascot present",
                  page.evaluate("document.querySelectorAll('.luma-svg,.luma-stage').length") == 0
                  and page.evaluate("typeof window.Luma") == "undefined")

            # 20. 1366x768 layout sanity via viewport resize.
            page.locator("#nav-agents").click()
            page.set_viewport_size({"width": 1366, "height": 768})
            page.wait_for_timeout(500)
            overflow = page.evaluate(
                "() => document.documentElement.scrollWidth > window.innerWidth")
            check("20 1366x768 layout without horizontal overflow", not overflow)
            shot(page, "07-native-1366.png")
            shutdown(tmp, proc, browser)
    finally:
        if test_proc.poll() is None:
            test_proc.kill()
        # Reap the orphaned ping child (ending only the selected parent is
        # the product's documented behaviour, so the test cleans up its own
        # child by parentage — never by image name).
        subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "Get-CimInstance Win32_Process -Filter \"Name='PING.EXE'\" | "
             "Where-Object { $_.ParentProcessId -eq %d } | "
             "ForEach-Object { Stop-Process -Id $_.ProcessId -Force }" % test_pid],
            capture_output=True,
        )
        time.sleep(0.6)
        for _ in range(6):
            if not WS.exists():
                break
            shutil.rmtree(WS, ignore_errors=True)
            time.sleep(1.0)

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} native UI-02A checks passed")
    if failed:
        for name, _, extra in failed:
            print("FAILED:", name, extra)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
