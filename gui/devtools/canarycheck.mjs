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
    if (cfg.canaryAlert) window.__CURE_MOCK_CANARY_ALERT = true;
  }, init);
  await page.goto(devPageUrl);
  await page.waitForSelector("#landing-view:not(.hidden)", { timeout: 15000 });
  return { context, page };
}

async function runScan(page) {
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(800);
}

// ── 1. canary toggle OFF state after scan ──────────────────────────────
async function guardOff(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 6, allClear: true });
  await runScan(page);
  await page.screenshot({ path: join(shotsDir, `canary-off-${tag}.png`) });
  console.log("guard-off:", tag);
  await context.close();
}

// ── 2. canary toggle ON state ──────────────────────────────────────────
async function guardOn(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 6, allClear: true });
  await runScan(page);
  const toggle = await page.$("#canary-toggle");
  if (toggle) {
    await toggle.click();
    await page.waitForTimeout(500);
  }
  await page.screenshot({ path: join(shotsDir, `canary-on-${tag}.png`) });
  console.log("guard-on:", tag);
  await context.close();
}

// ── 3. canary alert overlay ────────────────────────────────────────────
async function alertOverlay(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 6, allClear: true, canaryAlert: true });
  await runScan(page);
  const toggle = await page.$("#canary-toggle");
  if (toggle) {
    await toggle.click();
    await page.waitForTimeout(3000); // wait for mock alert to fire
  }
  await page.screenshot({ path: join(shotsDir, `canary-alert-${tag}.png`) });
  console.log("alert-overlay:", tag);
  await context.close();
}

// ── 4. dismiss alert ──────────────────────────────────────────────────
async function alertDismissed(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 6, allClear: true, canaryAlert: true });
  await runScan(page);
  const toggle = await page.$("#canary-toggle");
  if (toggle) {
    await toggle.click();
    await page.waitForTimeout(3000);
  }
  const dismissBtn = await page.$("#canary-dismiss-btn");
  if (dismissBtn) {
    await dismissBtn.click();
    await page.waitForTimeout(300);
  }
  await page.screenshot({ path: join(shotsDir, `canary-dismissed-${tag}.png`) });
  console.log("alert-dismissed:", tag);
  await context.close();
}

// ── 5. reduced-motion: guard block visible ─────────────────────────────
async function guardReduced(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 4, allClear: true, reduced: true });
  await runScan(page);
  await page.screenshot({ path: join(shotsDir, `canary-reduced-${tag}.png`) });
  console.log("guard-reduced:", tag);
  await context.close();
}

// ── run all ────────────────────────────────────────────────────────────
for (const [w, h, tag] of [[900, 600, "900x600"], [1920, 1080, "1920x1080"]]) {
  await guardOff({ width: w, height: h }, tag);
  await guardOn({ width: w, height: h }, tag);
  await alertOverlay({ width: w, height: h }, tag);
  await alertDismissed({ width: w, height: h }, tag);
}
await guardReduced({ width: 900, height: 600 }, "900x600");

await browser.close();
console.log("canary check done");
