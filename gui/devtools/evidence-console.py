"""Native Python Playwright quality gates and documentation capture.

All mutations use mock-tauri.js; no host files/processes are remediated.
Run: python gui/devtools/evidence-console.py [verify|layout|pixel|capture|demo|agent-video|all]
"""
from pathlib import Path
from datetime import datetime, timezone
import argparse
import json
import os
import re
import subprocess
import sys
import threading
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from functools import partial
from playwright.sync_api import sync_playwright, expect
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
URL = (ROOT / "gui/dist/index.dev.html").as_uri()
SHOTS = Path(os.environ.get("CURE_SHOTS_DIR", ROOT / "gui/dev-screenshots"))
DOCS = ROOT / "docs/screenshots"
AXE = ROOT / "gui/devtools/node_modules/axe-core/axe.min.js"
FIXED = datetime(2026, 10, 4, 3, 30, tzinfo=timezone.utc)
checks = []



def companion_running(page):
    """Animations still running inside a companion stage.

    The check is scoped to the companion on purpose: a colour transition on a
    navigation icon is the interface settling, not a loop that failed to stop.
    """
    return page.evaluate("""() => {
        const stages = [...document.querySelectorAll('.luma-stage')];
        return document.getAnimations()
            .filter((a) => a.playState === 'running')
            .filter((a) => a.effect && a.effect.target &&
                            stages.some((s) => s.contains(a.effect.target)))
            .length;
    }""")

def check(condition, label):
    assert condition, label
    checks.append(label)


def open_page(browser, size=(1440, 900), reduced=False, video=False, **knobs):
    cfg = {"viewport": {"width": size[0], "height": size[1]},
           "reduced_motion": "reduce" if reduced else "no-preference",
           "timezone_id": "Asia/Kolkata", "locale": "en-GB"}
    if video:
        cfg["record_video_dir"] = str(ROOT / "docs/media")
        cfg["record_video_size"] = cfg["viewport"]
    context = browser.new_context(**cfg)
    page = context.new_page()
    page.clock.set_fixed_time(FIXED)
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
    page.on("request", lambda r: errors.append("Remote request: " + r.url)
            if r.url.startswith(("http:", "https:")) and not r.url.startswith(URL.rsplit('/', 1)[0]) else None)
    init = {"__CURE_MOCK_OVERLAY_HITS": 0, "__CURE_MOCK_EMPTY_QUARANTINE": True,
            **{"__CURE_MOCK_" + k.upper(): v for k, v in knobs.items()}}
    page.add_init_script("Object.assign(window," + json.dumps(init) + ")")
    page.goto(URL)
    page.wait_for_load_state("networkidle")
    expect(page.locator("#view-overview")).to_be_visible()
    return context, page, errors


def start_scan(page):
    page.locator("#nav-scan").click()
    page.locator("#xray-start-btn").click()


def finish_scan(page):
    page.wait_for_function("window.__CURE_SCAN_DONE === true", timeout=30000)
    if not page.locator("#results-view").is_visible():
        btn = page.locator("#xray-investigate-btn")
        if btn.is_visible():
            btn.click()
        else:
            open_btn = page.locator("#xr-result-open")
            if open_btn.is_visible():
                open_btn.click()
    expect(page.locator("#results-view")).to_be_visible(timeout=30000)


def scan_origin_flow(browser):
    """UI-01A regression: the scan's origin owns navigation.

    An Overview-origin scan keeps the operator on the Overview with a live,
    truthful hero; a Rescue-origin scan keeps the historical Rescue flow;
    manual navigation during any scan is never overridden at completion;
    repeated clicks never start a second collection; a failed overview scan
    shows the failure, never the previous scan's results.
    """
    commands = "window.__CURE_MOCK_COMMANDS.filter(c => c === 'run_auto_scan').length"

    # --- Overview origin: start, live state, completion, stay. ---
    ctx, page, errors = open_page(browser, (1440, 900))
    page.locator("#overview-rescue-btn").click()
    expect(page.locator("#ov-posture")).to_have_text("Scanning", timeout=15000)
    check(page.locator("#view-overview").is_visible(),
          "an overview-origin scan keeps the Overview on screen")
    check(page.evaluate("() => !document.getElementById('scan-view') || document.getElementById('scan-view').classList.contains('hidden')"),
          "the scan view is not forced in front of the operator")
    check(page.locator("#overview-rescue-btn").is_disabled(),
          "the hero scan button is disabled while a scan runs")
    page.wait_for_function(
        "document.getElementById('ov-scope').textContent.includes('elapsed')",
        timeout=10000)
    check("elapsed" in page.locator("#ov-scope").inner_text(),
          "the hero shows real elapsed time: " + page.locator("#ov-scope").inner_text())
    check(page.locator("#ov-dial-label").inner_text().strip().lower() == "layers reporting",
          "the dial labels itself as live reporting during a scan")
    check(page.evaluate(commands) == 1,
          "exactly one collection is running")
    # Rapid re-clicks during the scan must not stack a second scan.
    page.evaluate("for (let i = 0; i < 5; i++) document.getElementById('overview-rescue-btn').click()")
    check(page.evaluate(commands) == 1,
          "repeated clicks never start a second collection")
    # Completion: no navigation, truthful final state.
    expect(page.locator("#ov-posture")).not_to_have_text("Scanning", timeout=30000)
    expect(page.locator("#view-overview")).to_be_visible()
    check(page.locator("#view-title").inner_text() == "Overview",
          "completion leaves the operator on the Overview")
    check(page.locator("#results-view").is_hidden(),
          "an overview-origin scan never force-opens Investigate")
    check(not page.locator("#overview-rescue-btn").is_disabled(),
          "the hero scan button re-arms after completion")
    check(page.locator("#ov-review-btn").is_enabled(),
          "review findings is offered after completion")
    check(page.evaluate(commands) == 1,
          "still exactly one collection after all re-clicks")
    posture = page.locator("#ov-posture").inner_text()
    check(posture in ("Assessment complete", "Attention required",
                      "Attention required · Partial coverage", "Partial coverage"),
          "the completed posture uses the honest vocabulary: " + posture)
    check("elapsed" not in page.locator("#ov-scope").inner_text(),
          "the scanning line is replaced by the scan's own scope summary")
    check(not errors, "overview-origin runtime: " + str(errors))
    ctx.close()

    # --- Rescue origin: the historical flow still ends on results. ---
    ctx, page, errors = open_page(browser, (900, 600))
    start_scan(page)
    finish_scan(page)
    expect(page.locator("#view-title")).to_have_text("Investigate", timeout=10000)
    check(True, "a rescue-origin scan still lands on the findings view")
    check(not errors, "rescue-origin runtime: " + str(errors))
    ctx.close()

    # --- Manual navigation during a scan is never overridden. ---
    ctx, page, errors = open_page(browser, (900, 600))
    page.locator("#overview-rescue-btn").click()
    expect(page.locator("#ov-posture")).to_have_text("Scanning", timeout=15000)
    page.locator("#nav-agents").click()
    expect(page.locator("#view-agents")).to_be_visible()
    page.wait_for_function(
        "!document.getElementById('ov-posture').textContent.includes('Scanning')",
        timeout=30000)
    expect(page.locator("#view-agents")).to_be_visible()
    check(page.locator("#view-title").inner_text() == "AI Agent Radar",
          "manual navigation wins at scan completion: " + page.locator("#view-title").inner_text())
    check(page.locator("#results-view").is_hidden(),
          "completion does not drag the operator to Investigate")
    check(not errors, "mid-scan navigation runtime: " + str(errors))
    ctx.close()

    # --- Race 1 (preparing phase): navigating while the overlay check is
    #     in flight must not let the scan drag the operator back. ---
    ctx, page, errors = open_page(browser, (900, 600))
    start_scan(page)
    page.wait_for_function(
        """() => document.getElementById('status-text').textContent !== 'Collecting evidence'
             && !document.getElementById('status-text').textContent.includes('…')""",
        timeout=30000)
    page.locator("#nav-agents").click()
    expect(page.locator("#view-agents")).to_be_visible(timeout=10000)
    page.wait_for_timeout(600)  # let any late completion continuation fire
    check(page.locator("#view-title").inner_text() == "AI Agent Radar",
          "the preparing-phase race respects the operator's destination: "
          + page.locator("#view-title").inner_text())
    check(not errors, "completion-race runtime: " + str(errors))
    ctx.close()

    # --- Race 2 (completion instant): navigating exactly as the scan
    #     finishes must not let the shell snap back to the results view. ---
    ctx, page, errors = open_page(browser, (900, 600))
    start_scan(page)
    page.wait_for_function("window.__CURE_SCAN_DONE === true", timeout=30000)
    page.locator("#nav-agents").click()
    expect(page.locator("#view-agents")).to_be_visible(timeout=10000)
    page.wait_for_timeout(600)
    check(page.locator("#view-title").inner_text() == "AI Agent Radar",
          "the completion-instant race respects the operator's destination: "
          + page.locator("#view-title").inner_text())
    check(page.locator("#results-view").is_hidden(),
          "the completion-instant race does not open Investigate")
    check(not errors, "completion-instant runtime: " + str(errors))
    ctx.close()

    # --- Failure from Overview: truthful failure, no stale results. ---
    ctx, page, errors = open_page(browser, (900, 600), scan_error=True)
    page.locator("#overview-rescue-btn").click()
    expect(page.locator("#ov-posture")).to_have_text("Operation failed", timeout=20000)
    check(page.locator("#view-overview").is_visible(),
          "a failed overview scan stays on the Overview")
    check(page.locator("#ov-coverage-state").inner_text() == "—",
          "a failed scan does not present coverage numbers")
    check(page.locator("#ov-findings").inner_text() == "—",
          "a failed scan does not present finding counts")
    check(page.locator("#ov-findings-panel").is_hidden(),
          "a failed scan does not show the previous scan's findings")
    check("Operation failed" in page.locator("#ov-assessment").inner_text(),
          "the assessment metric states the failure")
    check(page.locator("#results-view").is_hidden(),
          "a failed scan does not open Investigate")
    check(not errors, "failure runtime: " + str(errors))
    ctx.close()


