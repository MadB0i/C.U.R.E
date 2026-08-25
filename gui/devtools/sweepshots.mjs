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

async function captureSweep(viewport, tag) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: 1 });
  const page = await context.newPage();
  await page.addInitScript(() => {
    window.__CURE_MOCK_ITEM_COUNT = 10;
    window.__CURE_MOCK_SWEEP = true;
  });
  await page.goto(devPageUrl);
  await page.waitForSelector("#landing-view:not(.hidden)", { timeout: 15000 });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(2200);
  await page.screenshot({ path: join(shotsDir, `sweep-results-${tag}.png`), fullPage: false });
  console.log("captured sweep results:", tag);
  await context.close();
}

await captureSweep({ width: 900, height: 600 }, "900x600");
await captureSweep({ width: 1920, height: 1080 }, "1920x1080");
await browser.close();
console.log("done — sweep screenshots saved to dev-screenshots/");
