import { chromium } from "playwright";
import { fileURLToPath } from "node:url";
import path from "node:path";
import os from "node:os";
const dist = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");
const url = "file:///" + path.join(dist, "index.dev.html").replace(/\\/g, "/");
const outDir = process.env.CURE_SHOTS_DIR || path.join(os.tmpdir(), "cure-v41-check");
const browser = await chromium.launch();
const failures = [];
const ok = (n, c, x = "") => { console.log((c ? "ok   " : "FAIL ") + n + (x ? " [" + x + "]" : "")); if (!c) failures.push(n); };
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
const errs = [];
page.on("pageerror", (e) => errs.push(e.message));
page.on("console", (m) => { if (m.type() === "error") errs.push(m.text()); });
await page.goto(url);
await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
// topbar: breadcrumb + clock + badge
ok("breadcrumb home", (await page.textContent(".crumb-home"))?.trim() === "Home");
ok("clock ticking", ((await page.textContent("#topbar-clock")) || "").length > 5, await page.textContent("#topbar-clock"));
ok("canary badge", (await page.textContent(".nav-badge"))?.trim() === "Experimental");
ok("results hero in DOM pre-scan", (await page.locator("#results-view .hero-banner").count()) === 1);
await page.click("#start-rescue-btn");
await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
await page.waitForTimeout(1500);
ok("results hero visible", await page.locator("#results-view .hero-banner").isVisible());
ok("crumb current = Scan Results", ((await page.textContent("#view-title")) || "").includes("Scan Results"), await page.textContent("#view-title"));
// receipt on quarantine success
await page.click("#review-cards .quarantine-btn >> nth=0");
await page.waitForSelector("#confirm-overlay:not(.hidden)", { timeout: 5000 });
await page.click("#confirm-ok");
await page.waitForTimeout(1200);
const rcVisible = await page.locator("#action-receipt:not(.hidden)").isVisible().catch(() => false);
const rcText = ((await page.textContent("#action-receipt-text")) || "").trim();
ok("receipt shown on quarantine", rcVisible, rcText.slice(0, 80));
ok("receipt names the item", /Quarantined: .+ — moved to quarantine/.test(rcText), rcText.slice(0, 80));
await page.screenshot({ path: path.join(outDir, "v41-results.png") });
await page.click("#action-receipt-x");
await page.waitForTimeout(300);
ok("receipt dismisses", await page.locator("#action-receipt.hidden").count() === 1);
// quarantine hero
await page.click("#nav-quarantine");
await page.waitForTimeout(500);
ok("quarantine hero", await page.locator("#view-quarantine .hero-banner").isVisible());
// incident hero
await page.click("#nav-incident");
await page.waitForTimeout(500);
ok("incident hero", await page.locator("#view-incident .hero-banner").isVisible());
// cleanup: open, scan, steps + summary
await page.click("#open-cleanup").catch(() => page.click("#nav-cleanup"));
await page.waitForSelector("#cleanup-view:not(.hidden)", { timeout: 10000 });
await page.click("#cleanup-scan-btn");
await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 30000 });
await page.waitForTimeout(800);
const step1 = await page.getAttribute("#cleanup-steps .step[data-step='1']", "class");
ok("step1 current while reviewing", (step1 || "").includes("current"), step1);
const sum = await page.evaluate(() => ({
  items: document.getElementById("cs-total-items")?.textContent,
  size: document.getElementById("cs-total-size")?.textContent,
  sel: document.getElementById("cs-selected")?.textContent,
  status: document.getElementById("cs-status")?.textContent,
}));
console.log("summary: " + JSON.stringify(sum));
ok("summary items numeric", /^\d+$/.test((sum.items || "").trim()), sum.items);
ok("summary size has unit", /[KMG]?B/.test(sum.size || ""), sum.size);
ok("summary selected live", /items ·/.test(sum.sel || ""), sum.sel);
ok("summary status Ready", (sum.status || "").trim() === "Ready", sum.status);
await page.screenshot({ path: path.join(outDir, "v41-cleanup.png") });
// contrast on new elements
const c = await page.evaluate(() => {
  const lum = (rgb) => {
    const m = rgb.match(/[\d.]+/g).map(Number);
    const f = (v) => { v/=255; return v<=0.03928? v/12.92 : Math.pow((v+0.055)/1.055,2.4); };
    return 0.2126*f(m[0])+0.7152*f(m[1])+0.0722*f(m[2]);
  };
  const ratio = (fg,bg) => { const x=lum(fg),y=lum(bg); return ((Math.max(x,y)+0.05)/(Math.min(x,y)+0.05)); };
  const parse = (x) => x.match(/[\d.]+/g).map(Number);
  const comp = (el) => {
    let acc = [8,10,16]; const stack = []; let n = el;
    while (n && n !== document.body) { stack.unshift(getComputedStyle(n).backgroundColor); n = n.parentElement; }
    for (const col of stack) { const p = parse(col);
      if (p.length < 4 || p[3] >= 1) { if (!(p[0]===0&&p[1]===0&&p[2]===0&&p[3]===0)) acc=[p[0],p[1],p[2]]; }
      else { const t=p[3]; acc=[acc[0]*(1-t)+p[0]*t,acc[1]*(1-t)+p[1]*t,acc[2]*(1-t)+p[2]*t]; } }
    return "rgb("+acc.map(Math.round).join(", ")+")";
  };
  const out = {};
  for (const [s,k] of [[".hero-title","ht"],[".hero-desc","hd"],[".hero-tag","hg"],[".step-t","st"],[".step-d","sd"],[".sum-rows dd","sr"],[".important-box span","ib"],[".crumb-home","cb"],["#footbar-msg","fb"],[".receipt-x","rx"]]) {
    const el = document.querySelector(s);
    out[k] = el ? ratio(getComputedStyle(el).color, comp(el)).toFixed(2) : "missing";
  }
  return out;
});
console.log("contrast: " + JSON.stringify(c));
for (const [k,v] of Object.entries(c)) ok("contrast " + k + ">=4.5", v !== "missing" && Number(v) >= 4.5, String(v));
ok("no page errors", errs.length === 0, errs.join("; ").slice(0, 200));
await browser.close();
console.log(failures.length ? "V41 CHECKS: FAILURES\n" + failures.join("\n") : "V41 CHECKS: ALL PASS");
process.exit(failures.length ? 1 : 0);