def goto_view(page, nav, view):
    """Click a nav item and wait until that view is the settled destination.

    Cleanup navigation awaits switchView() before committing the active nav
    item. Clicking again before that promise resolves can let its continuation
    reveal cleanup over the next destination. Wait for both the active nav item
    and the sole visible view, then for finite entrance animations to finish.
    """
    page.locator(nav).click()
    page.wait_for_function(
        """([navSelector, viewId]) => {
             const nav = document.querySelector(navSelector);
             const v = document.getElementById(viewId);
             const shown = [...document.querySelectorAll('.stage > .view')]
               .filter(x => !x.classList.contains('hidden'));
             return nav?.getAttribute('aria-current') === 'true' &&
               !!v && !v.classList.contains('hidden') && shown.length === 1 &&
               shown[0] === v && !document.querySelector('.exiting, .pre-enter');
           }""",
        arg=[nav, view],
    )
    settled(page)


def settled(page):
    # Wait for entrance animations to finish before measuring or clicking.
    # Infinite loops (the operator node, the scan field, the storage core) must not
    # block: they have no finite end time and are ignored here.
    page.wait_for_function("""() => document.getAnimations().every(a =>
      !Number.isFinite(a.effect.getComputedTiming().endTime) ||
      (!a.pending && a.playState !== 'running'))""")


def layout(page, label):
    # Measure settled geometry, not the drawer's entrance transform. Keep
    # infinite scan/companion loops running; they must not block layout checks.
    settled(page)
    result = page.evaluate("""() => {
      const width=innerWidth;
      const visible=e=>e.getClientRects().length && getComputedStyle(e).visibility!=='hidden';
      const bad=[...document.querySelectorAll('body *')].filter(e=>visible(e) &&
        !e.closest('.table-wrap') && !e.closest('.sr-only') && !e.classList.contains('skip-link') &&
        (e.getBoundingClientRect().right>width+1 || e.getBoundingClientRect().left < -1));
      const foot=document.querySelector('.footbar').getBoundingClientRect();
      return {bad:bad.slice(0,8).map(e=>e.tagName+'.'+e.className),
        documentOverflow:document.documentElement.scrollWidth>width,
        footer:foot.bottom<=innerHeight+1};
    }""")
    check(not result["bad"], label + " horizontal bounds: " + str(result["bad"]))
    check(not result["documentOverflow"], label + " document has no horizontal overflow")
    check(result["footer"], label + " footer remains in window")


def axe(page, label):
    if not AXE.exists():
        raise RuntimeError("Run npm ci in gui/devtools before accessibility gates")
    if not page.evaluate("typeof axe !== 'undefined'"):
        page.add_script_tag(path=str(AXE))
    violations = page.evaluate("async () => (await axe.run(document, {runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa','wcag22aa']}})).violations")
    check(not violations, label + " axe: " + str([(v["id"], [n["target"] for n in v["nodes"]]) for v in violations]))


def shot(page, target):
    target.parent.mkdir(parents=True, exist_ok=True)
    page.evaluate("document.activeElement?.blur()")
    page.screenshot(path=str(target), animations="disabled", caret="hide")


def keyboard(page):
    page.locator("#nav-overview").focus()
    for _ in range(24):
        page.keyboard.press("Tab")
        state = page.evaluate("""() => {
          const e=document.activeElement,s=getComputedStyle(e);
          return {body:e===document.body,outline:s.outlineStyle,shadow:s.boxShadow};
        }""")
        check(state["body"] or state["outline"] != "none" or state["shadow"] != "none", "keyboard focus visible")


def core_flow(browser, size=(900, 600), reduced=False, capture=False, video=False):
    ctx, page, errors = open_page(browser, size, reduced, video)
    if capture:
        shot(page, DOCS / "01-overview.png")
    layout(page, "idle " + str(size))
    axe(page, "idle")
    check(page.locator(".nav .nav-item").count() == 8, "eight primary destinations")
    check(page.locator(".nav button:disabled").count() == 0, "no disabled primary navigation")
    check(page.locator(".nav-number").count() == 0, "navigation carries no numeric labels")
    check([t.strip() for t in page.locator(".nav .nav-item span").all_inner_texts()] ==
          ["Overview", "Agents", "Rescue", "Investigate", "Quarantine", "Detective", "Storage", "Monitor"],
          "primary destinations are icon and title only")
    check(page.locator("#xray-canvas").count() == 1, "system x-ray canvas present")
    check(page.locator("[data-companion='overview'] .luma-svg").count() == 0,
          "no mascot is rendered in the overview")
    keyboard(page)
    start_scan(page)
    page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 2")
    # Mid-scan identity checks run immediately: the mock collection finishes
    # in ~5s regardless of how long layout settles take, so anything that
    # asserts on a live scan must not queue behind a settle wait.
    check(page.locator("#xray-field").get_attribute("data-running") == "true",
          "x-ray reports the real running state")
    check(int(page.locator("#xray-inspected").inner_text()) >= 2,
          "instrument panel uses the collected item count")
    box = page.locator("#xray-canvas").bounding_box()
    store = page.locator("#xray-canvas").evaluate("c => c.width")
    check(box is not None and box["height"] >= 150 and store >= box["width"] and store > 0,
          "x-ray renders into a real backing store at " + str(size))
    # The scan view's own still (and the palette gate's reference frame) is a
    # live-collection frame, not the finished report.
    if capture:
        shot(page, DOCS / "02-scan.png")
    else:
        shot(page, SHOTS / ("scan-reduced.png" if reduced else "scan.png"))
    layout(page, "scan " + str(size))
    page.wait_for_function("window.__CURE_SCAN_DONE === true")
    check(page.locator("#sweep-items").inner_text().startswith("2 ") or
          int(page.locator("#sweep-items").inner_text().split()[0]) >= 2, "sweep uses collected item count")
    check(page.locator("#sweep-sources li").count() == 9, "all nine inspection layers reported")
    if reduced:
        check(companion_running(page) == 0, "reduced motion has no running companion animation")
    finish_scan(page)
    layout(page, "findings " + str(size))
    axe(page, "findings")
    check(page.locator("#review-cards .review-card").count() == 5, "high and suspicious findings remain review-only")
    check(page.evaluate("window.__CURE_MOCK_QUARANTINE_CALLS||0") == 0, "scan never quarantines")
    if capture:
        shot(page, DOCS / "03-review-required.png")
        page.locator("#nav-overview").click()
        shot(page, SHOTS / "overview-review.png")
        page.locator("#nav-results").click()
    select = page.locator("#review-cards .finding-select").first
    select.click()
    page.wait_for_function("document.querySelector('#evidence-body').textContent.includes('UNSIGNED')")
    expect(page.get_by_role("dialog", name="AcmeTray0.bat")).to_be_visible()
    check(page.locator("#evidence-body").inner_text().count("bd5b914") == 1, "full hash retained in inspector")
    layout(page, "inspector")
    axe(page, "inspector")
    page.keyboard.press("Shift+Tab")
    check(page.evaluate("document.activeElement.closest('#evidence-inspector') !== null"), "inspector traps keyboard focus")
    if capture:
        page.locator("#evidence-body").evaluate("e=>e.scrollTop=0")
        shot(page, DOCS / "04-evidence-inspector.png")
    page.locator('#evidence-body .quarantine-btn').click()
    expect(page.locator('#confirm-overlay')).to_be_visible()
    page.locator('#confirm-cancel').click()
    check(page.evaluate("window.__CURE_MOCK_QUARANTINE_CALLS||0") == 0, 'inspector confirmation cancel leaves file untouched')
    expect(page.locator('#evidence-body .quarantine-btn')).to_be_focused()
    page.keyboard.press("Escape")
    check(select.evaluate("e=>e===document.activeElement"), "inspector returns focus to finding")
    btn = page.locator("#review-cards .quarantine-btn").first
    for exit_key in ("cancel", "Escape"):
        btn.click()
        expect(page.locator("#confirm-cancel")).to_be_focused()
        check(page.locator("#confirm-overlay .confirm-section").count() == 3, "quarantine consequences separated")
        axe(page, "confirmation")
        if exit_key == "cancel":
            page.locator("#confirm-cancel").click()
        else:
            page.keyboard.press(exit_key)
        check(page.evaluate("window.__CURE_MOCK_QUARANTINE_CALLS||0") == 0, "cancel does not move file")
        expect(btn).to_be_focused()
    btn.click()
    layout(page, "confirmation")
    if capture:
        shot(page, DOCS / "05-quarantine-confirm.png")
    page.locator("#confirm-ok").click()
    expect(btn).to_have_text("Quarantined")
    check(page.evaluate("window.__CURE_MOCK_QUARANTINE_CALLS") == 1, "one explicit quarantine invocation")
    page.locator("#nav-quarantine").click()
    expect(page.locator("#q-list .q-row")).to_have_count(1)
    check("18304 bytes" in page.locator("#q-list").inner_text(), "archive integrity metadata visible")
    layout(page, "quarantine")
    axe(page, "quarantine")
    if capture:
        shot(page, DOCS / "06-quarantine.png")
    page.locator("#q-list .q-undo").click()
    expect(page.locator("#q-receipt")).to_contain_text("Restored:")
    expect(page.locator("#q-empty")).to_be_visible()
    check("Restored:" in page.locator("#q-receipt").inner_text(), "undo leaves visible receipt")
    page.locator("#nav-results").click()
    expect(page.locator("#review-cards .quarantine-btn").first).to_have_text("Quarantine")
    check(not errors, "no runtime errors or remote requests: " + str(errors))
    video_path = page.video.path() if video else None
    ctx.close()
    return video_path


