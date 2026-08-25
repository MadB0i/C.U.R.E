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

async function newPage(viewport, init) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: 1 });
  const page = await context.newPage();
  await page.addInitScript((cfg) => {
    window.__CURE_MOCK_ITEM_COUNT = cfg.items ?? 10;
    if (cfg.allClear) window.__CURE_MOCK_ALL_CLEAR = true;
    if (cfg.sweep) window.__CURE_MOCK_SWEEP = true;
  }, init);
  await page.goto(devPageUrl);
  await page.waitForSelector("#landing-view:not(.hidden)", { timeout: 15000 });
  return { context, page };
}

async function resultsShot(viewport, tag, init, name) {
  const { context, page } = await newPage(viewport, init);
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(2200);
  await page.screenshot({ path: join(shotsDir, name) });
  console.log("captured:", name);
  await context.close();
}

async function cleanupIdleShot(viewport, tag) {
  const { context, page } = await newPage(viewport, { items: 10, allClear: true });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(1800);
  await page.click("#open-cleanup");
  await page.waitForSelector("#cleanup-idle:not(.hidden)", { timeout: 15000 });
  await page.waitForTimeout(700);
  await page.screenshot({ path: join(shotsDir, `v2-cleanup-idle-${tag}.png`) });
  console.log("captured:", `v2-cleanup-idle-${tag}.png`);
  await context.close();
}

for (const [w, h, tag] of [[900, 600, "900x600"], [1920, 1080, "1920x1080"]]) {
  await resultsShot({ width: w, height: h }, tag, { items: 10, allClear: true }, `v2-allclear-${tag}.png`);
  await resultsShot({ width: w, height: h }, tag, { items: 12, sweep: true }, `v2-findings-${tag}.png`);
  await cleanupIdleShot({ width: w, height: h }, tag);
}

await browser.close();
console.log("done");
