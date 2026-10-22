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
    page.get_by_role("button", name="Start Rescue Scan", exact=True).click()


def finish_scan(page):
    expect(page.locator("#results-view")).to_be_visible(timeout=30000)


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
    # Infinite loops (the LUMA node, the scan field, the storage core) must not
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
    check(page.locator(".nav .nav-item").count() == 7, "seven primary destinations")
    check(page.locator(".nav button:disabled").count() == 0, "no disabled primary navigation")
    check(page.locator(".nav-number").count() == 0, "navigation carries no numeric labels")
    check([t.strip() for t in page.locator(".nav .nav-item span").all_inner_texts()] ==
          ["Overview", "Rescue", "Investigate", "Quarantine", "Detective", "Storage", "Monitor"],
          "primary destinations are icon and title only")
    check(page.locator("#xray-canvas").count() == 1, "system x-ray canvas present")
    check(page.locator("[data-companion='overview'] .luma-svg").count() == 1,
          "LUMA is present in the overview")
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
                # The invariant is that an incomplete collection never reads
                # as satisfied. Which concerned pose she takes depends on
                # whether real findings exist alongside the missing coverage.
                companion = page.locator('[data-companion="overview"]').get_attribute('data-state')
                check(companion in ("review", "discovering"),
                      "incomplete collection never gives satisfied companion: " + str(companion))
            if name == "partial":
                check(page.locator('[data-companion="overview"]').get_attribute('data-state') == 'review',
                      "an empty but incomplete collection puts LUMA into review, not discovery")
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
    # The companion must not repeat the state it is already showing.
    companion_copy = page.locator('[data-companion="storage"]')
    title = companion_copy.locator(".luma-title").inner_text().strip().lower()
    detail = companion_copy.locator(".luma-detail").inner_text().strip().lower()
    check(title != detail,
          "the companion does not repeat the same label twice: "
          + title + " / " + detail)
    check("scan" not in detail,
          "the companion does not print the bare word 'scan': " + detail)
    # The topbar names the kind of scan that is running.
    check("STORAGE" in page.locator("#session-meta").inner_text(),
          "the topbar says a storage measurement is in flight: "
          + page.locator("#session-meta").inner_text())
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
            page.get_by_role("button", name="Start Rescue Scan", exact=True).click()
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
    # The engine call is real from the UI's point of view; wait for the pose
    # rather than assuming it is already there.
    expect(page.locator('[data-companion="storage"]')).to_have_attribute('data-state','cleanup')
    page.wait_for_timeout(1000)
    expect(page.locator('[data-companion="storage"]')).to_have_attribute('data-state','success')
    page.wait_for_timeout(1600)
    expect(page.locator('#sw-receipt-overlay')).to_be_visible()
    page.wait_for_timeout(1200)
    path=page.video.path();ctx.close()
    Path(path).replace(ROOT/'docs/media/demo.webm')
    check(not errors,'demo runtime: '+str(errors))


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
    expect(page.locator('[data-companion="detective"]')).to_have_attribute('data-state','major')

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