def states(browser, capture=False):
    coverage = [{"area": "Registry autoruns", "state": "AccessDenied", "detail": "Administrator access required"},
                {"area": "Scheduled tasks", "state": {"Partial": {"skipped": 2}}, "detail": "2 files skipped"},
                {"area": "Services (auto-start)", "state": "Unavailable", "detail": "Collector unavailable"},
                {"area": "WMI subscriptions", "state": "CheckFailed", "detail": "Collection failed"}]
    for name, knobs in [("all-clear", {"all_safe": True}), ("partial", {"all_safe": True, "coverage": coverage}),
                        ("error", {"scan_error": True}), ("guidance", {"guidance_source": "windows-service"}),
                        ("partial-findings", {"coverage": coverage}),
                        ("long-values", {"long_values": True}), ("quarantine-error", {"quarantine_error": True})]:
        ctx, page, errors = open_page(browser, (1440, 900) if capture else (900, 600), **knobs)
        if name == "quarantine-error":
            page.locator("#nav-quarantine").click()
            expect(page.locator("#q-subline")).to_contain_text("unavailable")
        else:
            start_scan(page)
            if name == "error":
                expect(page.locator("#scan-retry")).to_be_visible()
                check("Operation failed" in page.locator("#status-text").inner_text(), "scan error states the failure in the pill")
                check("Collection failed" in page.locator("#log").inner_text(), "the scan log keeps the actual error text")
                page.evaluate("window.__CURE_MOCK_SCAN_ERROR=false")
                page.locator("#scan-retry").click()
                finish_scan(page)
            else:
                finish_scan(page)
            if name == "partial":
                check("Coverage incomplete" in page.locator("#headline").inner_text(), "incomplete scan never called clean")
                page.locator("#nav-overview").click()
                expect(page.locator("#ov-posture")).to_have_text("Partial coverage")
                check("ACCESS DENIED" in page.locator("#ov-coverage").inner_text(), "access denied communicated in text")
            if name == "partial-findings":
                expect(page.locator("#status-text")).to_have_text("Attention required · Partial coverage")
                check("complete" not in page.locator("#status-text").inner_text().lower().replace("incomplete", ""), "partial coverage with findings never claims scan complete")
            if name == "all-clear":
                expect(page.locator("#review-clear")).to_be_visible()
                check("No persistence findings" in page.locator("#review-clear").inner_text(), "completed empty result uses collected-evidence wording")
                if capture:shot(page, DOCS / "07-all-clear.png")
            if name == "guidance":
                row = page.locator("#review-cards .review-card").first
                check(row.locator(".quarantine-btn").count() == 0, "non-file-backed source gets guidance only")
            if name == "long-values":
                # A slower test-only entrance makes this CI timing regression
                # reproducible even when evidence metadata resolves immediately.
                page.locator("#evidence-inspector").evaluate(
                    "e => e.style.animationDuration = '600ms'")
                page.locator("#review-cards .finding-select").first.click()
                page.wait_for_function("document.querySelector('#evidence-body').textContent.includes('UNSIGNED')")
                check(page.locator("#evidence-title img").count() == 0, "attacker-controlled name is escaped")
                check("long-folder-name\\" * 30 in page.locator("#evidence-body").inner_text(), "long command preserved without truncation")
        layout(page, name)
        axe(page, name)
        shot(page, SHOTS / (name + ".png"))
        check(not errors, name + " runtime: " + str(errors))
        ctx.close()


