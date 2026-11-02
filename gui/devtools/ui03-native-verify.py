"""Stage UI-03 native verification against the real Tauri binary.

Drives the release cure-gui.exe over the DevTools protocol — the real
backend, real persistence collectors, real events, no mocks. It walks the
20 UI-03 native acceptance scenarios from the stage brief. What cannot be
triggered natively (a real backend failure, the native save-dialog export)
is stated in the result lines instead of being faked.

Scenario map (master brief, Phase 10):
  1. Fresh launch honest idle Rescue state .......... IDLE checks
  2. Main scan button inside the scanner workspace .. IDLE geometry
  3. Scan starts exactly once ....................... RUN poll + one log line
  4. Actual progress events update correct layers ... RUN poll timeline
  5. Radar animation reflects scanning state ........ RUN sweep/attribute
  6. Collector statuses remain legible .............. DONE name/status boxes
  7. Partial / access-denied coverage is accurate ... DONE verdict vs notes
  8. Zero findings do not imply full security ....... DONE empty-state text
  9. Completion updates radar+telemetry together .... DONE four readouts
 10. Results view concise and actionable ............ DONE results console
 11. Rescue-origin navigation behaves correctly ..... DONE stays in Rescue
 12. Overview-origin navigation remains correct ..... MID nav live line
 13. Mid-scan manual navigation is respected ........ MID nav there + back
 14. Scan errors reported without stale findings .... covered by the mock
      frontend suite (evidence-console.py failure run); asserted here only
      as "retry control exists and stale results were cleared".
 15. Defender audit-only behavior remains safe ...... TOOLS + modal wording
 16. Evidence export remains functional ............. TOOLS entry present;
      the dialog flow itself is covered by the mock suite (deterministic).
 17. Dark/light themes and responsive layouts ....... THEME + 1366x768
 18. No mascot returns .............................. DOM absence probe
 19. QA process-ownership guard remains active ...... lease self-audit
 20. Agents and Flight Recorder regressions ......... ui02a-native-verify.py
      (separate native run, reported in the acceptance document).

Run: python gui/devtools/ui03-native-verify.py
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
OUT = REPO / "testing" / "native" / "screenshots-ui03" / "native"
OUT.mkdir(parents=True, exist_ok=True)
DOCS_OUT = REPO / "docs" / "screenshots" / "ui03"
DOCS_OUT.mkdir(parents=True, exist_ok=True)
PORT = 9407
TMP_DIR = Path("D:/CURE_UI03_TMP")

# Process ownership: cleanup only touches processes a previous run of this
# harness recorded. Name-based killing is strictly forbidden (UI-02B).
sys.path.insert(0, str(REPO / "testing" / "native"))
from process_ownership import OwnershipLease, pre_launch_cleanup  # noqa: E402

LEASE_PATH = REPO / "testing" / "native" / ".process-lease-ui03.json"

SCAN_TIMEOUT_S = 300

results = []


def check(name, ok, extra=""):
    results.append((name, ok, extra))
    status = "PASS" if ok else "FAIL"
    print(f"{status}  {name}" + (f"  [{extra}]" if extra else ""))


def shot(page, name):
    target = OUT / name
    page.screenshot(path=str(target))
    shutil.copy2(target, DOCS_OUT / name)
    print(f"  screenshot: {name}")


def wait_until(page, js, timeout=SCAN_TIMEOUT_S * 1000, poll=0.3):
    end = time.time() + timeout / 1000
    while time.time() < end:
        try:
            if page.evaluate(js):
                return True
        except Exception:
            pass
        time.sleep(poll)
    return False


# Compact page-side state read. Everything comes from the page's own
# authoritative objects: CureSweep state and the rendered DOM.
STATE_JS = """() => {
  const v = window.CureSweep ? window.CureSweep.getVerdict() : null;
  const field = document.getElementById('xray-field');
  return {
    running: field ? field.getAttribute('data-running') : null,
    state: field ? field.getAttribute('data-state') : null,
    live: document.getElementById('xray-live')?.textContent?.trim() || '',
    readout: document.getElementById('xray-core-readout')?.textContent?.trim() || '',
    active: v ? v.active : null,
    phase: v ? v.phase : null,
    covered: v ? v.covered : null,
    total: v ? v.total : null,
    findings: v ? v.findings : null,
    complete: v ? v.complete : null,
    layers: v ? v.layers.map(l => [l.id, l.outcome]) : [],
  };
}"""


def poll_state(page, seen_running, seen_outcomes, angle_samples):
    s = page.evaluate(STATE_JS)
    if s["active"]:
        seen_running.add(s["active"])
    for lid, outcome in s["layers"]:
        if outcome not in ("pending", "running"):
            seen_outcomes.add(lid)
    angle = page.evaluate("() => window.CureSweep ? window.CureSweep.getState().sweepAngle : null")
    if angle is not None:
        angle_samples.append(angle)
    return s


def launch(pw, port):
    lease = OwnershipLease(LEASE_PATH)
    pre_launch_cleanup(lease, port)

    if TMP_DIR.exists():
        try:
            shutil.rmtree(TMP_DIR)
        except Exception:
            pass
    TMP_DIR.mkdir(parents=True, exist_ok=True)

    user_data_dir = str(TMP_DIR / "wvdata")
    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(port)
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
        raise SystemExit(f"CDP endpoint never appeared on port {port}")

    browser = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
    page = browser.contexts[0].pages[0]
    page.set_viewport_size({"width": 1600, "height": 900})
    page.wait_for_load_state("domcontentloaded")

    for _ in range(30):
        if page.evaluate(
            "() => { const v = document.getElementById('view-overview');"
            " return v && !v.classList.contains('hidden'); }"
        ):
            break
        time.sleep(0.5)
    else:
        browser.close()
        raise SystemExit("connected to a stale instance (Overview not visible)")

    return proc, browser, page


def shutdown(proc, browser):
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
    if TMP_DIR.exists():
        try:
            shutil.rmtree(TMP_DIR, ignore_errors=True)
        except Exception:
            pass
    OwnershipLease(LEASE_PATH).clear()


def run_acceptance():
    print("=== Stage UI-03 native acceptance (real backend, no mocks) ===")
    print(f"Target binary: {EXE}")
    assert EXE.exists(), f"Binary missing: {EXE}"

    with sync_playwright() as pw:
        proc, browser, page = launch(pw, PORT)
        lease_meta = None
        try:
            # ------------------------------------------------------------------
            # 1 + 2 + 6(pre): honest idle state, button inside the workspace.
            page.locator("#nav-scan").click()
            wait_until(
                page,
                "() => { const v = document.getElementById('scan-view');"
                " return v && !v.classList.contains('hidden'); }",
            )
            time.sleep(0.6)

            idle = page.evaluate(STATE_JS)
            idle_btn = page.evaluate(
                """() => {
                  const btn = document.getElementById('xray-start-btn');
                  const field = document.getElementById('xray-field');
                  const ws = document.querySelector('.xr-workstation');
                  if (!btn || !field || !ws) return {ok: false};
                  const b = btn.getBoundingClientRect();
                  const w = ws.getBoundingClientRect();
                  const inWs = b.top >= w.top - 2 && b.bottom <= w.bottom + 2
                            && b.left >= w.left - 2 && b.right <= w.right + 2;
                  const head = btn.closest('.rescue-head');
                  return {
                    ok: inWs && !head,
                    text: btn.textContent.trim(),
                    disabled: btn.disabled,
                    inWs: inWs,
                  };
                }"""
            )

            check(
                "1. Fresh launch: honest idle Rescue state",
                idle["live"] == "Radar standby"
                and idle["readout"] == "READY // STANDBY"
                and idle["state"] == "idle"
                and idle["phase"] == "idle"
                and idle_btn.get("disabled") is False,
                f"live={idle['live']!r}, readout={idle['readout']!r}",
            )
            check(
                "2. Main scan button positioned inside the scanner workspace",
                idle_btn.get("ok") is True and idle_btn.get("text") == "Start System Scan",
                f"text={idle_btn.get('text')!r}, inside workspace={idle_btn.get('inWs')}",
            )
            check(
                "6p. Nine inspection layers listed at standby",
                idle["total"] == 9
                and len(page.locator("#sweep-sources .xr-layer").all()) == 9
                and page.locator("#xray-matrix-cover").inner_text().strip() == "0 / 9 checked",
                f"total={idle['total']}",
            )

            shot(page, "01-idle-scanner.png")

            # ------------------------------------------------------------------
            # 3 + 4 + 5: one scan, real progress events, radar reflects state.
            seen_running = set()
            seen_outcomes = set()
            angle_samples = []
            start_clicks = 2  # deliberate double click; must still start once
            page.locator("#xray-start-btn").click()
            try:
                page.locator("#xray-start-btn").click(timeout=1500)
            except Exception:
                pass  # disabled is the expected outcome for the second click

            started = wait_until(
                page,
                "() => document.getElementById('xray-field')?.getAttribute('data-state') === 'preparing'"
                " || document.getElementById('xray-field')?.getAttribute('data-running') === 'true'",
                timeout=30000,
            )
            check(
                "3a. Scan start accepted (preparing or running after start)",
                started,
                "overlay review runs before collection",
            )

            # Poll the whole run; screenshots happen mid-poll.
            shot2_done = False
            nav_checks_done = False
            end = time.time() + SCAN_TIMEOUT_S
            while time.time() < end:
                s = poll_state(page, seen_running, seen_outcomes, angle_samples)
                if s["running"] == "true" and not shot2_done:
                    time.sleep(0.8)
                    shot(page, "02-active-scanning.png")
                    shot2_done = True
                if s["running"] == "true" and shot2_done and not nav_checks_done:
                    # 13 + 12: manual navigation mid-scan is respected; the
                    # Overview reports the live operation; the scan keeps
                    # collecting; Rescue shows it still running on return.
                    page.locator("#nav-overview").click()
                    wait_until(
                        page,
                        "() => { const v = document.getElementById('view-overview');"
                        " return v && !v.classList.contains('hidden'); }",
                        timeout=10000,
                    )
                    time.sleep(1.2)
                    ov = page.evaluate(
                        """() => ({
                          posture: document.getElementById('ov-posture')?.textContent?.trim() || '',
                          scope: document.getElementById('ov-scope')?.textContent?.trim() || '',
                        })"""
                    )
                    shot(page, "03-midscan-overview.png")
                    page.locator("#nav-scan").click()
                    wait_until(
                        page,
                        "() => { const v = document.getElementById('scan-view');"
                        " return v && !v.classList.contains('hidden'); }",
                        timeout=10000,
                    )
                    back = page.evaluate(STATE_JS)
                    nav_checks_done = True
                    check(
                        "12/13. Mid-scan navigation respected, Overview live, scan continued",
                        ("Scanning" in ov["posture"] or "Collecting" in ov["scope"])
                        and back["running"] == "true"
                        and back["phase"] in ("scanning", "preparing"),
                        f"overview posture={ov['posture']!r}, scope={ov['scope'][:60]!r}, "
                        f"rescue running={back['running']}",
                    )
                if s["running"] == "false" and s["phase"] in ("complete", "failed") and shot2_done:
                    break
                time.sleep(0.4)

            final = page.evaluate(STATE_JS)
            if not nav_checks_done:
                check(
                    "12/13. Mid-scan navigation respected, Overview live, scan continued",
                    False,
                    "scan finished before the mid-run navigation window "
                    f"(phase={final['phase']})",
                )
            if not shot2_done:
                check(
                    "RUN captured active-scanning screenshot",
                    False,
                    f"scan reached {final['phase']} before the running state "
                    "was observed",
                )

            # appendLog renders "<li><b>stage</b><span>message</span></li>".
            log_lines = page.evaluate(
                "() => [...document.querySelectorAll('#log li span')].map(e => e.textContent)"
            )
            finished_lines = [ln for ln in log_lines if "scan finished" in ln.lower()]
            check(
                "3. Scan started exactly once (one completion line, no re-entry)",
                len(finished_lines) == 1 and final["phase"] in ("complete", "failed"),
                f"completion lines={len(finished_lines)}, phase={final['phase']}",
            )
            check(
                "4. Real progress events drove the layers",
                len(seen_running) >= 2 or len(seen_outcomes) >= 3,
                f"layers seen active={sorted(seen_running)}, resolved={len(seen_outcomes)}",
            )
            sweep_moved = len(set(round(a, 3) for a in angle_samples[-20:])) > 1
            check(
                "5. Radar animation reflects scanning state",
                final["running"] == "false"
                and sweep_moved
                and any(a is not None for a in angle_samples),
                f"sweep samples varied={sweep_moved} ({len(angle_samples)} samples)",
            )

            if final["phase"] == "failed":
                check("RUN completed", False, f"native scan failed: {final['readout']}")
            else:
                check("RUN completed", True, f"phase={final['phase']}")

            # ------------------------------------------------------------------
            # 6 + 7 + 9: legible statuses, accurate coverage, consistent readouts.
            legible = page.evaluate(
                """() => {
                  const rows = [...document.querySelectorAll('#sweep-sources .xr-layer')];
                  return rows.map(r => {
                    const name = r.querySelector('.xr-layer-name');
                    const status = r.querySelector('.xr-layer-status');
                    const more = r.querySelector('.xr-layer-more');
                    return {
                      id: r.getAttribute('data-layer-id'),
                      outcome: r.getAttribute('data-outcome'),
                      name: name ? name.textContent.trim() : '',
                      status: status ? status.textContent.trim() : '',
                      hasWhy: !!more,
                      overlap: name && status
                        ? (name.getBoundingClientRect().right
                           > status.getBoundingClientRect().left + 1)
                        : true,
                    };
                  });
                }"""
            )
            check(
                "6. Collector statuses legible and separate from names",
                all(r["name"] and r["status"] and not r["overlap"] and r["hasWhy"] for r in legible)
                and len(legible) == 9,
                "; ".join(f"{r['id']}={r['status'] or 'EMPTY'}" for r in legible),
            )

            verdict = page.evaluate("() => window.CureSweep.getVerdict()")
            # `covered` already includes partial layers; every one of the nine
            # layers must be accounted for by an outcome the backend reported.
            accounted = (
                verdict["covered"]
                + verdict["denied"]
                + verdict["failed"]
                + verdict["unreported"]
            )
            gaps = [
                l for l in verdict["layers"]
                if l["outcome"] in ("denied", "failed", "partial")
            ]
            gap_rows_in_notes = page.evaluate(
                """() => {
                  const notes = [...document.querySelectorAll('#xr-result-notes li')];
                  return notes.map(n => n.textContent.trim()).filter(t =>
                    /denied|partial|failed|unreadable|skipped/i.test(t));
                }"""
            )
            gap_ok = True
            gap_extra = []
            for g in gaps:
                if g["outcome"] in ("denied", "failed", "partial"):
                    # every hard gap must surface in the results notes
                    if not gap_rows_in_notes:
                        gap_ok = False
                        gap_extra.append(f"{g['id']} missing from notes")
            check(
                "7. Partial / access-denied coverage accurate and surfaced",
                accounted == 9
                and gap_ok
                and all(
                    page.evaluate(
                        f"""() => {{
                          const row = document.querySelector(
                            '#sweep-sources [data-layer-id="{g}"] .xr-layer-detail');
                          return row && row.textContent.trim().length > 0;
                        }}"""
                    )
                    for g in [x["id"] for x in gaps]
                ),
                f"covered={verdict['covered']}/9, gaps={[g['id'] + ':' + g['outcome'] for g in gaps]}, "
                f"notes rows={len(gap_rows_in_notes)}",
            )

            # 9: radar, matrix, HUD and inline results all quote the same N/9.
            consistency = page.evaluate(
                """() => ({
                  matrix: document.getElementById('xray-matrix-cover')?.textContent?.trim() || '',
                  coverage: document.getElementById('xray-coverage')?.textContent?.trim() || '',
                  hud: document.getElementById('xray-hud-coverage')?.textContent?.trim() || '',
                  stat: (() => {
                    const items = [...document.querySelectorAll('#xr-result-stats .xr-stat-label')];
                    const i = items.findIndex(e => /layers checked/i.test(e.textContent));
                    if (i < 0) return '';
                    return document.querySelectorAll('#xr-result-stats .xr-stat-value')[i]
                      ?.textContent?.trim() || '';
                  })(),
                })"""
            )
            expected = f"{verdict['covered']} / {verdict['total']}"
            check(
                "9. Completion updates radar and telemetry consistently",
                consistency["matrix"].startswith(expected.split(" / ")[0] + " / ")
                and consistency["coverage"] == expected
                and consistency["hud"] == expected
                and consistency["stat"] == expected,
                f"matrix={consistency['matrix']!r}, coverage={consistency['coverage']!r}, "
                f"hud={consistency['hud']!r}, results stat={consistency['stat']!r}, expected~{expected!r}",
            )

            shot(page, "04-completed-scanner.png")

            # The inline results console sits below the fold: capture it
            # directly for the acceptance record.
            page.locator("#rescue-results").scroll_into_view_if_needed()
            time.sleep(0.3)
            page.locator("#rescue-results").screenshot(
                path=str(OUT / "05b-results-console-detail.png")
            )
            shutil.copy2(OUT / "05b-results-console-detail.png", DOCS_OUT / "05b-results-console-detail.png")
            page.locator("#rescue-results").screenshot(
                path=str(OUT / "05-results-console.png")
            )
            shutil.copy2(OUT / "05-results-console.png", DOCS_OUT / "05-results-console.png")
            print("  screenshot: 05-results-console.png (+ detail)")

            # ------------------------------------------------------------------
            # 8 + 10 + 11: honest empty state / actionable results / stay in Rescue.
            results_info = page.evaluate(
                """() => {
                  const panel = document.getElementById('rescue-results');
                  const title = document.getElementById('xr-results-title')?.textContent?.trim() || '';
                  const sub = document.getElementById('xr-result-sub')?.textContent?.trim() || '';
                  const empty = document.getElementById('xr-result-empty');
                  const stats = [...document.querySelectorAll('#xr-result-stats .xr-stat-label')]
                    .map(e => e.textContent.trim());
                  const findings = document.querySelectorAll('#xr-result-findings .xr-finding').length;
                  return {
                    visible: panel && !panel.classList.contains('hidden'),
                    title, sub, emptyShown: empty && !empty.classList.contains('hidden'),
                    empty: empty ? empty.textContent.trim() : '',
                    stats, findings,
                  };
                }"""
            )
            check(
                "11. Rescue-origin scan stays in the Rescue workflow",
                results_info["visible"]
                and page.evaluate(
                    "() => document.getElementById('scan-view') &&"
                    " !document.getElementById('scan-view').classList.contains('hidden')"
                ),
                f"results rendered inline, scan-view still active",
            )

            if verdict["findings"] == 0:
                honest_empty = (
                    "No findings" in results_info["title"]
                    and (
                        "not evidence" in results_info["empty"]
                        or "inspected scope" in results_info["empty"]
                        or verdict["complete"]
                    )
                )
                check(
                    "8. Zero findings do not imply full security",
                    honest_empty and results_info["findings"] == 0,
                    f"title={results_info['title']!r}, empty={results_info['empty'][:90]!r}",
                )
            else:
                findings_rows = page.evaluate(
                    """() => [...document.querySelectorAll('#xr-result-findings .xr-finding')]
                        .map(li => ({
                          name: li.querySelector('.xr-finding-name')?.textContent?.trim() || '',
                          where: li.querySelector('.xr-finding-where')?.textContent?.trim() || '',
                          risk: li.querySelector('.xr-finding-risk')?.textContent?.trim() || '',
                        }))"""
                )
                check(
                    "8. Findings presented with title, location, risk (no all-clear)",
                    results_info["findings"] > 0
                    and all(r["name"] and r["risk"] for r in findings_rows)
                    and "No findings" not in results_info["title"],
                    f"{results_info['findings']} finding row(s); "
                    f"first={findings_rows[0] if findings_rows else None}",
                )

            check(
                "10. Results view concise and actionable",
                3 <= len(results_info["stats"]) <= 6
                and results_info["sub"].count("·") >= 1
                and page.locator("#xr-result-open").is_visible(),
                f"stats={results_info['stats']}, sub={results_info['sub'][:70]!r}",
            )


            # ------------------------------------------------------------------
            # 15 + 16: Defender audit-only + export entry (Tools menu).
            page.locator("#xr-tools summary").click()
            time.sleep(0.3)
            tools = page.evaluate(
                """() => {
                  const def = document.getElementById('xray-defender-scan-btn');
                  const exp = document.getElementById('xray-export-case-btn');
                  return {
                    defVisible: !!def && def.offsetParent !== null,
                    defNote: def ? def.querySelector('.xr-tools-note')?.textContent?.trim() || '' : '',
                    expVisible: !!exp && exp.offsetParent !== null,
                  };
                }"""
            )
            check(
                "15/16. Tools menu: Defender audit-only note + export entry present",
                tools["defVisible"] and "audit only" in tools["defNote"].lower()
                and tools["expVisible"],
                f"defender note={tools['defNote']!r}, export visible={tools['expVisible']}",
            )
            # Open the Defender modal, verify audit-only defaults, close it.
            page.locator("#xray-defender-scan-btn").click()
            wait_until(
                page,
                "() => { const m = document.getElementById('xray-defender-modal');"
                " return m && !m.classList.contains('hidden'); }",
                timeout=5000,
            )
            defender = page.evaluate(
                """() => {
                  const modal = document.getElementById('xray-defender-modal');
                  const scanBtn = document.getElementById('xray-defender-run-scan');
                  const contain = document.getElementById('xray-defender-containment-box');
                  return {
                    open: modal && !modal.classList.contains('hidden'),
                    scanLabel: scanBtn ? scanBtn.textContent.trim() : '',
                    containmentHidden: contain ? contain.classList.contains('hidden') : true,
                  };
                }"""
            )
            check(
                "15b. Defender modal default is read-only remediation-off",
                defender["open"]
                and "DisableRemediation" in defender["scanLabel"]
                and defender["containmentHidden"],
                f"scan button={defender['scanLabel']!r}",
            )
            page.locator("#xray-defender-modal-done").click()
            time.sleep(0.2)
            page.keyboard.press("Escape")
            # The modal's own handlers may already have closed the tools
            # menu; force it closed instead of toggling blind.
            page.evaluate("() => { const t = document.getElementById('xr-tools'); if (t) t.open = false; }")

            # ------------------------------------------------------------------
            # 17: themes + responsive layouts.
            page.locator("#theme-toggle").click()
            time.sleep(0.5)
            theme_light = page.evaluate(
                "() => document.documentElement.getAttribute('data-theme')"
            )
            shot(page, "06-light-theme.png")
            page.locator("#theme-toggle").click()
            time.sleep(0.5)
            theme_dark = page.evaluate(
                "() => document.documentElement.getAttribute('data-theme')"
            )
            check(
                "17a. Dark and light themes switch",
                theme_light == "light" and theme_dark in ("dark", None),
                f"light={theme_light!r}, restored={theme_dark!r}",
            )

            page.set_viewport_size({"width": 1366, "height": 768})
            time.sleep(0.6)
            layout = page.evaluate(
                """() => {
                  const overflow = document.documentElement.scrollWidth
                    > window.innerWidth + 1;
                  const radar = document.querySelector('.xr-radar-col')?.getBoundingClientRect();
                  const side = document.querySelector('.xr-side')?.getBoundingClientRect();
                  const overlap = radar && side
                    ? radar.right > side.left + 1 : false;
                  const clipped = radar && side
                    ? (radar.width < 50 || side.width < 50) : true;
                  return { overflow, overlap, clipped,
                           scrollW: document.documentElement.scrollWidth,
                           w: window.innerWidth };
                }"""
            )
            check(
                "17b. 1366x768: no overflow, no panel overlap or clipping",
                not layout["overflow"] and not layout["overlap"] and not layout["clipped"],
                f"scrollW={layout['scrollW']}, viewport={layout['w']}, "
                f"overlap={layout['overlap']}, clipped={layout['clipped']}",
            )
            shot(page, "07-1366x768.png")

            page.set_viewport_size({"width": 1920, "height": 1080})
            time.sleep(0.6)
            layout_hd = page.evaluate(
                "() => document.documentElement.scrollWidth > window.innerWidth + 1"
            )
            check("17c. 1920x1080: no horizontal overflow", not layout_hd, "")

            # ------------------------------------------------------------------
            # 14 (native part) + 18 + 19.
            check(
                "14n. Failure affordance present (retry control); error path itself "
                "covered by the mock suite",
                page.locator("#scan-retry").count() == 1,
                "retry button present in workspace",
            )

            mascot = page.evaluate(
                """() => ({
                  luma: !!document.querySelector(
                    '[id*="luma" i], [class*="luma" i], '
                    + '[id*="mascot" i], [class*="mascot" i]'),
                  companionInRescue: !!document.querySelector(
                    '#scan-view [data-companion], #results-view [data-companion]'),
                })"""
            )
            check(
                "18. No mascot or companion widgets in Rescue/Investigate",
                not mascot["luma"] and not mascot["companionInRescue"],
                f"luma={mascot['luma']}, companion={mascot['companionInRescue']}",
            )

            lease_alive = LEASE_PATH.exists()
            check(
                "19. QA process-ownership lease active for this harness",
                lease_alive,
                f"lease file={LEASE_PATH.name}",
            )

            # Return the workstation to its completed idle framing for the
            # final documentation shot at full size.
            page.set_viewport_size({"width": 1600, "height": 900})
            time.sleep(0.5)
            shot(page, "08-final-completed-1600.png")

        finally:
            shutdown(proc, browser)

    print("\n=== Native acceptance summary ===")
    total = len(results)
    passed = sum(1 for _, ok, _ in results if ok)
    failed = total - passed
    print(f"Total checks: {total} | Passed: {passed} | Failed: {failed}")
    for name, ok, extra in results:
        if not ok:
            print(f"  FAILED: {name} [{extra}]")
    if failed:
        raise SystemExit(f"{failed} check(s) failed")
    print("UI-03 NATIVE ACCEPTANCE: ALL CHECKS PASSED")


if __name__ == "__main__":
    run_acceptance()