def companion_flow(browser, capture=False):
    # The character is presentation only: pause/reduced motion and backend truth matter.
    for failed in [False, True]:
        ctx,page,errors=open_page(browser,(1440,900) if capture else (900,600),storage_locked=failed)
        stage=page.locator('[data-companion="overview"]')
        check(stage.get_attribute('data-state')=='idle','welcome pose before any scan')
        check(page.locator('.luma-svg').evaluate_all('(es)=>es.every(e=>e.getAttribute("aria-hidden")==="true" && e.getAttribute("focusable")==="false")'),'art does not enter accessibility or focus trees')
        # The character area carries one control and one label.
        companion_buttons=stage.locator('.luma-controls button')
        check(companion_buttons.count()==1,'the companion has exactly one control')
        check(companion_buttons.first.inner_text().strip().lower()=='hide','the only companion control is Hide')
        check(stage.locator('.luma-kind').is_hidden(),'the duplicated state eyebrow is gone')
        check(stage.locator('.luma-title').is_visible(),'one readable state label remains')
        # Hide removes her from the visible area and the hero closes the gap
        # she left; the preference brings her back.
        companion_buttons.first.click()
        check(page.evaluate("""() => {
            const h = document.querySelector('[data-companion="overview"]');
            const hero = document.querySelector('.ov-hero');
            return h && h.hidden === true &&
                   hero.getAttribute('data-companion-hidden') === 'true' &&
                   getComputedStyle(hero).gridTemplateColumns.split(' ').length === 2;
        }"""),'hiding the companion closes the hero column')
        check(page.evaluate("""() => {
            const stages = [...document.querySelectorAll('.luma-stage')];
            return document.getAnimations().filter(a=>a.playState==='running')
                .filter(a=>a.effect&&a.effect.target&&stages.some(s=>s.contains(a.effect.target))).length;
        }""")==0,'a hidden companion stops animating')
        page.locator('#nav-eventlog').click()
        # Hiding her cleared the preference box, so bringing her back is a
        # tick rather than a clear.
        page.locator('#pref-companion').check()
        check(page.evaluate("""() => {
            const h = document.querySelector('[data-companion="overview"]');
            return h && h.hidden === false;
        }"""),'the preference brings the companion back')
        page.locator('#nav-overview').click()
        # The motion preference lives in the display preferences, and still
        # governs every companion.
        page.locator('#nav-eventlog').click()
        expect(page.locator('#view-eventlog')).to_be_visible()
        page.locator('#pref-companion-motion').check()
        check(page.locator('.luma-stage').evaluate_all('(es)=>es.every(e=>e.dataset.paused==="true")'),'pause applies to every companion stage')
        check(page.evaluate('window.CureSweep.motionPaused()'),'one pause gesture governs the whole presentation')
        check(companion_running(page)==0,'pausing stops every running companion loop')
        page.reload();expect(page.locator('#view-overview')).to_be_visible()
        page.locator('#nav-eventlog').click()
        check(stage.locator('.luma-stage').get_attribute('data-paused')=='true','motion preference survives reload')
        page.locator('#pref-companion-motion').uncheck()
        page.emulate_media(reduced_motion='reduce')
        # The note lives in the Overview, so look at it where it is visible.
        page.locator('#nav-overview').click()
        expect(stage.locator('.luma-reduced')).to_be_visible()
        check(page.locator('.luma-stage').evaluate_all('(es)=>es.every(e=>e.dataset.paused==="true")'),'live reduced-motion preference stops companion')
        check(companion_running(page)==0,'reduced motion has no ongoing companion loops')
        page.emulate_media(reduced_motion='no-preference')
        page.locator('#nav-scan').click();page.get_by_role('button',name='Start Rescue Scan',exact=True).click()
        page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 1")
        check(page.locator('[data-companion="scan"]').get_attribute('data-state')=='scan',
              'LUMA reflects the real running collection')
        # Let collection finish before leaving the view; a scan that is still
        # running legitimately returns the operator to the findings screen.
        finish_scan(page)
        page.locator('#nav-cleanup').click()
        clean=page.locator('[data-companion="storage"]')
        # Opening the workstation performs no work, so LUMA must not be in a
        # working pose. Which calm pose she holds depends on the last real
        # result; what matters is that nothing is pretending to run.
        opening_state=clean.get_attribute('data-state')
        check(opening_state not in ('scan','cleanup','quarantine','discovering'),
              'storage idle never starts sweeping activity: '+str(opening_state))
        check(opening_state in ('idle','review','success','failure','cancelled'),
              'storage idle holds a settled pose: '+str(opening_state))
        page.locator('#sw-scan').click()
        expect(page.locator('#sw-progress')).to_be_visible(timeout=15000)
        check(clean.get_attribute('data-state')=='scan','LUMA reflects a storage measurement in progress')
        expect(page.locator('#sw-progress')).to_be_hidden(timeout=20000)
        check(clean.get_attribute('data-state')=='discovering',
              'LUMA reacts to the real measured count once results land: '
              + str(clean.get_attribute('data-state')))
        page.locator('#sw-tab-large').click()
        page.locator('#sw-large-body .sw-queue-btn').first.click()
        check(clean.get_attribute('data-state')=='review',
              'queuing an item puts LUMA into a calm review pose')
        page.locator('#sw-tab-queue').click()
        expect(page.locator('#sw-queue-groups')).to_be_visible()
        expect(page.locator('#sw-queue-badge')).to_have_text('1')
        page.locator('#sw-preview-btn').click();expect(page.locator('#sw-preview-overlay')).to_be_visible()
        check(page.locator('#sw-preview-overlay .luma-stage').count()==0,
              'serious confirmation contains no assistant artwork')
        check(page.evaluate('window.__CURE_LAST_STORAGE_CALL===undefined'),'confirmation does not execute removal')
        page.keyboard.press('Escape')
        check(page.evaluate('window.__CURE_LAST_STORAGE_CALL===undefined'),'cancel leaves removal uninvoked')
        page.locator('#sw-preview-btn').click();page.locator('#sw-confirm-input').fill('REMOVE')
        page.locator('#sw-preview-confirm').click()
        expect(clean).to_have_attribute('data-state','cleanup')
        expect(page.locator('#sw-receipt-overlay')).to_be_visible(timeout=15000)
        expect(clean).to_have_attribute('data-state','success')
        check(clean.locator('.luma-detail').inner_text()==
              page.locator('#sw-receipt-label').inner_text(),
              'companion result uses the exact engine receipt')
        # Close the receipt before capturing anything else: it is modal, so
        # leaving it open would put the next screenshots behind a dialog.
        page.locator('#sw-receipt-close').click()
        expect(page.locator('#sw-receipt-overlay')).to_be_hidden()
        if capture:
            # Public storage screenshots. All figures are labelled mock data.
            page.locator('#nav-cleanup').click()
            page.locator('#sw-scan').click()
            expect(page.locator('#sw-progress')).to_be_hidden(timeout=20000)
            page.wait_for_timeout(700)
            # The map is the point of this capture: make sure the tab strip is
            # on it, whatever the earlier flow left selected.
            page.locator('#sw-tab-map').click()
            page.wait_for_timeout(500)
            page.locator('#stage-main').evaluate('e=>e.scrollTop=0')
            shot(page, DOCS/'08-storage-map.png')
            page.locator('#sw-tab-large').click()
            page.locator('#sw-large-body .sw-queue-btn').first.click()
            page.locator('#sw-tab-queue').click()
            page.wait_for_timeout(400)
            page.locator('#sw-preview-btn').click()
            expect(page.locator('#sw-preview-overlay')).to_be_visible()
            page.wait_for_timeout(500)
            shot(page, DOCS/'09-storage-preview.png')
            page.keyboard.press('Escape')
            expect(page.locator('#sw-preview-overlay')).to_be_hidden()
        page.locator('#nav-quarantine').click()
        check(page.locator('#view-quarantine .luma-stage,#evidence-body .luma-stage').count()==0,
              'evidence and archive remain free of assistant artwork')
        check(not errors,'companion flow has no runtime or remote errors: '+str(errors));ctx.close()


