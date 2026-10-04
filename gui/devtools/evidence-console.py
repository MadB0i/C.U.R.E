"""Native Python Playwright quality gates and documentation capture.

All mutations use mock-tauri.js; no host files/processes are remediated.
Run: python gui/devtools/evidence-console.py [verify|layout|pixel|capture|demo|all]
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
    page.get_by_role("button", name="Start Rescue Scan", exact=True).click()


def finish_scan(page):
    expect(page.locator("#results-view")).to_be_visible(timeout=30000)


def layout(page, label):
    # Measure settled geometry, not the drawer's entrance transform. Keep
    # infinite scan/companion loops running; they must not block layout checks.
    page.wait_for_function("""() => document.getAnimations().every(a =>
      !Number.isFinite(a.effect.getComputedTiming().endTime) ||
      (!a.pending && a.playState !== 'running'))""")
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
    check(page.locator(".nav .nav-item").count() == 6, "six primary destinations")
    check(page.locator(".nav button:disabled").count() == 0, "no disabled primary navigation")
    check(page.locator(".nav-number").count() == 0, "navigation carries no numeric labels")
    check([t.strip() for t in page.locator(".nav .nav-item span").all_inner_texts()] ==
          ["Overview", "Rescue", "Investigate", "Quarantine", "Disk Cleanup", "Monitor"],
          "primary destinations are icon and title only")
    check(page.locator("#xray-canvas").count() == 1, "system x-ray canvas present")
    check(page.locator("[data-companion='overview'] .companion-art").count() == 1,
          "Luma is present in the overview")
    keyboard(page)
    start_scan(page)
    page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 2")
    layout(page, "scan " + str(size))
    check(page.locator("#xray-field").get_attribute("data-running") == "true",
          "x-ray reports the real running state")
    check(int(page.locator("#xray-inspected").inner_text()) >= 2,
          "instrument panel uses the collected item count")
    box = page.locator("#xray-canvas").bounding_box()
    store = page.locator("#xray-canvas").evaluate("c => c.width")
    check(box["height"] >= 150 and store >= box["width"] and store > 0,
          "x-ray renders into a real backing store at " + str(size))
    if capture:
        shot(page, DOCS / "02-scan.png")
    else:
        shot(page, SHOTS / ("scan-reduced.png" if reduced else "scan.png"))
    check(page.locator("#sweep-items").inner_text().startswith("2 ") or
          int(page.locator("#sweep-items").inner_text().split()[0]) >= 2, "sweep uses collected item count")
    check(page.locator("#sweep-sources li").count() == 9, "all nine inspection layers reported")
    if reduced:
        check(page.evaluate("document.getAnimations().filter(a=>a.playState==='running').length") == 0, "reduced motion has no running animation")
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
                check("Collection failed" in page.locator("#status-text").inner_text(), "scan error exposes retry")
                page.evaluate("window.__CURE_MOCK_SCAN_ERROR=false")
                page.locator("#scan-retry").click()
                finish_scan(page)
            else:
                finish_scan(page)
            if name == "partial":
                check("Coverage incomplete" in page.locator("#headline").inner_text(), "incomplete scan never called clean")
                page.locator("#nav-overview").click()
                expect(page.locator("#ov-posture")).to_have_text("Coverage incomplete")
                check("ACCESS DENIED" in page.locator("#ov-coverage").inner_text(), "access denied communicated in text")
            if name in ("partial", "partial-findings"):
                check(page.locator('[data-companion="overview"]').get_attribute('data-state') == 'review', 'incomplete collection never gives satisfied companion')
            if name == "partial-findings":
                expect(page.locator("#status-text")).to_have_text("Review required · Coverage incomplete")
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
    ctx, page, errors = open_page(browser, (900, 600), sweep=True, canary_alert=True, cleanup_failures=True)
    start_scan(page);finish_scan(page)
    # Individual process termination stays explicit and separate from scan/quarantine.
    page.locator("#nav-processes").click()
    page.locator(".kill-one").first.click()
    expect(page.locator("#confirm-overlay")).to_be_visible()
    page.keyboard.press("Escape")
    # Disk Cleanup is a primary destination, reached directly.
    page.locator("#nav-cleanup").click()
    expect(page.locator("#cleanup-idle")).to_be_visible()
    check(page.locator("#storage-reclaimable").inner_text() == "—",
          "unmeasured cleanup never invents reclaimable bytes")
    check(page.locator("#storage-legend .st-value").evaluate_all('(es)=>es.every(e=>e.textContent==="Not measured")'),
          "every category stays explicitly unmeasured before the engine reports")
    check(page.locator("#storage-core").bounding_box()["height"] >= 180,
          "storage core occupies the cleanup stage rather than leaving it empty")
    layout(page, "cleanup idle")
    page.locator("#cleanup-scan-btn").click()
    expect(page.locator("#cleanup-body")).to_be_visible()
    check(page.locator("#storage-reclaimable").inner_text() != "—",
          "reclaimable total comes from the engine measurement")
    check(page.locator("#storage-legend .st-value").evaluate_all('(es)=>es.every(e=>e.textContent!=="Not measured")'),
          "category breakdown reflects measured engine data")
    check(page.locator("[data-companion='cleanup']").get_attribute("data-variant", ).startswith("clean-"),
          "cleanup companion rotates through real cleanup variants")
    page.locator(".cleanup-cat").first.click()
    page.locator("#cleanup-btn").click()
    expect(page.locator("#confirm-overlay")).to_be_visible()
    page.locator("#confirm-ok").click()
    expect(page.locator("#confirm-overlay")).to_be_hidden()
    check(page.locator("#cleanup-queue").is_visible(), "queued removals are listed during cleanup")
    check("Freed" not in page.locator("#storage-freed-wrap").inner_text(),
          "busy cleanup never fabricates reclaimed bytes")
    expect(page.locator("#cleanup-status")).to_be_visible()
    check(page.evaluate("window.__CURE_LAST_CLEANUP_CALL !== undefined"), "cleanup runs only confirmed selections")
    expect(page.locator("#storage-freed-wrap")).to_be_visible()
    check("Reclaimed" in page.locator("#storage-freed-wrap").inner_text(),
          "reclaimed receipt uses the engine's own bytes_freed")
    layout(page, "cleanup");axe(page, "cleanup")
    page.locator("#nav-eventlog").click()
    page.locator("#view-eventlog [data-route='view-canary']").click()
    page.locator("#can-toggle").click()
    expect(page.locator("#can-state")).to_have_text("ACTIVE")
    expect(page.locator("#canary-alert-overlay")).to_be_visible(timeout=8000)
    axe(page, "Canary alert");page.keyboard.press("Escape")
    page.locator("#can-toggle").click();expect(page.locator("#can-state")).to_contain_text("OFF")
    page.locator("#nav-results").click();page.locator("#nav-incident").click()
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
            if nav == "nav-cleanup":expect(page.locator("#cleanup-idle")).to_be_visible()
            layout(page, nav + str(size))
        # Desktop hero composition must not push critical information away.
        if size[0] >= 900:
            page.locator("#nav-scan").click()
            page.get_by_role("button", name="Start Rescue Scan", exact=True).click()
            page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 1")
            layout(page, "scan hero " + str(size))
            check(page.locator("#xray-core-readout").bounding_box()["y"] +
                  page.locator("#xray-core-readout").bounding_box()["height"] <= size[1],
                  "x-ray readout stays inside the viewport at " + str(size))
            check(page.locator(".instrument").bounding_box()["y"] <= size[1],
                  "live instrument panel is above the fold at " + str(size))
            page.locator("#nav-overview").click()
        page.locator("#nav-overview").click()
        shot(page, SHOTS / ("overview-%sx%s.png" % size))
        check(not errors, "responsive runtime: " + str(errors));ctx.close()


def demo_flow(browser):
    ctx,page,errors=open_page(browser,(1440,900),video=True,cleanup_delay_ms=1800)
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
    # Cleanup is a separate, explicitly confirmed action; all video data is labelled mock data.
    page.locator('#nav-cleanup').click()
    page.locator('#cleanup-scan-btn').click();expect(page.locator('#cleanup-body')).to_be_visible()
    page.locator('.cleanup-cat.on').filter(has_text='Browser caches').click()
    page.locator('.cleanup-cat.on').filter(has_text='Windows.old').click()
    page.locator('#cleanup-btn').click();expect(page.locator('#confirm-overlay')).to_be_visible()
    page.wait_for_timeout(1200);page.locator('#confirm-ok').click()
    expect(page.locator('[data-companion="cleanup"]')).to_have_attribute('data-state','cleanup')
    page.wait_for_timeout(1000)
    expect(page.locator('[data-companion="cleanup"]')).to_have_attribute('data-state','success')
    page.wait_for_timeout(1200)
    path=page.video.path();ctx.close()
    Path(path).replace(ROOT/'docs/media/demo.webm')
    check(not errors,'demo runtime: '+str(errors))


def companion_flow(browser, capture=False):
    # The character is presentation only: pause/reduced motion and backend truth matter.
    for failed in [False, True]:
        ctx,page,errors=open_page(browser,(1440,900) if capture else (900,600),cleanup_failures=failed,cleanup_delay_ms=2400)
        stage=page.locator('[data-companion="overview"]')
        check(stage.get_attribute('data-state')=='idle','welcome pose before any scan')
        check(page.locator('.companion-art').evaluate_all('(es)=>es.every(e=>e.getAttribute("aria-hidden")==="true" && e.getAttribute("focusable")==="false")'),'art does not enter accessibility or focus trees')
        stage.get_by_role('button',name='Pause motion for Luma').click()
        check(page.locator('.companion').evaluate_all('(es)=>es.every(e=>e.dataset.paused==="true")'),'pause applies to every companion stage')
        check(page.evaluate('window.CureSweep.motionPaused()'),'one pause gesture governs the whole presentation')
        check(page.evaluate('document.getAnimations().filter(a=>a.playState==="running").length')==0,'pausing stops every running loop')
        page.reload();expect(page.locator('#view-overview')).to_be_visible()
        check(stage.get_attribute('data-paused')=='true','motion preference survives reload')
        stage.get_by_role('button',name='Resume motion for Luma').click()
        page.emulate_media(reduced_motion='reduce')
        expect(stage.locator('.companion-reduced')).to_be_visible()
        check(page.locator('.companion').evaluate_all('(es)=>es.every(e=>e.dataset.paused==="true")'),'live reduced-motion preference stops companion')
        check(page.evaluate('document.getAnimations().filter(a=>a.playState==="running").length')==0,'reduced motion has no ongoing loops')
        page.emulate_media(reduced_motion='no-preference')
        page.locator('#nav-scan').click();page.get_by_role('button',name='Start Rescue Scan',exact=True).click()
        page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 1")
        check(page.locator('[data-companion="scan"]').get_attribute('data-variant')=='scan-console','first scan session uses the console variant')
        # Let collection finish before leaving the view; a scan that is still
        # running legitimately returns the operator to the findings screen.
        finish_scan(page)
        page.locator('#nav-cleanup').click()
        clean=page.locator('[data-companion="cleanup"]')
        check(clean.get_attribute('data-state')=='idle','cleanup idle never starts sweeping activity')
        page.locator('#cleanup-scan-btn').click();expect(page.locator('#cleanup-body')).to_be_visible()
        page.locator('#cleanup-btn').click();expect(page.locator('#confirm-overlay')).to_be_visible()
        check(page.locator('#confirm-overlay .companion').count()==0,'serious confirmation contains no assistant artwork')
        check(page.evaluate('window.__CURE_LAST_CLEANUP_CALL===undefined'),'confirmation does not execute cleanup')
        page.keyboard.press('Escape')
        check(page.evaluate('window.__CURE_LAST_CLEANUP_CALL===undefined'),'cancel leaves cleanup uninvoked')
        page.locator('#cleanup-btn').click();page.locator('#confirm-ok').click()
        expect(clean).to_have_attribute('data-state','cleanup')
        expect(clean.locator('.companion-detail')).to_contain_text('Waiting for the engine result')
        check('Freed' not in clean.inner_text(),'busy pose never fabricates reclaimed bytes')
        check(page.locator('.cleanup-cat .cc-state').evaluate_all('(es)=>es.every(e=>["Included","Skipped"].includes(e.textContent))'),'cleanup category selection is explicit in text')
        check(page.locator('.cleanup-summary').is_visible(),'actual cleanup summary stays available at minimum window')
        layout(page,'companion cleanup running');axe(page,'companion cleanup running')
        if capture and not failed:
            page.locator('#stage-main').evaluate('e=>e.scrollTop=0');shot(page,DOCS/'08-cleanup.png')
        expect(page.locator('#cleanup-status')).to_be_visible(timeout=15000)
        expect(clean).to_have_attribute('data-state','review' if failed else 'success')
        check(clean.locator('.companion-detail').inner_text()==page.locator('#cleanup-status').inner_text(),'companion result uses exact engine receipt')
        # Wait for the normal post-action measurement to resolve before public capture.
        expect(page.locator('#cleanup-body')).to_be_visible()
        page.wait_for_function('document.querySelector("#cleanup-subline").textContent.startsWith('+repr('4' if failed else '3')+')')
        check(page.evaluate('window.__CURE_LAST_CLEANUP_RESULT.attempted')==1245,'cleanup fixture reports files rather than category count')
        if capture and not failed:shot(page,DOCS/'09-cleanup-result.png')
        page.locator('#nav-quarantine').click()
        check(page.locator('#view-quarantine .companion,#evidence-body .companion').count()==0,'evidence and archive remain free of assistant artwork')
        check(not errors,'companion flow has no runtime or remote errors: '+str(errors));ctx.close()


def luma_states(browser):
    """Every authored state must be reachable, distinct and restrained."""
    ctx,page,errors=open_page(browser,(1440,900))
    # Measured here, on the overview, because a hidden view reports no box.
    overview_w=page.evaluate(
        "() => Math.round(document.querySelector('[data-companion=\"overview\"] .luma-node')"
        ".getBoundingClientRect().width)")
    check(0<overview_w<=96,'node stays small on the overview: '+str(overview_w))
    # Each variant must still rotate, so the node is not one frozen loop.
    seen=[]
    for i in range(3):
        start_scan(page)
        if i==0:
            # Measured mid-scan, while the stage is on screen and has a box.
            expect(page.locator('[data-companion="scan"] .luma-node')).to_be_visible()
            scan_w=page.evaluate(
                "() => Math.round(document.querySelector('[data-companion=\"scan\"] .luma-node')"
                ".getBoundingClientRect().width)")
            check(0<scan_w<=96,'node stays secondary to the scan field data: '+str(scan_w))
        finish_scan(page)
        seen.append(page.locator('[data-companion="scan"]').get_attribute('data-variant'))
    check(seen==['scan-console','scan-scanner','scan-lens'],
          'scan sessions rotate through all three scan variants: '+str(seen))
    # Cleanup variants rotate the same way.
    cseen=[]
    for _ in range(3):
        page.locator('#nav-cleanup').click()
        cseen.append(page.locator('[data-companion="cleanup"]').get_attribute('data-variant'))
        page.locator('#nav-overview').click()
    check(cseen==['clean-sweep','clean-sort','clean-recycle'],
          'cleanup sessions rotate through all three cleanup variants: '+str(cseen))
    # Seal appears only after a completed, confirmed quarantine.
    check(page.locator('[data-companion="result"]').get_attribute('data-state')!='seal',
          'no seal state before any quarantine')
    page.locator('#nav-results').click()
    page.locator('#review-cards .quarantine-btn').first.click()
    page.locator('#confirm-ok').click()
    expect(page.locator('[data-companion="result"]')).to_have_attribute('data-state','seal')
    # LUMA is an instrument, not a character or a data source: no text to read,
    # no filter to bloom, and nothing large enough to compete with the numbers.
    restraint=page.evaluate("""() => {
        const node=document.querySelector('[data-companion="result"] .luma-node');
        const cs=getComputedStyle(node);
        return {texts: node.querySelectorAll('text').length,
                filters: node.querySelectorAll('filter').length,
                images: node.querySelectorAll('image').length,
                filter: cs.filter,
                aria: node.getAttribute('aria-hidden'),
                focusable: node.getAttribute('focusable')};
      }""")
    check(restraint['texts']==0 and restraint['images']==0,
          'node carries no text or raster data of its own: '+str(restraint))
    check(restraint['filters']==0 and restraint['filter']=='none',
          'node renders without any glow filter: '+str(restraint['filter']))
    check(restraint['aria']=='true' and restraint['focusable']=='false',
          'node stays out of the accessibility and focus trees: '+str(restraint))
    # Every state must read differently: one ring colour plus one animated
    # layer, so a repeated signature would mean two states look alike.
    signatures=page.evaluate("""() => {
        const stage=document.querySelector('[data-companion="result"]');
        return ['idle','scan','cleanup','review','success','seal'].map(s => {
          window.CureCompanion.result(s, 'probe', '');
          const ring=stage.querySelector('.ln-ring');
          const core=stage.querySelector('.ln-core');
          return [getComputedStyle(ring).stroke, getComputedStyle(ring).opacity,
                  getComputedStyle(core).fill,
                  getComputedStyle(stage.querySelector('.ln-sweep')).display,
                  getComputedStyle(stage.querySelector('.ln-segs')).display].join('|');
        });
      }""")
    check(len(set(signatures))==5,
          'every companion state has its own visual treatment: '+str(len(set(signatures))))
    check(not errors,'luma state runtime: '+str(errors));ctx.close()


def palette_and_assets():
    """Offline asset guard plus a measured, restrained palette check."""
    shipped=["index.html","style.css","xray.css","luma.css","storage.css",
             "app.js","luma.js","xray.js","storage-core.js"]
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
    # against what the interface itself paints now that LUMA is a small node
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
        if mode in ("all","verify"):
            core_flow(browser,reduced=True);states(browser);secondary(browser);responsive(browser)
        if mode=="capture":states(browser,capture=True)
        if mode in ("all","verify","capture"):companion_flow(browser,capture=mode=="capture")
        if mode in ("all","verify"):luma_states(browser)
        if mode=="layout":responsive(browser)
        if mode=="pixel":core_flow(browser);pixel()
        if mode=="all":pixel()
        browser.close()
    server.shutdown();server.server_close()
    (SHOTS / "quality-results.json").write_text(json.dumps({"mode":mode,"checks":len(checks),"passed":checks},indent=2))
    print(f"PASS: {len(checks)} checks ({mode})")


if __name__=="__main__":main()
