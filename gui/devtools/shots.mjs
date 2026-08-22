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

async function capture(label, viewport, opts = {}) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    reducedMotion: opts.reduced ? "reduce" : "no-preference",
  });
  const page = await context.newPage();
  const itemCount = opts.items ?? 50;
  await page.addInitScript((n) => {
    window.__CURE_MOCK_ITEM_COUNT = n;
  }, itemCount);
  await page.goto(devPageUrl);

  if (opts.phase === "early") {
    // fixed stage delays total ~1950ms before the first item event
    await page.waitForTimeout(2150);
    await page.screenshot({ path: join(shotsDir, `${label}-early.png`) });
  } else if (opts.phase === "mid") {
    await page.waitForFunction(
      () => (window.__cureNodeCount || 0) >= 14,
      null,
      { timeout: 30000 }
    );
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(shotsDir, `${label}-mid.png`) });
  } else if (opts.phase === "results") {
    await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
    await page.waitForTimeout(1900);
    await page.screenshot({ path: join(shotsDir, `${label}-results.png`) });
  }
  await context.close();
  console.log("captured:", label, opts.phase);
}

await capture("v2-900", { width: 900, height: 600 }, { phase: "early", items: 50 });
await capture("v2-900", { width: 900, height: 600 }, { phase: "mid", items: 50 });
await capture("v2-900", { width: 900, height: 600 }, { phase: "results", items: 8 });
await capture("v2-1920", { width: 1920, height: 1080 }, { phase: "early", items: 50 });
await capture("v2-1920", { width: 1920, height: 1080 }, { phase: "mid", items: 50 });
await capture("v2-1920", { width: 1920, height: 1080 }, { phase: "results", items: 11 });

await browser.close();
console.log("done");
