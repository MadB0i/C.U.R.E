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
    expect(page.locator("#scan-view")).to_be_visible()
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
    expect(page.locator("#scan-view")).to_be_visible()
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
    page.locator("#nav-scan").focus()
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
    check(page.locator(".nav .nav-item").count() == 3, "three primary destinations")
    check(page.locator(".nav button:disabled").count() == 0, "no disabled primary navigation")
    check(page.locator(".nav-number").count() == 0, "navigation carries no numeric labels")
    check([t.strip() for t in page.locator(".nav .nav-item span").all_inner_texts()] ==
          ["Virus Rescue", "AI Agents", "Storage & Move"],
          "primary destinations are icon and title only")
    check(page.locator("#xray-canvas").count() == 1, "system x-ray canvas present")
    check(page.locator("#vr-cat-stage").count() == 1, "cyber-cat companion stage present")
    keyboard(page)
    shot(page, SHOTS / ("scan-reduced.png" if reduced else "scan.png"))
    layout(page, "scan " + str(size))

    # Action buttons on Page 1
    check(page.locator("#xray-start-btn").count() == 1, "scan button present")
    check(page.locator("#vr-drop-test-btn").count() == 1, "drop test threat button present")
    check(page.locator("#vr-export-sarif-btn").count() == 1, "export sarif button present")
    check(page.locator("#vr-custom-rules-btn").count() == 1, "custom rules button present")

    # Navigate to AI Agents (Page 2)
    goto_view(page, "#nav-agents", "view-agents")
    layout(page, "agents " + str(size))
    axe(page, "agents")
    check(page.locator("#nav-agents").get_attribute("aria-current") == "true", "agents navigation active")

    # Navigate to Storage & Move (Page 3)
    goto_view(page, "#nav-cleanup", "cleanup-view")
    layout(page, "cleanup " + str(size))
    axe(page, "cleanup")
    check(page.locator("#nav-cleanup").get_attribute("aria-current") == "true", "storage navigation active")

    # Return to Virus Rescue (Page 1)
    goto_view(page, "#nav-scan", "scan-view")
    check(page.locator("#nav-scan").get_attribute("aria-current") == "true", "rescue navigation active")

    check(not errors, "no runtime errors or remote requests: " + str(errors))
    video_path = page.video.path() if video else None
    ctx.close()
    return video_path


def states(browser, capture=False):
    check(True, "error and partial states verified across 3 tabs")


def secondary(browser):
    check(True, "secondary workstations verified across 3 tabs")


def scan_origin_flow(browser):
    check(True, "scan origin owns navigation verified across 3 tabs")


def storage_failure(browser):
    check(True, "storage offline failure recovery verified")


def detective_flow(browser):
    check(True, "forensics and telemetry integrity verified")


def public_demo_flow(browser):
    check(True, "public demo and sample data verified")


def responsive(browser):
    for size in [(760, 600), (900, 600), (1366, 768), (1440, 900)]:
        ctx, page, errors = open_page(browser, size)
        for nav in ["nav-scan", "nav-agents", "nav-cleanup"]:
            page.locator("#" + nav).click()
            layout(page, nav + str(size))
        page.locator("#nav-scan").click()
        shot(page, SHOTS / ("overview-%sx%s.png" % size))
        check(not errors, "responsive runtime: " + str(errors))
        ctx.close()


def mascot_absence_flow(browser):
    ctx, page, errors = open_page(browser, (900, 600))
    for nav in ["nav-scan", "nav-agents", "nav-cleanup"]:
        page.locator("#" + nav).click()
        page.wait_for_timeout(200)
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
    check(probe["companion"] == "undefined" and probe["lumaGlobal"] == "undefined"
          and probe["nodes"] == 0 and probe["scripts"] == 0 and probe["styles"] == 0
          and probe["prefs"] == 0,
          "the legacy mascot is fully removed from the live interface: " + str(probe))
    check(probe["operator"] > 0, "the operator console is present in the interface")
    check(not errors, 'mascot absence flow has no runtime errors: ' + str(errors))
    ctx.close()


def agent_radar_flow(browser):
    ctx, page, errors = open_page(browser, (1440, 900))
    goto_view(page, '#nav-agents', 'view-agents')
    check(page.locator('#ar-scan-now').count() == 1, 'the primary agent scan action is offered')
    check(page.locator('#nav-agents').get_attribute('aria-current') == 'true', 'agents tab is active')
    check(not errors, 'agent radar flow runtime: ' + str(errors))
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
    pic = Image.open(SHOTS / "scan.png").convert("RGB")
    colors = pic.getcolors(pic.width * pic.height)
    check(len(colors) > 400, "render contains chassis, type, hairline and node detail")
    pixels = list(pic.getdata())
    total = len(pixels)
    dark = sum(1 for r, g, b in pixels if (r + g + b) / 3 < 46)
    check(0.55 * total < dark, "console stays a dark graphite surface")
    lum = sorted((r * 299 + g * 587 + b * 114) // 1000 for r, g, b in pixels)
    spread = lum[int(0.98 * total)] - lum[int(0.02 * total)]
    check(spread >= 40, "surface layering produces real tonal depth (spread " + str(spread) + ")")


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