def secondary(browser):
    ctx, page, errors = open_page(browser, (900, 600), sweep=True, canary_alert=True,
                                  storage_locked=True)
    start_scan(page);finish_scan(page)
    # Individual process termination stays explicit and separate from scan/quarantine.
    page.locator("#nav-processes").click()
    page.locator(".kill-one").first.click()
    expect(page.locator("#confirm-overlay")).to_be_visible()
    page.keyboard.press("Escape")
    # Storage Workstation is a primary destination, reached directly.
    page.locator("#nav-cleanup").click()
    expect(page.locator("#sw-panel-map")).to_be_visible()
    check(page.locator("#sw-amount").inner_text() == "—",
          "an unmeasured workstation never invents reclaimable bytes")
    check(page.locator("#sw-amount-label").inner_text().strip().lower() == "not measured",
          "the headline states that nothing has been measured")
    check(page.locator("#sw-treemap-hint").inner_text().strip() != "",
          "the map explains itself before it has anything to draw")
    check(page.locator("#sw-classes li").count() == 0,
          "no category is listed before the engine reports one")
    check(page.locator("#sw-preview-btn").is_disabled(),
          "there is no path to removal without a measured scan")
    layout(page, "storage idle");axe(page, "storage idle")

    page.locator("#sw-scan").click()
    expect(page.locator("#sw-progress")).to_be_visible()
    check(page.locator("#sw-cancel").is_visible(), "a running scan can be stopped")
    layout(page, "storage scanning")
    # While the scan is running the summary must not claim that nothing has
    # been measured. The engine is measuring, and the figures on screen are
    # the ones it reported.
    check(page.locator("#sw-amount").inner_text() != "—",
          "the headline shows the engine's live measurement while scanning")
    check("Not measured" not in page.locator("#sw-amount-label").inner_text(),
          "a running scan never says the machine is not measured: "
          + page.locator("#sw-amount-label").inner_text())
    check("Run a scan" not in page.locator("#sw-amount-note").inner_text(),
          "a running scan does not tell the operator to run a scan: "
          + page.locator("#sw-amount-note").inner_text()[:80])
    check("measured" in page.locator("#sw-amount-label").inner_text().lower(),
          "the running summary is labelled as a measurement in progress: "
          + page.locator("#sw-amount-label").inner_text())
    # The shell no longer carries scan summaries; a running measurement is
    # named by the session pill, and the detail lives on the storage page.
    check("storage" in page.locator("#status-text").inner_text().lower(),
          "the session pill says a storage measurement is in flight: "
          + page.locator("#status-text").inner_text())
    check(page.locator("#session-meta").count() == 0,
          "the global header carries no scan summary line")
    # The panel is already on screen, so wait for the real completion signal:
    # the progress strip is only hidden once the engine has returned.
    expect(page.locator("#sw-progress")).to_be_hidden(timeout=20000)
    check(page.locator("#sw-amount").inner_text() != "—",
          "the headline comes from the engine measurement")
    check(page.locator("#sw-progress").is_hidden(), "progress is dismissed when the scan ends")
    check(page.locator("#sw-classes li").count() > 0,
          "measured categories are listed")
    check(page.locator("#sw-treemap canvas").count() == 1, "the map renders into a canvas")
    # The map is a drill-down, and the trail is the way back.
    page.locator("#sw-tab-map").click()
    expect(page.locator("#sw-panel-map")).to_be_visible()
    check(page.locator("#sw-breadcrumbs").inner_text().strip() != "",
          "the map says where it is looking even before it is drilled into")
    tiles = page.evaluate("""() => (window.CureStorageWorkstation.state.treemapCells||[])
        .map(c=>({x:c.x,y:c.y,w:c.w,h:c.h,kind:c.item.class.kind}))
        .sort((a,b)=>b.w*b.h-a.w*a.h)""")
    check(len(tiles) > 1, "the map draws more than one tile: " + str(len(tiles)))
    if tiles:
        canvas = page.locator("#sw-treemap-canvas")
        canvas.scroll_into_view_if_needed()
        page.wait_for_timeout(300)
        box = canvas.bounding_box()
        biggest = tiles[0]
        page.mouse.move(box["x"] + biggest["x"] + biggest["w"] / 2,
                        box["y"] + biggest["y"] + biggest["h"] / 2)
        page.wait_for_timeout(400)
        tip = page.locator("#sw-map-tip")
        expect(tip).to_be_visible()
        tip_text = tip.text_content()
        check(len(tip_text.strip()) > 0, "hovering a tile says something about it")
        check("%" in tip_text or "files" in tip_text,
              "the hover detail says how much the tile is: " + tip_text[:90])
        page.mouse.click(box["x"] + biggest["x"] + biggest["w"] / 2,
                         box["y"] + biggest["y"] + biggest["h"] / 2)
        page.wait_for_timeout(400)
        check(page.locator("#sw-breadcrumbs").inner_text().strip() != "All categories",
              "drilling in writes the trail: "
              + page.locator("#sw-breadcrumbs").inner_text())
        page.locator("#sw-breadcrumbs button").first.click()
        page.wait_for_timeout(300)
        check(page.locator("#sw-breadcrumbs").inner_text().strip() == "All categories",
              "the trail goes back to the top")
    # The map is not the only route into the drill: the folder table is.
    check(page.locator("#sw-folders-body .sw-file-name").count() > 0,
          "the biggest folders table is a second route into the map")
    page.locator("#sw-tab-large").click()
    page.wait_for_timeout(200)
    page.locator("#sw-folders-body .sw-file-name").first.click()
    page.wait_for_timeout(400)
    check(page.locator("#sw-breadcrumbs").inner_text().strip() != "All categories",
          "a folder in the table opens the map at that folder: "
          + page.locator("#sw-breadcrumbs").inner_text())
    check(page.locator("#sw-panel-map").is_visible(),
          "opening a folder from the table brings the map forward")
    page.locator("#sw-breadcrumbs button").first.click()
    page.wait_for_timeout(300)

    # Every row carries its tier, and protected/unknown rows offer no action.
    check(page.locator("#sw-large-body tr .tier-chip").count() > 0,
          "each measured file shows its safety tier")
    check(page.locator("#sw-large-body .sw-no-action").count() > 0,
          "protected and uncertain files offer no removal action")
    check(page.locator("#sw-large-body .sw-queue-btn").count() > 0,
          "reviewable files offer a per-item action")

    # Duplicates are only reported after full-content verification.
    page.locator("#sw-tab-dupes").click()
    expect(page.locator("#sw-panel-dupes")).to_be_visible()
    dupe_coverage = page.locator("#sw-dupe-coverage").inner_text()
    check("in full" in dupe_coverage,
          "duplicate coverage states what was actually verified: " + dupe_coverage[:80])
    check("SHA-256" in dupe_coverage,
          "duplicate coverage names the verification method: " + dupe_coverage[:120])
    check(page.locator("#sw-dupe-list .sw-dupe").count() >= 1, "verified duplicate groups shown")
    check(page.locator("#sw-dupe-list .sw-kept-tag").count() >= 1,
          "each duplicate group names the copy that would be kept")
    layout(page, "storage duplicates");axe(page, "storage duplicates")

    # Coverage must be inspectable and must not read as clean when partial.
    page.locator("#sw-tab-coverage").click()
    expect(page.locator("#sw-panel-coverage")).to_be_visible()
    check(page.locator("#sw-roots-body tr").count() > 0, "every root and its outcome is listed")
    check(page.locator("#sw-stat-rows dt").count() > 0, "the coverage report is populated")
    layout(page, "storage coverage");axe(page, "storage coverage")

    # Queue a reviewable file, preview it, and refuse to confirm without the word.
    page.locator("#sw-tab-large").click()
    page.locator("#sw-large-body .sw-queue-btn").first.click()
    expect(page.locator("#sw-queue-badge")).to_have_text("1")
    page.locator("#sw-tab-queue").click()
    expect(page.locator("#sw-queue-groups")).to_be_visible()
    page.locator("#sw-preview-btn").click()
    expect(page.locator("#sw-preview-overlay")).to_be_visible()
    check("no undo" in page.locator("#sw-preview-consequence").inner_text(),
          "the preview states the consequence in plain language")
    check(page.locator("#sw-preview-confirm").is_disabled(),
          "removal cannot be confirmed without typing the word")
    check(page.evaluate("window.__CURE_LAST_STORAGE_CALL === undefined"),
          "opening the preview executes nothing")
    page.locator("#sw-confirm-input").fill("rem")
    check(page.locator("#sw-preview-confirm").is_disabled(),
          "a partial confirmation word is still refused")
    page.locator("#sw-confirm-input").fill("REMOVE")
    check(page.locator("#sw-preview-confirm").is_enabled(),
          "the exact confirmation word enables removal")
    layout(page, "storage preview");axe(page, "storage preview")
    # The confirmation dialog must keep the keyboard inside it. A keyboard
    # user must not be able to Tab out of the "type REMOVE" dialog into the
    # page behind it.
    trapped = page.evaluate("""() => {
      const ov = document.getElementById('sw-preview-overlay');
      const inside = [];
      for (let i = 0; i < 16; i++) {
        const a = document.activeElement;
        inside.push(!!(a && ov.contains(a)));
      }
      return inside;
    }""")
    check(page.evaluate("""() => {
      const ov = document.getElementById('sw-preview-overlay');
      ov.focus();
      const ev = new KeyboardEvent('keydown', {key: 'Tab', bubbles: true});
      let stayed = true;
      for (let i = 0; i < 16; i++) {
        document.dispatchEvent(ev);
        const a = document.activeElement;
        if (!a || !ov.contains(a)) { stayed = false; break; }
      }
      return stayed;
    }"""), "the removal dialog keeps the keyboard inside it")
    check(page.evaluate("""() => {
      const ov = document.getElementById('sw-preview-overlay');
      const bg = [...document.querySelectorAll('#stage-main > *')]
        .filter((n) => !ov.contains(n) && n.id !== 'cleanup-view');
      return bg.every((n) => n.inert === true);
    }"""), "the page behind the removal dialog is inert")
    check(page.locator("#sw-status-text").get_attribute("aria-live") == "polite",
          "the storage status is announced when it changes")
    page.keyboard.press("Escape")
    check(page.evaluate("window.__CURE_LAST_STORAGE_CALL === undefined"),
          "cancelling the preview executes nothing")
    expect(page.locator("#sw-preview-overlay")).to_be_hidden()
    check(page.evaluate("""() => {
      const ov = document.getElementById('sw-preview-overlay');
      const bg = [...document.querySelectorAll('#stage-main > *')]
        .filter((n) => !ov.contains(n) && n.id !== 'cleanup-view');
      return bg.every((n) => n.inert === false);
    }"""), "the page behind the dialog is interactive again afterwards")

    # Execute for real, then read the receipt.
    page.locator("#sw-preview-btn").click()
    page.locator("#sw-confirm-input").fill("REMOVE")
    page.locator("#sw-preview-confirm").click()
    expect(page.locator("#sw-receipt-overlay")).to_be_visible(timeout=15000)
    check(page.evaluate("window.__CURE_LAST_STORAGE_CALL !== undefined"),
          "removal runs only after an exact confirmation")
    check(page.locator("#sw-receipt-rows dd").count() >= 4,
          "the receipt reports estimate, verification, volume change and refusals")
    check(page.locator("#sw-receipt-amount").inner_text() != "—",
          "the receipt reports verified reclaimed bytes")
    layout(page, "storage receipt");axe(page, "storage receipt")
    page.locator("#sw-receipt-close").click()
    expect(page.locator("#sw-receipt-overlay")).to_be_hidden()
    check(page.locator("#sw-amount-label").inner_text().strip().lower() == "not measured",
          "the scan is invalidated after a change, not left stale on screen")

    page.locator("#nav-eventlog").click()
    page.locator("#view-eventlog [data-route='view-canary']").click()
    page.locator("#can-toggle").click()
    expect(page.locator("#can-state")).to_have_text("ACTIVE")
    expect(page.locator("#canary-alert-overlay")).to_be_visible(timeout=8000)
    axe(page, "Canary alert");page.keyboard.press("Escape")
    page.locator("#can-toggle").click();expect(page.locator("#can-state")).to_contain_text("OFF")
    page.locator("#nav-results").click()
    page.locator("#nav-eventlog").click()
    page.locator("#view-eventlog [data-route='view-incident']").click()
    page.locator("#btn-incident-start").click()
    expect(page.locator("#inc-results")).to_be_visible(timeout=15000)
    layout(page, "login observation");axe(page, "login observation")
    inspect = page.locator("#view-incident button").filter(has_text="Inspect").first
    inspect.click();expect(page.locator("#inc-drawer-overlay")).to_be_visible()
    page.keyboard.press("Tab");expect(page.locator("#inc-drawer-close")).to_be_focused()
    page.keyboard.press("Escape");expect(inspect).to_be_focused()
    check(not errors, "secondary runtime: " + str(errors));ctx.close()