def storage_failure(browser):
    """A scan that breaks must leave the screen saying so, not stuck.

    The failure path once threw a missing-method error inside the
    companion's own adapter, which swallowed the engine's message and
    left the headline claiming the machine was still being measured.
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
    check(page.locator('[data-companion="storage"]').get_attribute("data-state") == "failure",
          "the companion takes the failure pose, not the scanning one")
    check(page.locator("#sw-progress").is_hidden(),
          "the progress panel is dismissed when the scan breaks")
    layout(page, "storage failed");axe(page, "storage failed")
    check(not errors, "a failed scan raises no runtime error: " + str(errors))
    ctx.close()


def luma_states(browser):
    """Every authored state must be reachable, distinct and restrained."""
    ctx,page,errors=open_page(browser,(1440,900))
    overview=page.locator('[data-companion="overview"]')
    overview_w=page.evaluate(
        "() => Math.round(document.querySelector('[data-companion=\"overview\"] .luma-stage')"
        ".getBoundingClientRect().width)")
    check(0<overview_w<=160,'LUMA stays secondary on the overview: '+str(overview_w))

    # The rig is actually built: head, ears, both eyes, tail joints and paws.
    rig=page.evaluate("""() => {
        const svg=document.querySelector('[data-companion="overview"] .luma-svg');
        return {head:svg.querySelectorAll('.lm-head').length,
                ears:svg.querySelectorAll('.lm-ear').length,
                eyes:svg.querySelectorAll('.lm-eye').length,
                irises:svg.querySelectorAll('.lm-iris').length,
                glints:svg.querySelectorAll('.lm-glint').length,
                lids:svg.querySelectorAll('.lm-lid').length,
                tailJoints:svg.querySelectorAll('.lm-tail-base,.lm-tail-1,.lm-tail-2,.lm-tail-tip').length,
                paws:svg.querySelectorAll('.lm-paw').length,
                brows:svg.querySelectorAll('.lm-brow').length,
                whiskers:svg.querySelectorAll('.lm-whisker').length};
      }""")
    check(all(v>=1 for v in rig.values()),
          'the character is a rigged figure, not a single shape: '+str(rig))
    check(rig['eyes']==2 and rig['lids']==2,
          'both eyes exist and both can blink: '+str(rig))
    check(rig['tailJoints']==4,'the tail is articulated across four joints')
    check(rig['paws']==2,'both front paws are rigged')

    # The character must be a ginger tabby, with the cream belly, the white
    # socks and the tabby markings the art direction calls for.
    coat=page.evaluate("""() => {
        const host=document.querySelector('[data-companion="overview"]');
        const cs=getComputedStyle(host);
        const svg=host.querySelector('.luma-svg');
        return {
          fur: cs.getPropertyValue('--luma-fur').trim(),
          cream: cs.getPropertyValue('--luma-cream').trim(),
          sock: cs.getPropertyValue('--luma-sock').trim(),
          stripe: cs.getPropertyValue('--luma-stripe').trim(),
          stripes: svg.querySelectorAll('.lm-stripe').length,
          rings: svg.querySelectorAll('.lm-tail-ring').length,
          socks: svg.querySelectorAll('.lm-sock').length,
          asleep: svg.querySelectorAll('.lm-asleep').length,
          awake: svg.querySelectorAll('.lm-awake').length,
        }; }""")
    check(coat['stripes']>=10,'the tabby markings are drawn: '+str(coat['stripes']))
    check(coat['rings']>=3,'the tail is ringed: '+str(coat['rings']))
    check(coat['socks']>=2,'the paws have white socks: '+str(coat['socks']))
    check(coat['asleep']==1 and coat['awake']==1,
          'there is a real curled sleeping pose, not a sitting cat with closed eyes')

    # The rim light must actually paint something. The earlier version
    # traced a path out and back along the same curve, which has zero area.
    rim=page.evaluate("""() => {
        const svg=document.querySelector('[data-companion="overview"] .luma-svg');
        const rim=svg.querySelector('.lm-body-rim');
        if(!rim) return {present:false};
        const b=rim.getBBox();
        return {present:true, w:Math.round(b.width), h:Math.round(b.height),
                filled: rim.getAttribute('fill') !== 'none'}; }""")
    check(rim['present'] and rim['w']>2 and rim['h']>2,
          'the rim light covers a real area: '+str(rim))

    # The lid must be a curved eyelid, not a rectangle wider than the eye.
    lid=page.evaluate("""() => {
        const svg=document.querySelector('[data-companion="overview"] .luma-svg');
        const eye=svg.querySelector('.lm-eye');
        const lid=svg.querySelector('.lm-lid-fill');
        const eb=eye.getBoundingClientRect(), lb=lid.getBoundingClientRect();
        return {tag: lid.tagName, eyeW: Math.round(eb.width),
                lidW: Math.round(lb.width)}; }""")
    check(lid['tag']!='rect' and lid['lidW']<=lid['eyeW']+2,
          'the eyelid is shaped like an eye, not a visor: '+str(lid))

    # Petting: only the cat you touched reacts, and the artwork is the hit
    # region rather than an empty square.
    pet=page.evaluate("""() => {
        const stages=[...document.querySelectorAll('[data-companion] .luma-stage')];
        const before=stages.map(s=>s.querySelector('.luma-svg').classList.contains('lm-pulse'));
        const svg=stages[0].querySelector('.luma-svg');
        const shape=svg.querySelector('.lm-body-shape');
        const r=shape.getBoundingClientRect();
        shape.dispatchEvent(new MouseEvent('click', {
          bubbles:true, clientX:r.left+r.width/2, clientY:r.top+r.height/2}));
        const after=stages.map(s=>s.querySelector('.luma-svg').classList.contains('lm-pulse'));
        // A click on the empty corner of the canvas is not a pet.
        stages.forEach(s=>s.querySelector('.luma-svg').classList.remove('lm-pulse'));
        const empty=svg.getBoundingClientRect();
        svg.dispatchEvent(new MouseEvent('click', {
          bubbles:true, clientX:empty.left+2, clientY:empty.top+2}));
        const corner=stages.map(s=>s.querySelector('.luma-svg').classList.contains('lm-pulse'));
        return {before, after, corner}; }""")
    check(sum(pet['after'])==1,
          'petting one cat purrs one cat: '+str(pet['after']))
    check(sum(pet['corner'])==0,
          'clicking empty space is not a pet: '+str(pet['corner']))

    # Keyboard: the companion is a real control.
    kbd=page.evaluate("""() => {
        const stage=document.querySelector('[data-companion="overview"] .luma-stage');
        return {role: stage.getAttribute('role'),
                tabindex: stage.getAttribute('tabindex'),
                label: stage.getAttribute('aria-label')}; }""")
    check(kbd['role']=='button' and kbd['tabindex']=='0' and bool(kbd['label']),
          'the companion can be reached and operated from the keyboard: '+str(kbd))

    # The storage companion keeps its own persona: a global event must not
    # overwrite it.
    scoped=page.evaluate("""() => {
        const storage=document.querySelector('[data-companion="storage"]');
        const before=storage.getAttribute('data-state');
        window.CureCompanion.seal('global probe');
        const after=storage.getAttribute('data-state');
        return {before, after}; }""")
    check(scoped['before']==scoped['after'],
          'a global event does not overwrite a screen-specific companion: '+str(scoped))

    # Each state must be visually distinct: prop geometry plus brow angle.
    probe=page.evaluate("""() => {
        const stage=document.querySelector('[data-companion="overview"]');
        const states=['idle','scan','discovering','review','cleanup','quarantine',
                      'success','failure','cancelled','sleeping'];
        return states.map(s => {
          window.CureCompanion.idle(null);
          const fn={idle:()=>window.CureCompanion.idle('probe'),
                    discovering:()=>window.CureCompanion.discovering(3,'probe'),
                    review:()=>window.CureCompanion.review(2,'probe'),
                    quarantine:()=>window.CureCompanion.seal('probe'),
                    failure:()=>window.CureCompanion.failed('probe'),
                    cancelled:()=>window.CureCompanion.cancelled('probe'),
                    sleeping:()=>window.CureCompanion.sleep(),
                    cleanup:()=>window.CureCompanion.cleanup('cleanup','probe','probe'),
                    success:()=>window.CureCompanion.cleanup('success','probe','probe'),
                    scan:()=>window.CureCompanion.scan('scan','probe')}[s];
          fn();
          const svg=stage.querySelector('.luma-svg');
          const prop=svg.querySelector('.lm-prop');
          const brow=svg.querySelector('.lm-brow-l');
          return {state:svg.getAttribute('data-state'),
                  props:prop.children.length,
                  shape:prop.firstElementChild?prop.firstElementChild.tagName:'none',
                  brow:getComputedStyle(brow).transform};
        });
      }""")
    check(len(set(p["state"] for p in probe))==10,
          'all ten authored states are reachable: '+str([p["state"] for p in probe]))
    signatures=[p["state"]+'|'+str(p["props"])+'|'+p["shape"]+'|'+p["brow"] for p in probe]
    check(len(set(signatures))==10,
          'every companion state has its own visual treatment: '+str(len(set(signatures))))
    props={p["state"]:p["props"] for p in probe}
    check(props['idle']==0,'idle carries no prop')
    check(all(props[s]>0 for s in
              ['scan','discovering','review','cleanup','quarantine','success',
               'failure','cancelled','sleeping']),
          'every active state has its own prop: '+str(props))

    # LUMA must never invent a reaction to a finding that does not exist.
    page.evaluate("window.CureCompanion.discovering(0,'nothing found')")
    expect(overview).to_have_attribute('data-state','idle')
    check('nothing found' in overview.locator('.luma-detail').inner_text(),
          'a zero finding count returns LUMA to idle with the real detail')

    # Restraint: vector only, no text, no raster, no glow filter.
    restraint=page.evaluate("""() => {
        const svg=document.querySelector('[data-companion="overview"] .luma-svg');
        const cs=getComputedStyle(svg);
        return {texts: svg.querySelectorAll('text').length,
                filters: svg.querySelectorAll('filter').length,
                images: svg.querySelectorAll('image').length,
                filter: cs.filter,
                aria: svg.getAttribute('aria-hidden'),
                focusable: svg.getAttribute('focusable'),
                tspan: svg.querySelectorAll('tspan').length};
      }""")
    check(restraint['texts']==0 and restraint['images']==0 and restraint['tspan']==0,
          'the figure carries no text or raster data of its own: '+str(restraint))
    check(restraint['filters']==0 and restraint['filter']=='none',
          'the figure renders without any glow filter: '+str(restraint['filter']))
    check(restraint['aria']=='true' and restraint['focusable']=='false',
          'the figure stays out of the accessibility and focus trees: '+str(restraint))

    # She is interactive: clicking her produces a reaction, and it is not a
    # state change (so a pet can never be mistaken for a finding).
    page.evaluate("window.CureCompanion.idle(null)")
    overview.locator('.luma-stage').first.click()
    check(page.evaluate("document.querySelector('[data-companion=\"overview\"] .luma-svg')"
                        ".classList.contains('lm-pulse')"),
          'clicking LUMA produces a reaction')
    check(overview.get_attribute('data-state')=='idle',
          'a pet does not change her state into a finding')

    # Sleeping is a real state and returns cleanly. The character area
    # carries one control now (Hide), so the state is driven through the
    # companion's own interface.
    page.evaluate("window.CureCompanion.sleep()")
    expect(overview).to_have_attribute('data-state','sleeping')
    page.evaluate("window.CureCompanion.wake()")
    expect(overview).to_have_attribute('data-state','idle')

    # Seal appears only after a completed, confirmed quarantine.
    check(page.locator('[data-companion="result"]').get_attribute('data-state')!='quarantine',
          'no quarantine pose before any quarantine')
    start_scan(page);finish_scan(page)
    goto_view(page, '#nav-results', 'results-view')
    settled(page)
    # Deliberate: LUMA is withheld from a findings-dense screen so she never
    # competes with evidence. She still receives state changes.
    check(page.locator('#companion-result').is_hidden(),
          'LUMA stays out of the way on a findings-dense screen')
    btn = page.locator('#review-cards .quarantine-btn').first
    expect(btn).to_be_visible()
    btn.click()
    page.locator('#confirm-ok').click()
    expect(page.locator('[data-companion="result"]')).to_have_attribute('data-state','quarantine')

    # Blinking and breathing actually run when motion is allowed. Measured on
    # the overview, because a hidden view reports no running animations.
    goto_view(page, '#nav-overview', 'view-overview')
    page.evaluate("window.CureCompanion.idle(null)")
    page.wait_for_timeout(400)
    running=page.evaluate("""() => document.getAnimations()
        .filter(a => a.playState === 'running'
                     && a.effect && a.effect.target
                     && String(a.effect.target.getAttribute('class')||'').includes('lm-'))
        .length""")
    check(running>0,'LUMA is genuinely animating when motion is allowed: '+str(running))

    check(not errors,'luma state runtime: '+str(errors));ctx.close()


def palette_and_assets():
    """Offline asset guard plus a measured, restrained palette check."""
    shipped=["index.html","style.css","xray.css","luma.css","storage-workstation.css",
             "detective.css","app.js","luma.js","xray.js","storage-workstation.js",
             "casefile.js","detective.js"]
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
        if mode in ("all","verify"):
            luma_states(browser);storage_failure(browser);detective_flow(browser);public_demo_flow(browser)
        if mode=="layout":responsive(browser)
        if mode=="pixel":core_flow(browser);pixel()
        if mode=="all":pixel()
        browser.close()
    server.shutdown();server.server_close()
    (SHOTS / "quality-results.json").write_text(json.dumps({"mode":mode,"checks":len(checks),"passed":checks},indent=2))
    print(f"PASS: {len(checks)} checks ({mode})")


if __name__=="__main__":main()
