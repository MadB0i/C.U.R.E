"""Native Python Playwright quality gates and documentation capture.

All mutations use mock-tauri.js; no host files/processes are remediated.
Run: python gui/devtools/evidence-console.py [verify|layout|pixel|capture|demo|all]
"""
from pathlib import Path
from datetime import datetime, timezone
import argparse
import json
import os
import subprocess
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
    check(page.locator(".nav .nav-item").count() == 5, "five primary destinations")
    check(page.locator(".nav button:disabled").count() == 0, "no disabled primary navigation")
    keyboard(page)
    start_scan(page)
    page.wait_for_function("document.querySelectorAll('#log .item-line').length >= 2")
    layout(page, "scan " + str(size))
    if capture:
        shot(page, DOCS / "02-scan.png")
    else:
        shot(page, SHOTS / ("scan-reduced.png" if reduced else "scan.png"))
    check(page.locator("#sweep-items").inner_text().startswith("2 ") or
          int(page.locator("#sweep-items").inner_text().split()[0]) >= 2, "sweep uses collected item count")
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
        shot(page, DOCS / "01-overview.png")
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
            if name == "all-clear":
                expect(page.locator("#review-clear")).to_be_visible()
                check("No persistence findings" in page.locator("#review-clear").inner_text(), "completed empty result uses collected-evidence wording")
                if capture:shot(page, DOCS / "07-all-clear.png")
            if name == "guidance":
                row = page.locator("#review-cards .review-card").first
                check(row.locator(".quarantine-btn").count() == 0, "non-file-backed source gets guidance only")
            if name == "long-values":
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
    page.locator("#nav-scan").click();page.locator("#start-cleanup-btn").click()
    page.locator("#cleanup-scan-btn").click()
    expect(page.locator("#cleanup-body")).to_be_visible()
    page.locator(".cleanup-cat").first.click()
    page.locator("#cleanup-btn").click()
    expect(page.locator("#confirm-overlay")).to_be_visible()
    page.locator("#confirm-ok").click()
    expect(page.locator("#cleanup-status")).to_be_visible()
    check(page.evaluate("window.__CURE_LAST_CLEANUP_CALL !== undefined"), "cleanup runs only confirmed selections")
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
    for size in [(390, 844), (760, 600), (900, 600), (1440, 900), (1920, 1080)]:
        ctx, page, errors = open_page(browser, size)
        for nav in ["nav-overview", "nav-scan", "nav-results", "nav-quarantine", "nav-eventlog"]:
            page.locator("#" + nav).click()
            if nav == "nav-quarantine":expect(page.locator("#q-empty")).to_be_visible()
            layout(page, nav + str(size))
        page.locator("#nav-overview").click()
        shot(page, SHOTS / ("overview-%sx%s.png" % size))
        check(not errors, "responsive runtime: " + str(errors));ctx.close()


def demo_flow(browser):
    ctx,page,errors=open_page(browser,(1440,900),video=True)
    page.wait_for_timeout(900)  # Intentional presentation beats, not test synchronization.
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
    path=page.video.path();ctx.close()
    Path(path).replace(ROOT/'docs/media/demo.webm')
    check(not errors,'demo runtime: '+str(errors))


def pixel():
    pic=Image.open(SHOTS / "scan.png").convert("RGB")
    colors=pic.getcolors(pic.width*pic.height)
    check(len(colors)>100, "render contains topology, text and hairline detail")
    violet=sum(n for n,(r,g,b) in colors if b>r and r>g and b-g>25)
    check(20<violet<pic.width*pic.height*.1, "violet is visible and restrained")
    check(not any(word in (ROOT / "gui/dist/style.css").read_text().lower() for word in ["radial-gradient", "linear-gradient", "pulse", "glow"]), "no decorative gradient or pulse system")


def main():
    global URL
    # Ephemeral loopback server lets axe inspect CSS without file-origin CORS errors.
    class QuietHandler(SimpleHTTPRequestHandler):
        def log_message(self, *_): pass
    server=ThreadingHTTPServer(('127.0.0.1',0),partial(QuietHandler,directory=str(ROOT/'gui/dist')))
    threading.Thread(target=server.serve_forever,daemon=True).start()
    URL=f'http://127.0.0.1:{server.server_port}/index.dev.html'
    args=argparse.ArgumentParser();args.add_argument("mode",nargs="?",default="all")
    mode=args.parse_args().mode;SHOTS.mkdir(parents=True,exist_ok=True)
    with sync_playwright() as p:
        browser=p.chromium.launch()
        if mode in ("all","verify","capture"):
            core_flow(browser,(1440,900) if mode=="capture" else (900,600),capture=mode=="capture")
        if mode=="demo":demo_flow(browser)
        if mode in ("all","verify"):
            core_flow(browser,reduced=True);states(browser);secondary(browser);responsive(browser)
        if mode=="capture":states(browser,capture=True)
        if mode=="layout":responsive(browser)
        if mode=="pixel":core_flow(browser);pixel()
        if mode=="all":pixel()
        browser.close()
    server.shutdown();server.server_close()
    (SHOTS / "quality-results.json").write_text(json.dumps({"mode":mode,"checks":len(checks),"passed":checks},indent=2))
    print(f"PASS: {len(checks)} checks ({mode})")


if __name__=="__main__":main()