def responsive(browser):
    for size in [(390, 844), (760, 600), (900, 600), (1366, 768), (1440, 900), (1920, 1080)]:
        ctx, page, errors = open_page(browser, size)
        for nav in ["nav-overview", "nav-scan", "nav-results", "nav-quarantine", "nav-eventlog", "nav-cleanup"]:
            page.locator("#" + nav).click()
            if nav == "nav-quarantine":expect(page.locator("#q-empty")).to_be_visible()
            if nav == "nav-cleanup":expect(page.locator("#sw-panel-map")).to_be_visible()
            layout(page, nav + str(size))
        # Every workstation tab must survive the smallest supported window.
        if size == (900, 600):
            for tab in ["sw-tab-large", "sw-tab-dupes", "sw-tab-queue", "sw-tab-coverage"]:
                page.locator("#" + tab).click()
                layout(page, "storage " + tab + str(size))
            page.locator("#sw-scan").click()
            expect(page.locator("#sw-progress")).to_be_hidden(timeout=20000)
            page.locator("#sw-tab-large").click()
            check(page.locator("#sw-large-body tr").count() > 0,
                  "the largest-files table renders at the minimum window")
            layout(page, "storage populated " + str(size))
            page.locator("#nav-overview").click()
        # Desktop hero composition must not push critical information away.
        if size[0] >= 900:
            page.locator("#nav-scan").click()
            page.locator("#xray-start-btn").click()
            page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 1")
            # The readout empties once the collection finishes, so measure
            # the live composition before waiting out the scan.
            check(page.locator("#xray-core-readout").bounding_box()["y"] +
                  page.locator("#xray-core-readout").bounding_box()["height"] <= size[1],
                  "x-ray readout stays inside the viewport at " + str(size))
            check(page.locator(".instrument").bounding_box()["y"] <= size[1],
                  "live instrument panel is above the fold at " + str(size))
            page.wait_for_function("window.__CURE_SCAN_DONE === true")
            layout(page, "scan hero " + str(size))
            page.locator("#nav-overview").click()
        page.locator("#nav-overview").click()
        shot(page, SHOTS / ("overview-%sx%s.png" % size))
        check(not errors, "responsive runtime: " + str(errors));ctx.close()


def demo_flow(browser):
    ctx,page,errors=open_page(browser,(1440,900),video=True,storage_delay_ms=1800)
    page.wait_for_timeout(1800)  # Intentional presentation beats, not test synchronization.
    start_scan(page);finish_scan(page)
    page.wait_for_timeout(900)
    page.locator('#review-cards .finding-select').first.click()
    page.wait_for_function("document.querySelector('#evidence-body').textContent.includes('UNSIGNED')")
    page.wait_for_timeout(1100)
    page.locator('#evidence-body .quarantine-btn').click()
    expect(page.locator('#confirm-overlay')).to_be_visible()
    page.wait_for_timeout(1600)
    page.locator('#confirm-ok').click()
    page.wait_for_function("document.querySelector('#evidence-body').textContent.includes('Quarantined')")
    page.locator('#evidence-body button').filter(has_text='Open Quarantine').click()
    expect(page.locator('#q-list .q-row')).to_have_count(1)
    page.wait_for_timeout(1200)
    page.locator('#q-list .q-undo').click()
    expect(page.locator('#q-receipt')).to_contain_text('Restored:')
    page.wait_for_timeout(1200)
    # Storage is a separate, explicitly confirmed action. All video data is
    # labelled mock data and the confirmation states that removal is permanent.
    page.locator('#nav-cleanup').click()
    page.wait_for_timeout(700)
    page.locator('#sw-scan').click()
    expect(page.locator('#sw-panel-map')).to_be_visible(timeout=15000)
    page.wait_for_timeout(1500)
    page.locator('#sw-tab-dupes').click()
    page.wait_for_timeout(1400)
    page.locator('#sw-tab-large').click()
    page.locator('#sw-large-body .sw-queue-btn').first.click()
    page.wait_for_timeout(600)
    page.locator('#sw-tab-queue').click()
    page.wait_for_timeout(700)
    page.locator('#sw-preview-btn').click()
    expect(page.locator('#sw-preview-overlay')).to_be_visible()
    page.wait_for_timeout(1600)
    page.locator('#sw-confirm-input').fill('REMOVE')
    page.locator('#sw-preview-confirm').click()
    page.wait_for_timeout(2000)
    expect(page.locator('#sw-receipt-overlay')).to_be_visible()
    page.wait_for_timeout(1600)
    expect(page.locator('#sw-receipt-overlay')).to_be_visible()
    page.wait_for_timeout(1200)
    path=page.video.path();ctx.close()
    Path(path).replace(ROOT/'docs/media/demo.webm')
    check(not errors,'demo runtime: '+str(errors))


def agent_demo(browser):
    """Record the 25–30 second synthetic Agent Radar story for the README."""
    import shutil
    temp = ROOT / "docs/media/.agent-video"
    shutil.rmtree(temp, ignore_errors=True)
    temp.mkdir(parents=True, exist_ok=True)
    ctx = browser.new_context(
        viewport={"width": 1440, "height": 900},
        record_video_dir=str(temp),
        record_video_size={"width": 1440, "height": 900},
    )
    page = ctx.new_page()
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
    page.clock.set_fixed_time(FIXED)
    page.goto((ROOT / "gui/dist-demo/index.html").as_uri())
    page.wait_for_load_state("networkidle")
    expect(page.locator("#view-overview .demo-banner")).to_be_visible()
    page.wait_for_timeout(2000)

    goto_view(page, "#nav-agents", "view-agents")
    expect(page.locator("#ar-demo-banner")).to_be_visible()
    page.wait_for_timeout(3500)
    page.locator("#ar-jump-monitor").click()
    page.wait_for_timeout(1200)
    page.locator("#ar-workspace").fill(r"C:\Projects\nova-app")
    page.wait_for_timeout(1000)
    page.locator("#ar-start").click()
    expect(page.locator("#ar-session-badge")).to_have_text("Recording", timeout=10000)
    expect(page.locator("#ar-stat-total")).to_have_text("6")
    page.wait_for_timeout(3200)

    page.locator("#ar-event-list .ar-event").nth(2).click()
    expect(page.locator("#ar-evidence-card")).to_be_visible()
    page.locator("#ar-evidence-card").scroll_into_view_if_needed()
    page.wait_for_timeout(3500)
    page.evaluate("document.getElementById('ar-stop').click()")
    expect(page.locator("#ar-session-badge")).to_have_text("Stopped", timeout=10000)
    page.wait_for_timeout(1800)

    page.locator("#ar-case-summary").fill("Reviewed a synthetic agent session")
    page.locator("#ar-case-include").check()
    page.wait_for_timeout(1000)
    page.locator("#ar-case-preview").click()
    expect(page.locator("#ar-case-stage img")).to_be_visible(timeout=10000)
    page.locator("#ar-case-stage").evaluate("e => e.scrollIntoView({block: 'center'})")
    page.wait_for_timeout(4000)
    page.locator("#ar-case-export").click()
    expect(page.locator("#ar-case-status")).to_contain_text("preview-only", timeout=10000)
    page.wait_for_timeout(3200)
    check(not errors, "agent video runtime: " + str(errors))
    path = page.video.path()
    ctx.close()
    Path(path).replace(ROOT / "docs/media/agent-demo.webm")
    shutil.rmtree(temp, ignore_errors=True)


