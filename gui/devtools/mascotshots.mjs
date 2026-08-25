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

async function captureLanding(viewport, tag) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  await page.addInitScript(() => {
    window.__CURE_MOCK_RESCUE = false;
    window.__CURE_MOCK_ITEM_COUNT = 8;
  });
  await page.goto(devPageUrl);
  await page.waitForSelector("#landing-view:not(.hidden)", { timeout: 15000 });
  await page.waitForTimeout(600);
  await page.screenshot({ path: join(shotsDir, `landing-${tag}.png`) });
  await context.close();
  console.log("captured landing:", tag);
}

async function captureSet(viewport, tag) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  await page.addInitScript((n) => {
    window.__CURE_MOCK_ITEM_COUNT = n;
    window.__CURE_MOCK_CLEANUP_DELAY_MS = 2600;
  }, 50);
  await page.goto(devPageUrl);

  // watcher-triggered flow: auto-scan starts immediately (unchanged behavior)
  await page.waitForFunction(() => window.__cureMascotActive === true, null, {
    timeout: 60000,
  });
  await page.screenshot({ path: join(shotsDir, `mascot-scan-${tag}.png`) });

  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(1900);
  await page.screenshot({ path: join(shotsDir, `mascot-results-${tag}.png`) });

  // cleanup: idle first, explicit scan, then run
  await page.click("#open-cleanup");
  await page.waitForSelector("#cleanup-idle:not(.hidden)", { timeout: 15000 });
  await page.screenshot({ path: join(shotsDir, `cleanup-idle-${tag}.png`) });
  await page.click("#cleanup-scan-btn");
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  await page.waitForTimeout(400);
  await page.check('#cleanup-dl-list input[data-path*="setup_toolkit"]');
  const btn = page.locator("#cleanup-btn");
  await btn.click();
  await page.waitForTimeout(120);
  await btn.click();

  await page.waitForFunction(() => window.__cureTossActive === true, null, {
    timeout: 5000,
  });
  await page.waitForTimeout(650);
  await page.screenshot({ path: join(shotsDir, `mascot-toss-${tag}.png`) });

  await page.waitForFunction(
    () => document.getElementById("cleanup-status").textContent.startsWith("Freed"),
    null,
    { timeout: 20000 }
  );
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(shotsDir, `mascot-done-${tag}.png`) });

  await context.close();
  console.log("captured set:", tag);
}

await captureLanding({ width: 900, height: 600 }, "900");
await captureLanding({ width: 1920, height: 1080 }, "1920");
await captureSet({ width: 900, height: 600 }, "900");
await captureSet({ width: 1920, height: 1080 }, "1920");

await browser.close();
console.log("done");
