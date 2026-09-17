import { chromium } from "playwright";
import { fileURLToPath } from "node:url";
import path from "node:path";
const dist = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");
const url = "file:///" + path.join(dist, "index.dev.html").replace(/\\/g, "/");
const browser = await chromium.launch();
const failures = [];
for (const [w, h] of [[900,700],[1280,720],[1366,768],[1600,900],[1920,1080]]) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  const errs = [];
  page.on("pageerror", (e) => errs.push("pageerror: " + e.message));
  page.on("console", (m) => { if (m.type() === "error") errs.push("console: " + m.text()); });
  await page.goto(url);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(1500);
  const r = await page.evaluate(() => {
    const de = document.documentElement;
    const btn = document.querySelector("#btn-quarantine-folder");
    const br = btn.getBoundingClientRect();
    const elAt = document.elementFromPoint(br.x + br.width / 2, br.y + br.height / 2);
    const groups = document.querySelectorAll(".nav-group").length;
    const meta = (document.getElementById("session-meta") || {}).textContent || "";
    return { hOverflow: de.scrollWidth - de.clientWidth, btnReachable: btn.contains(elAt) || elAt === btn, groups, meta };
  });
  // quarantine flow: confirm -> undo path, q-paths contrast
  await page.click("#review-cards .quarantine-btn >> nth=0").catch(() => {});
  await page.waitForSelector("#confirm-overlay:not(.hidden)", { timeout: 5000 }).catch(() => {});
  const modal = await page.evaluate(async () => {
    const ov = document.querySelector("#confirm-overlay");
    if (!ov || ov.classList.contains("hidden")) return { opened: false };
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await new Promise((r) => setTimeout(r, 150));
    const a = document.activeElement;
    return { opened: true, closed: ov.classList.contains("hidden"), focusAfter: (a && (a.id || a.className)) || "(none)" };
  });
  await page.click("#review-cards .quarantine-btn >> nth=0").catch(() => {});
  await page.waitForSelector("#confirm-overlay:not(.hidden)", { timeout: 5000 }).catch(() => {});
  await page.click("#confirm-ok").catch(() => {});
  await page.waitForTimeout(800);
  await page.click("#nav-quarantine").catch(() => {});
  await page.waitForTimeout(600);
  const qp = await page.evaluate(() => {
    const el = document.querySelector(".q-paths");
    if (!el) return "no-quarantined-items";
    const lum = (rgb) => {
      const m = rgb.match(/[\d.]+/g).map(Number);
      const f = (v) => { v/=255; return v<=0.03928? v/12.92 : Math.pow((v+0.055)/1.055,2.4); };
      return 0.2126*f(m[0])+0.7152*f(m[1])+0.0722*f(m[2]);
    };
    const parse = (c) => c.match(/[\d.]+/g).map(Number);
    let acc = [8,10,16]; const stack = []; let n = el;
    while (n && n !== document.body) { stack.unshift(getComputedStyle(n).backgroundColor); n = n.parentElement; }
    for (const c of stack) { const p = parse(c);
      if (p.length < 4 || p[3] >= 1) { if (!(p[0]===0&&p[1]===0&&p[2]===0&&p[3]===0)) acc=[p[0],p[1],p[2]]; }
      else { const t=p[3]; acc=[acc[0]*(1-t)+p[0]*t,acc[1]*(1-t)+p[1]*t,acc[2]*(1-t)+p[2]*t]; } }
    const bg = "rgb("+acc.map(Math.round).join(", ")+")";
    const x = lum(getComputedStyle(el).color), y = lum(bg);
    return ((Math.max(x,y)+0.05)/(Math.min(x,y)+0.05)).toFixed(2);
  });
  const line = `${w}x${h}: hOverflow=${r.hOverflow}px btnReachable=${r.btnReachable} groups=${r.groups} meta="${r.meta.slice(0,44)}" modal=${modal.opened ? (modal.closed ? "esc-ok->" + modal.focusAfter : "ESC-FAIL") : "not-opened"} qp=${qp} errs=${errs.length}`;
  console.log(line);
  if (r.hOverflow > 0 || !r.btnReachable || (modal.opened && !modal.closed) || errs.length) failures.push(line + " || " + errs.join("; "));
  if (r.groups !== 4) failures.push(`nav groups=${r.groups} @${w}x${h}, want 4`);
  if (!/CHECKED/.test(r.meta)) failures.push(`session-meta not populated @${w}x${h}: ${r.meta}`);
  if (qp !== "no-quarantined-items" && Number(qp) < 4.5) failures.push(`q-paths contrast=${qp} @${w}x${h}`);
  await page.close();
}
{
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await page.goto(url);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(1500);
  const a = await page.evaluate(() => {
    const lum = (rgb) => {
      const m = rgb.match(/[\d.]+/g).map(Number);
      const f = (v) => { v/=255; return v<=0.03928? v/12.92 : Math.pow((v+0.055)/1.055,2.4); };
      return 0.2126*f(m[0])+0.7152*f(m[1])+0.0722*f(m[2]);
    };
    const ratio = (fg,bg) => { const x=lum(fg),y=lum(bg); return ((Math.max(x,y)+0.05)/(Math.min(x,y)+0.05)); };
    const parse = (c) => c.match(/[\d.]+/g).map(Number);
    const comp = (el) => {
      let acc = [8,10,16]; const stack = []; let n = el;
      while (n && n !== document.body) { stack.unshift(getComputedStyle(n).backgroundColor); n = n.parentElement; }
      for (const c of stack) { const p = parse(c);
        if (p.length < 4 || p[3] >= 1) { if (!(p[0]===0&&p[1]===0&&p[2]===0&&p[3]===0)) acc=[p[0],p[1],p[2]]; }
        else { const t=p[3]; acc=[acc[0]*(1-t)+p[0]*t,acc[1]*(1-t)+p[1]*t,acc[2]*(1-t)+p[2]*t]; } }
      return "rgb("+acc.map(Math.round).join(", ")+")";
    };
    const out = {};
    for (const [s,k] of [[".metric-label","metric"],[".empty-state:not(.hidden)","empty"],[".chip.red","chipR"],[".chip.amber","chipA"],[".subline","sub"],[".rc-name","rcn"],[".session-meta","sess"],[".nav-group","navg"]]) {
      const el = document.querySelector(s);
      out[k] = el ? ratio(getComputedStyle(el).color, comp(el)).toFixed(2) : "missing";
    }
    out.procCaption = !!document.querySelector("#proc-table caption.sr-only");
    out.canaryRole = document.querySelector(".canary-alert-box")?.getAttribute("role");
    out.navLabels = [...document.querySelectorAll(".nav-item")].every((b) => b.getAttribute("aria-label"));
    out.selectable = document.querySelectorAll(".selectable").length;
    return out;
  });
  console.log("a11y: " + JSON.stringify(a));
  for (const [k,v] of Object.entries({metric:a.metric,empty:a.empty,chipR:a.chipR,chipA:a.chipA,sub:a.sub,rcn:a.rcn,sess:a.sess,navg:a.navg})) {
    if (v === "missing" || Number(v) < 4.5) failures.push("contrast " + k + "=" + v);
  }
  if (!a.procCaption) failures.push("proc table caption missing");
  if (a.canaryRole !== "alertdialog") failures.push("canary role=" + a.canaryRole);
  if (!a.navLabels) failures.push("nav aria-label missing");
  if (!a.selectable) failures.push("no selectable evidence");
  await page.close();
}
await browser.close();
console.log(failures.length ? "FAILURES:\n" + failures.join("\n") : "ALL V4 RESPONSIVE/OVERLAP/A11Y CHECKS PASSED");
process.exit(failures.length ? 1 : 0);