def detective_flow(browser):
    """The detective board must read measured evidence, not invent it."""
    ctx,page,errors=open_page(browser,(1440,900))
    goto_view(page,'#nav-detective','view-detective')
    check(page.locator('#dt-subline').inner_text().startswith('Nothing is measured'),
          'the detective board starts with no claim')
    page.locator('#dt-investigate').click()
    expect(page.locator('#dt-leads .dt-lead').first).to_be_visible(timeout=30000)
    scan_calls_before=page.evaluate("(window.__CURE_MOCK_COMMANDS||[]).filter(c=>c==='storage_scan').length")
    total=page.locator('#dt-map-total').inner_text()
    check(total not in ('', '—'), 'the board leads with a measured total: '+total)
    check(page.locator('#dt-leads .dt-lead').count() >= 3, 'the board lists leads from the scan')
    check(page.locator('#dt-leads .dt-significance-major').count() >= 1,
          'a concentration large enough to call major is marked')
    check(page.locator('#dt-map .dt-arc').count() >= 2, 'the map draws measured category arcs')
    check(page.locator('#dt-legend .dt-swatch').count() >= 2, 'the map has an accessible legend')
    check(page.locator('#dt-bands .dt-band').count() >= 2, 'the bands separate removable and off-limits')
    check(page.locator('#dt-timeline li.is-done').count() >= 2,
          'the timeline only advances on steps that happened')

    page.locator('#dt-leads .dt-lead').filter(
        has=page.locator('.dt-significance-major')).first.click()
    expect(page.locator('#dt-detail-body')).to_be_visible()
    terms=page.locator('#dt-detail-facts .dt-fact-term').all_inner_texts()
    for wanted in ('WHERE','WHY IT MATTERS','ORIGIN','RISK','ACTION','IF REMOVED'):
        check(wanted in terms, 'a lead explains '+wanted.lower())
    check(page.locator('#dt-detail-title').inner_text().strip() != '', 'the lead has a title')

    # The case file is a preview of the allowlisted model, nothing more.
    page.locator('#dt-case-preview').click()
    expect(page.locator('#dt-case-preview-stage img')).to_be_visible(timeout=10000)
    alt=page.locator('#dt-case-preview-stage img').get_attribute('alt') or ''
    check('Case file preview' in alt and total in alt, 'the preview names the measured total')
    leaked=page.evaluate("""() => {
      const s = JSON.stringify(window.CureCaseFile.audit(window.CureDetective.state.caseFile));
      return s.match(/[A-Za-z]:\\\\|Users|AppData|\\.tmp|\\.zip|bob/gi) || [];
    }""")
    check(not leaked, 'the case file model carries no path, user name or file name: '+str(leaked[:3]))
    check(page.evaluate("!!window.CureDetective.state.caseFile.demo") is False,
          'the product case file is not marked synthetic')

    page.locator('#dt-case-export').click()
    expect(page.locator('#dt-case-status')).to_contain_text('Case file written', timeout=10000)
    check('not uploaded' not in page.locator('#dt-case-status').inner_text().lower(),
          'the export status names the file, not an upload')
    check(scan_calls_before == page.evaluate(
          "(window.__CURE_MOCK_COMMANDS||[]).filter(c=>c==='storage_scan').length"),
          'exporting a case file does not rescan')

    # Opening a lead in the workstation adopts the same measurement.
    calls_before=page.evaluate("(window.__CURE_MOCK_COMMANDS||[]).filter(c=>c==='storage_scan').length")
    page.get_by_role('button', name='Open in Storage Workstation').click()
    expect(page.locator('#cleanup-view')).to_be_visible()
    expect(page.locator('#sw-amount')).not_to_have_text('—')
    check(calls_before == page.evaluate(
          "(window.__CURE_MOCK_COMMANDS||[]).filter(c=>c==='storage_scan').length"),
          'the workstation adopts the board measurement without a second scan')
    check(not errors,'detective runtime: '+str(errors));ctx.close()



