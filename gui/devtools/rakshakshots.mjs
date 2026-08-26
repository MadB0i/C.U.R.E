import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, "..", "dist");
const shotsDir = join(here, "..", "dev-screenshots");
const devPageUrl = pathToFileURL(join(distDir, "index.dev.html")).href;

mkdirSync(shotsDir, { recursive: true });

const browser = await chromium.launch();

function sameBytes(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

async function newPage(viewport, init = {}) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    reducedMotion: init.reduced ? "reduce" : "no-preference",
  });
  const page = await context.newPage();
  await page.addInitScript((cfg) => {
    window.__CURE_MOCK_ITEM_COUNT = cfg.items ?? 8;
    if (cfg.allClear) window.__CURE_MOCK_ALL_CLEAR = true;
  }, init);
  await page.goto(devPageUrl);
  await page.waitForSelector("#landing-view:not(.hidden)", { timeout: 15000 });
  return { context, page };
}

async function runScan(page) {
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
}

// ── 1. patrol frames (all-clear, after guard hold relaxes) ──────────────
async function patrol(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 10, allClear: true });
  await runScan(page);
  await page.waitForTimeout(7000); // wash + guard hold + relax done, patrol underway
  await page.screenshot({ path: join(shotsDir, `rk-patrol-a-${tag}.png`) });
  await page.waitForTimeout(2600);
  await page.screenshot({ path: join(shotsDir, `rk-patrol-b-${tag}.png`) });
  console.log("patrol frames:", tag);
  await context.close();
}

// ── 2. mid-scan: Rakshak visiting a node ────────────────────────────────
async function midscan(viewport, tag, reduced = false) {
  const { context, page } = await newPage(viewport, { items: 14, reduced });
  await page.click("#start-rescue-btn");
  if (!reduced) {
    await page
      .waitForFunction(() => window.__cureVisitActive === true, null, { timeout: 30000 })
      .catch(() => console.log("warn: no visit window caught (reduced?)"));
  }
  await page.waitForTimeout(reduced ? 4000 : 120);
  await page.screenshot({ path: join(shotsDir, `rk-midscan-${tag}.png`) });
  console.log("midscan:", tag);
  await context.close();
}

// ── 3. completion flourish (catch the wash mid-sweep) ───────────────────
async function flourish(viewport, tag, reduced = false) {
  const { context, page } = await newPage(viewport, { items: 10, allClear: true, reduced });
  await runScan(page);
  // wash starts 750ms after show (post-reveal); catch it mid-sweep
  await page.waitForTimeout(reduced ? 400 : 1150);
  await page.screenshot({ path: join(shotsDir, `rk-flourish-${tag}.png`) });
  console.log("flourish:", tag);
  await context.close();
}

// ── 4. fight gesture (HighRisk auto-clean escalation) ───────────────────
async function fight(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 8 });
  await page.click("#start-rescue-btn");
  await page
    .waitForFunction(() => window.__cureMascotActive === true, null, { timeout: 30000 })
    .catch(() => console.log("warn: fight window missed"));
  await page.screenshot({ path: join(shotsDir, `rk-fight-${tag}.png`) });
  console.log("fight:", tag);
  await context.close();
}

// ── 5. reduced-motion: patrol frames must be pixel-identical ────────────
async function reducedStatic(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 10, allClear: true, reduced: true });
  await runScan(page);
  await page.waitForTimeout(600);
  const a = await page.screenshot();
  await page.waitForTimeout(2200);
  const b = await page.screenshot();
  console.log(`reduced static ${tag}: frames identical = ${sameBytes(a, b)}`);
  await page.screenshot({ path: join(shotsDir, `rk-reduced-${tag}.png`) });
  await context.close();
}

for (const [w, h, tag] of [[900, 600, "900x600"], [1920, 1080, "1920x1080"]]) {
  await patrol({ width: w, height: h }, tag);
  await flourish({ width: w, height: h }, tag);
}
await midscan({ width: 900, height: 600 }, "900x600");
await midscan({ width: 1920, height: 1080 }, "1920x1080");
await midscan({ width: 900, height: 600 }, "reduced", true);
await fight({ width: 900, height: 600 }, "900x600");
await reducedStatic({ width: 900, height: 600 }, "900x600");

await browser.close();
console.log("done");