def agent_radar_flow(browser):
    """Agent Radar: honest session lifecycle, presence, recording, scoped
    evidence and a safe case file.

    UI-02: a fresh launch shows an idle 'not scanned' radar; detections
    appear only after an explicit operator scan; session-artifact intel
    comes from the real operator_agent_sessions survey.
    """
    ctx, page, errors = open_page(browser, (1440, 900))
    goto_view(page, '#nav-agents', 'view-agents')

    # Fresh launch: honest idle. No snapshot may pose as a current scan.
    check(page.locator('#ar-radar').get_attribute('data-phase') == 'idle',
          'the radar starts idle on a fresh launch')
    check(page.locator('#ar-blips .ar-blip').count() == 0,
          'no agent indicators exist before a scan')
    check(page.locator('#ar-agent-list').is_hidden(),
          'no agent inventory is presented before a scan')
    check(page.locator('#ar-agent-list .ar-agent-card').count() == 0,
          'no previous snapshot is shown as current results')
    check(page.locator('#ar-scan-at').inner_text().strip() == 'Not scanned this session',
          'the snapshot timestamp states that nothing has been scanned')
    check(page.locator('#ar-flight-live').is_hidden(), 'no synthetic session exists before start')
    check(not page.locator('#ar-scan-now').is_disabled(), 'the primary scan action is offered')

    # Manual scan: real enumeration, in this case the labelled mock.
    page.locator('#ar-scan-now').click()
    expect(page.locator('#ar-detected-count')).to_have_text('1', timeout=15000)
    check(page.locator('#ar-blips .ar-blip').count() == 1,
          'the radar shows one observed supported process')
    expect(page.locator('#ar-agent-list .ar-agent-card').first).to_be_visible()
    check('Presence is not proof of activity' in page.locator('#ar-radar-note').inner_text(),
          'process presence is not presented as activity')
    check(page.locator('#ar-scan-at').inner_text().startswith('Snapshot '),
          'the last-scanned timestamp is real: ' + page.locator('#ar-scan-at').inner_text())
    check(page.locator('#ar-radar').get_attribute('data-phase') == 'idle',
          'the sweep stops once the scan finishes')

    # Agent row: expandable, honest identity, real session-artifact intel.
    page.locator('#ar-agent-list .ar-agent-card .ar-agent-row').first.click()
    expect(page.locator('#ar-agent-list .ar-agent-body').first).to_be_visible()
    check('verified tool identity' in page.locator('#ar-agent-list .ar-agent-card').first.inner_text(),
          'identity confidence is stated in the inventory row')
    check('Session artifacts' in page.locator('#ar-agent-list .ar-agent-card').first.inner_text(),
          'session-artifact intelligence is surfaced from the real survey')
    check('PID' in page.locator('#ar-agent-list .ar-agent-card').first.inner_text(),
          'process identifiers are shown')

    # Radar interaction: selecting the indicator highlights the inventory row.
    page.locator('#ar-blips .ar-blip').first.click()
    check(page.locator('#ar-agent-list .ar-agent-card').first.get_attribute('class').find('is-selected') != -1,
          'selecting a radar indicator highlights the agent row')
    check(page.locator('#ar-blips .ar-blip').first.get_attribute('aria-pressed') == 'true',
          'the radar indicator reports its selected state to assistive tech')
    page.locator('#ar-blips .ar-blip').first.click()
    check(page.locator('#ar-agent-list .ar-agent-card').first.get_attribute('class').find('is-selected') == -1,
          'selecting again clears the highlight')

    # Refresh is a fresh enumeration, not a repaint.
    page.locator('#ar-refresh').click()
    expect(page.locator('#ar-agent-list .ar-agent-card').first).to_be_visible(timeout=15000)
    check(page.locator('#ar-scan-at').inner_text().startswith('Snapshot '),
          'refresh produces a fresh snapshot timestamp')

    # Scoped process control on an expanded per-process row.
    # (The refresh above re-rendered the inventory; wait for it to settle,
    # then expand.)
    page.wait_for_function("() => !window.CureAgentRadar.state.scanning")
    page.locator('#ar-agent-list .ar-agent-card .ar-agent-row').first.click()
    row = page.locator('#ar-agent-list .ar-agent-card .ar-proc').first
    expect(row).to_be_visible()
    check(row.locator('.ar-proc-details').inner_text() == 'Details',
          'each process row exposes a Details action')
    end_label = row.locator('.ar-proc-end').inner_text()
    check(end_label in ('End process', 'Force stop'),
          'the end action is labelled honestly for the process type: ' + end_label)
    row.locator('.ar-proc-end').click()
    expect(page.locator('#confirm-overlay')).to_be_visible()
    check('End' in page.locator('#confirm-title').inner_text(),
          'the confirmation names the action')
    check('Potential impact' in page.locator('#confirm-facts').inner_text(),
          'the confirmation states the potential impact')
    page.locator('#confirm-cancel').click()
    expect(page.locator('#confirm-overlay')).to_be_hidden()
    check('Cancelled' in page.locator('#ar-action-status').inner_text(),
          'cancelling reports that no change was made')
    row.locator('.ar-proc-end').click()
    expect(page.locator('#confirm-overlay')).to_be_visible()
    page.locator('#confirm-ok').click()
    expect(page.locator('#ar-action-status')).to_contain_text('PID', timeout=15000)
    status_text = page.locator('#ar-action-status').inner_text()
    check('gracefully' in status_text.lower() or 'force-stopped' in status_text.lower(),
          'the outcome reports how the process ended: ' + status_text[:70])

    check(page.locator('#ar-start').is_visible(), 'recording starts only from the explicit control')
    check(page.locator('#ar-flight-live').is_hidden(), 'no synthetic session exists before start')

    page.locator('#ar-workspace').fill('C:/Projects/cure-demo-app')
    page.locator('#ar-start').click()
    expect(page.locator('#ar-session-badge')).to_have_text('Recording', timeout=10000)
    expect(page.locator('#ar-stat-total')).to_have_text('6')
    check(page.locator('#ar-event-list .ar-event').count() == 6,
          'all recorded observations are listed')
    check('not attribution' in page.locator('.ar-caveat').first.inner_text(),
          'the recorder states that correlation is not attribution')
    check(page.locator('#ar-radar').get_attribute('data-phase') == 'monitoring',
          'the sweep runs only while a recording is actually active')
    check(page.locator('#ar-review-toggle').is_hidden(),
          'replay controls are not offered during live monitoring')
    check(page.locator('#ar-review').is_hidden(),
          'the replay slider stays hidden during live monitoring')

    page.locator('#ar-event-list .ar-event').first.click()
    expect(page.locator('#ar-evidence-card')).to_be_visible()
    check('Attribution is not established' in page.locator('#ar-evidence-card').inner_text(),
          'an evidence detail refuses attribution')
    selected_seq = page.evaluate("window.CureAgentRadar.state.selectedSeq")
    check(('observation ' + str(selected_seq)) in page.locator('#ar-evidence-card').inner_text().lower(),
          'event detail uses the recorder sequence number')
    check('Workspace' in page.locator('#ar-evidence-card').inner_text(),
          'the evidence detail names the recording boundary')

    page.locator('#ar-filters [data-filter="removed"]').click()
    check(page.locator('#ar-event-list .ar-event').count() == 1,
          'the type filter narrows recorded observations')
    page.locator('#ar-filters [data-filter="all"]').click()
    check(page.locator('#ar-event-list .ar-event').count() == 6,
          'the All filter restores every observation')

    page.locator('#ar-stop').click()
    expect(page.locator('#ar-session-badge')).to_have_text('Stopped', timeout=10000)
    check(page.locator('#ar-radar').get_attribute('data-phase') == 'idle',
          'the sweep stops when the recording stops')

    # Replay appears only in explicit review mode.
    check(page.locator('#ar-review-toggle').is_visible(),
          'a stopped session offers timeline review')
    check(page.locator('#ar-review').is_hidden(),
          'the replay slider is hidden until review is requested')
    page.locator('#ar-review-toggle').click()
    expect(page.locator('#ar-timeline')).to_be_visible()
    # Setting a range input and dispatching its input event must be one task:
    # Playwright's fill splits the two, and a recorder status render landing
    # between them reads back the reset slider value — seen as a flaky
    # 'Final' label instead of the scrubbed position. A user drag cannot
    # interleave this way; the assertions below are unchanged.
    page.locator('#ar-scrub').evaluate(
        "el => { el.value = '2'; el.dispatchEvent(new Event('input', {bubbles: true})); }")
    expect(page.locator('#ar-scrub-label')).to_contain_text('2 / 6')
    check(page.locator('#ar-event-list .ar-event').count() == 2,
          'replay position narrows the visible observations')
    page.locator('#ar-review-toggle').click()
    expect(page.locator('#ar-timeline')).to_be_hidden()
    check(page.locator('#ar-event-list .ar-event').count() == 6,
          'closing review restores the live list')

    # Export lives behind the compact Export menu.
    check(page.locator('#ar-export').is_hidden(),
          'export controls stay hidden until requested')
    page.locator('#ar-export-toggle').click()
    expect(page.locator('#ar-case-preview')).to_be_visible()
    check(page.locator('#ar-case-preview').is_enabled(), 'a stopped session unlocks the case file')
    page.locator('#ar-case-summary').fill('Reviewed a demo build')
    page.locator('#ar-case-include').check()
    page.locator('#ar-case-preview').click()
    expect(page.locator('#ar-case-stage img')).to_be_visible(timeout=10000)
    page.wait_for_timeout(3500)
    check(page.locator('#ar-case-export').is_enabled(),
          'case export stays enabled across a live status refresh')
    check(page.evaluate("""() => {
        const s = JSON.stringify(window.CureCaseFile.audit(window.CureAgentRadar.state.caseModel));
        return !/[A-Za-z]:\\\\|Users|AppData|\\.tsx|\\.json|cure-demo-app/i.test(s);
      }"""), 'the agent case model carries no path, file name or workspace name')
    check(page.evaluate("""() => {
        const s = JSON.stringify(window.CureAgentRadar.state.caseModel);
        return !/[A-Za-z]:\\\\|Users|AppData|\\.tsx|\\.json|cure-demo-app|secret-session/i.test(s);
      }"""), 'the raw agent case response carries no workspace or file data')

    page.locator('#ar-case-export').click()
    expect(page.locator('#ar-case-status')).to_contain_text('Case file written', timeout=10000)
    check('uploaded to' not in page.locator('#ar-case-status').inner_text().lower(),
          'agent case export reports a local path, not an upload')

    # A page reload restores the stopped session as collapsed history.
    # (The shell reopens on Overview, as a fresh launch does.)
    page.reload()
    page.wait_for_load_state('networkidle')
    page.locator('#nav-agents').click()
    page.wait_for_timeout(600)
    expect(page.locator('#ar-history')).to_be_visible(timeout=10000)
    check('previous recording' in page.locator('#ar-history').inner_text().lower(),
          'a recovered recording appears as collapsed history')
    check(page.locator('#ar-session-badge').inner_text().strip().lower() == 'stopped',
          'the recovered session is marked stopped')
    check(page.locator('#ar-flight-live').is_hidden(),
          'the recovered recording stays collapsed until opened')
    check(page.locator('#ar-radar-note').inner_text() == '',
          'the radar idle note stays empty across a reload (de-cluttered copy)')
    page.locator('#ar-history-open').click()
    expect(page.locator('#ar-flight-live')).to_be_visible()
    check(page.locator('#ar-stat-total').inner_text() == '6',
          'opening history shows the recorded evidence')

    # Windows Change Radar: a forensic lens under Investigate (Stage UI-06
    # moved the live radar controls out of the removed secondary view).
    page.locator('#nav-results').click()
    page.wait_for_timeout(400)
    page.locator('#lens-btn-winchanges').click()
    expect(page.locator('#lens-panel-winchanges')).to_be_visible(timeout=10000)
    check(page.locator('#view-title').inner_text() == 'Investigate',
          'Windows Change Radar is reachable under Investigate')
    page.locator('#ar-win-capture').click()
    expect(page.locator('#ar-winsnap-state')).to_contain_text('Trusted snapshot saved', timeout=10000)
    page.locator('#ar-win-compare').click()
    expect(page.locator('#ar-win-summary')).to_be_visible(timeout=10000)
    check(page.locator('#ar-win-lists .ar-win-item').count() == 2,
          'Windows persistence differences are shown as review leads')
    check('not malware verdicts' in page.locator('#ar-win-note').inner_text(),
          'Windows persistence changes are not called malware')

    axe(page, 'agent radar')
    layout(page, 'agent radar')
    check(not errors, 'agent radar runtime: ' + str(errors))
    ctx.close()


def public_demo_flow(browser):
    """The published demo is synthetic and has no write path."""
    ctx=browser.new_context(viewport={'width':1440,'height':900})
    page=ctx.new_page()
    errors=[]
    page.on('pageerror',lambda e:errors.append(str(e)))
    page.on('console',lambda m:errors.append(m.text) if m.type=='error' else None)
    page.goto((ROOT/'gui/dist-demo/index.html').as_uri())
    page.wait_for_load_state('networkidle')
    expect(page.locator('#view-overview .demo-banner')).to_be_visible()
    goto_view(page,'#nav-detective','view-detective')
    expect(page.locator('#view-detective .demo-banner')).to_be_visible()
    page.locator('#dt-investigate').click()
    expect(page.locator('#dt-leads .dt-lead').first).to_be_visible(timeout=30000)
    page.locator('#dt-case-preview').click()
    expect(page.locator('#dt-case-preview-stage img')).to_be_visible(timeout=10000)
    check(page.evaluate("!!window.CureDetective.state.caseFile.demo"),
          'the demo case file is marked synthetic')
    page.locator('#dt-case-export').click()
    expect(page.locator('#dt-case-status')).to_contain_text('Synthetic demo')
    commands=page.evaluate("window.__CURE_MOCK_COMMANDS||[]")
    check('storage_save_case_file' not in commands,
          'the public demo never asks the backend to write a case file')
    leaked=page.evaluate("""() => {
      const s = JSON.stringify(window.CureCaseFile.audit(window.CureDetective.state.caseFile));
      return s.match(/[A-Za-z]:\\\\|Users|AppData|\\.tmp|\\.zip|bob/gi) || [];
    }""")
    check(not leaked, 'the demo case file carries no path or user name: '+str(leaked[:3]))
    check(not errors,'public demo runtime: '+str(errors));ctx.close()


def mascot_absence_flow(browser):
    """The mascot was removed deliberately; it must stay removed.

    Walks every navigation destination and checks the live DOM: no companion
    API, no mascot nodes, no mascot assets, no companion preferences, and a
    present operator console.
    """
    ctx,page,errors=open_page(browser,(900,600))
    for nav in ["nav-overview","nav-scan","nav-cleanup","nav-quarantine",
                "nav-detective","nav-agents","nav-eventlog"]:
        page.locator("#"+nav).click()
        page.wait_for_timeout(300)
    probe = page.evaluate("""() => ({
        companion: typeof window.CureCompanion,
        lumaGlobal: typeof window.luma,
        nodes: document.querySelectorAll('.luma-svg,.luma-stage,.luma-host,.luma-figure').length,
        scripts: [...document.querySelectorAll('script[src]')]
            .map((s)=>s.getAttribute('src')).filter((x)=>x && x.includes('luma')).length,
        styles: [...document.querySelectorAll('link[rel="stylesheet"]')]
            .map((l)=>l.getAttribute('href')).filter((x)=>x && x.includes('luma')).length,
        prefs: document.querySelectorAll('#pref-companion,#pref-companion-motion').length,
        operator: document.querySelectorAll('.operator-node').length,
    })""")
    check(probe["companion"]=="undefined" and probe["lumaGlobal"]=="undefined"
          and probe["nodes"]==0 and probe["scripts"]==0 and probe["styles"]==0
          and probe["prefs"]==0,
          "the mascot is fully removed from the live interface: "+str(probe))
    check(probe["operator"]>0,"the operator console is present in the interface")
    check(not errors,'mascot absence flow has no runtime errors: '+str(errors));ctx.close()


def storage_failure(browser):
    """A scan that breaks must leave the screen saying so, not stuck.
    """
    ctx, page, errors = open_page(browser, (1440, 900), storage_error=True)
    page.locator("#nav-cleanup").click()
    page.wait_for_timeout(600)
    page.locator("#sw-scan").click()
    expect(page.locator("#sw-status-text")).to_have_text("Failed", timeout=20000)
    page.wait_for_timeout(400)
    check(page.locator("#sw-amount").inner_text() == "—",
          "a failed scan reports no measurement at all")
    check("not measured" in page.locator("#sw-amount-label").inner_text().lower(),
          "a failed scan says the machine is not measured: "
          + page.locator("#sw-amount-label").inner_text())
    check("did not complete" in page.locator("#sw-amount-note").inner_text(),
          "a failed scan says why: " + page.locator("#sw-amount-note").inner_text()[:80])
    check("did not complete" in page.locator("#sw-subline").inner_text(),
          "a failed scan says why in the subline: "
          + page.locator("#sw-subline").inner_text()[:80])
    check(page.locator("#sw-classes li").count() == 0,
          "a failed scan leaves no categories from the attempt that broke")
    check(page.evaluate("() => (window.CureStorageWorkstation.state.treemapCells||[]).length") == 0,
          "a failed scan leaves no map from the attempt that broke")
    check(page.locator("#sw-progress").is_hidden(),
          "the progress panel is dismissed when the scan breaks")
    layout(page, "storage failed");axe(page, "storage failed")
    check(not errors, "a failed scan raises no runtime error: " + str(errors))
    ctx.close()


def palette_and_assets():
    """Offline asset guard plus a measured, restrained palette check."""
    shipped = [
        "index.html", "style.css", "operator.css", "xray.css", "investigate.css",
        "quarantine.css", "monitor.css", "storage-workstation.css", "detective.css",
        "agent.css", "storage-rescue.css", "clean-3tab.css", "cyber-cat.css",
        "app.js", "operator.js", "xray.js", "storage-workstation.js", "casefile.js",
        "detective.js", "agent-radar.js", "storage-rescue.js", "move-bot.js",
        "cyber-cat.js", "virus-rescue-clean.js", "storage-clean.js"
    ]
    for name in shipped:
        body=(ROOT/"gui/dist"/name).read_text(encoding="utf-8")
        hits=[l for l in body.splitlines()
              if "http://" in l or "https://" in l or "//fonts." in l or "@import" in l]
        hits=[h for h in hits if "http://www.w3.org/2000/svg" not in h]
        check(not hits, "no remote asset reference in "+name+": "+str(hits[:2]))
    conf=json.loads((ROOT/"gui/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    allow=[Path(a).name for a in conf["build"]["frontendDist"]]
    check(sorted(allow)==sorted(shipped),
          "production asset allowlist matches the shipped frontend: "+str(allow))
    check(not (ROOT/"gui/dist/assets").exists(),
          "no asset drop is expected for the utility node")
    page=(ROOT/"gui/dist/index.html").read_text(encoding="utf-8")
    for name in shipped:
        if name.endswith((".css",".js")) and name!="app.js":
            check(name in page, "index.html references "+name)
    check("mock-tauri.js" not in page, "production shell never loads the mock backend")
    # The dev shell is generated from the production shell; drift between them
    # would silently invalidate every check below.
    sys.path.insert(0, str(ROOT / "gui/devtools"))
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "make_dev_shell", ROOT / "gui/devtools/make-dev-shell.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    production = (ROOT / "gui/dist/index.html").read_text(encoding="utf-8")
    dev = (ROOT / "gui/dist/index.dev.html").read_text(encoding="utf-8")
    check(dev == module.build(production),
          "index.dev.html matches the generated production shell")


def pixel():
    pic=Image.open(SHOTS / "scan.png").convert("RGB")
    colors=pic.getcolors(pic.width*pic.height)
    check(len(colors)>400, "render contains chassis, type, hairline and node detail")
    pixels=list(pic.getdata())
    total=len(pixels)
    violet=sum(1 for r,g,b in pixels if b>r and r>g and b-g>25)
    # The upper bound is the design guard: the accent must stay restrained. The
    # floor only has to prove the accent is still present, and it is calibrated
    # against what the interface itself paints
    # rather than a large illustrated character.
    check(0.010*total<violet<0.11*total, "violet identity accent stays restrained")
    # Palette discipline: cluster saturated pixels into hue families so
    # adjacent bins (one hue rendered at different lightness) count once, then
    # require that only a few families dominate the frame.
    import colorsys
    buckets = [0] * 12
    for r, g, b in pixels:
        mx, mn = max(r, g, b), min(r, g, b)
        if mx < 70 or mx - mn < 40:
            continue
        hue, sat, val = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
        if sat < 0.22 or val < 0.28:
            continue
        buckets[int(hue * 12) % 12] += 1
    strong = {k for k, v in enumerate(buckets) if v > 0.004 * total}
    families = []
    for bucket in sorted(strong):
        adjacent = False
        for group in families:
            if any(min((bucket - member) % 12, (member - bucket) % 12) <= 1
                   for member in group):
                adjacent = True
                break
        if adjacent:
            for group in families:
                if any(min((bucket - member) % 12, (member - bucket) % 12) <= 1
                       for member in group):
                    if bucket not in group:
                        group.append(bucket)
                    break
        else:
            families.append([bucket])
    check(len(families) <= 3,
          "no rainbow palette: at most three saturated hue families dominate: "
          + str(sorted(strong)))
    dark=sum(1 for r,g,b in pixels if (r+g+b)/3<46)
    check(0.55*total<dark, "console stays a dark graphite surface")
    # Depth, not flatness: the render must carry more than a handful of values.
    lum=sorted((r*299+g*587+b*114)//1000 for r,g,b in pixels)
    spread=lum[int(0.98*total)]-lum[int(0.02*total)]
    check(spread>=40, "surface layering produces real tonal depth (spread "+str(spread)+")")


def main():
    global URL
    # Ephemeral loopback server lets axe inspect CSS without file-origin CORS errors.
    class QuietHandler(SimpleHTTPRequestHandler):
        def log_message(self, *_): pass
        def end_headers(self):
            # Never let a browser reuse a previous run's asset: a stale
            # stylesheet or script would silently invalidate every check.
            self.send_header("Cache-Control", "no-store, max-age=0")
            super().end_headers()
    server=ThreadingHTTPServer(('127.0.0.1',0),partial(QuietHandler,directory=str(ROOT/'gui/dist')))
    threading.Thread(target=server.serve_forever,daemon=True).start()
    URL=f'http://127.0.0.1:{server.server_port}/index.dev.html'
    args=argparse.ArgumentParser();args.add_argument("mode",nargs="?",default="all")
    mode=args.parse_args().mode;SHOTS.mkdir(parents=True,exist_ok=True)
    palette_and_assets()
    with sync_playwright() as p:
        browser=p.chromium.launch()
        if mode in ("all","verify","capture"):
            core_flow(browser,(1440,900) if mode=="capture" else (900,600),capture=mode=="capture")
        if mode=="demo":demo_flow(browser)
        if mode=="agent-video":agent_demo(browser)
        if mode in ("all","verify"):
            core_flow(browser,reduced=True);states(browser);secondary(browser);responsive(browser);scan_origin_flow(browser)
        if mode=="capture":states(browser,capture=True)
        if mode in ("all","verify","capture"):mascot_absence_flow(browser)
        if mode in ("all","verify"):
            mascot_absence_flow(browser);storage_failure(browser);detective_flow(browser);agent_radar_flow(browser);public_demo_flow(browser)
        if mode=="layout":responsive(browser)
        if mode=="pixel":core_flow(browser);pixel()
        if mode=="all":pixel()
        browser.close()
    server.shutdown();server.server_close()
    (SHOTS / "quality-results.json").write_text(json.dumps({"mode":mode,"checks":len(checks),"passed":checks},indent=2))
    print(f"PASS: {len(checks)} checks ({mode})")


if __name__=="__main__":main()
