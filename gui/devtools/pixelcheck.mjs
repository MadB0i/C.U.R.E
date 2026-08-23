import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join } from "node:path";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, "..", "dist");
const devPageUrl = pathToFileURL(join(distDir, "index.dev.html")).href;

const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 900, height: 600 },
  deviceScaleFactor: 1,
});
await page.addInitScript(() => { window.__CURE_MOCK_ITEM_COUNT = 30; });

await page.goto(devPageUrl);
await page.waitForFunction(() => (window.__cureNodeCount || 0) >= 14, null, {
  timeout: 20000,
});
await page.waitForTimeout(600);

const stats = await page.evaluate(() => {
  const canvas = document.getElementById("radar");
  const ctx = canvas.getContext("2d");
  const { width: w, height: h } = canvas;
  const img = ctx.getImageData(0, 0, w, h).data;
  let teal = 0, amber = 0, red = 0;
  const near = (r, g, b, tr, tg, tb) =>
    Math.abs(r - tr) < 45 && Math.abs(g - tg) < 45 && Math.abs(b - tb) < 45;
  for (let i = 0; i < img.length; i += 4) {
    if (img[i + 3] < 40) continue;
    const r = img[i], g = img[i + 1], b = img[i + 2];
    if (near(r, g, b, 209, 161, 63)) amber++;
    else if (near(r, g, b, 225, 89, 79)) red++;
    else if (near(r, g, b, 79, 174, 125)) teal++;
  }
  return {
    tealPx: teal,
    amberPx: amber,
    redPx: red,
    nodes: window.__cureNodeCount || 0,
    feedLines: document.querySelectorAll("#log li.item-line").length,
    feedSample: [...document.querySelectorAll("#log li.item-line")]
      .slice(-4)
      .map((li) => li.textContent.trim()),
  };
});

console.log(JSON.stringify(stats, null, 2));

if (stats.nodes < 5) {
  console.error("FAIL: too few nodes spawned");
  process.exit(1);
}
if (stats.feedLines < 5) {
  console.error("FAIL: feed did not accumulate item lines");
  process.exit(1);
}
if (stats.amberPx < 50 || stats.redPx < 50) {
  console.error("FAIL: no amber/red node pixels on canvas");
  process.exit(1);
}
console.log("OK: nodes rendered in multiple risk colors, feed populated");

// --- results view: scan map canvas must carry the same network over ---
await page.waitForSelector("#results-view:not(.hidden)", { timeout: 30000 });
await page.waitForTimeout(1200);

const mapStats = await page.evaluate(() => {
  const canvas = document.getElementById("map-canvas");
  const ctx = canvas.getContext("2d");
  const { width: w, height: h } = canvas;
  const img = ctx.getImageData(0, 0, w, h).data;
  let teal = 0, amber = 0, red = 0;
  const near = (r, g, b, tr, tg, tb) =>
    Math.abs(r - tr) < 45 && Math.abs(g - tg) < 45 && Math.abs(b - tb) < 45;
  for (let i = 0; i < img.length; i += 4) {
    if (img[i + 3] < 40) continue;
    const r = img[i], g = img[i + 1], b = img[i + 2];
    if (near(r, g, b, 209, 161, 63)) amber++;
    else if (near(r, g, b, 225, 89, 79)) red++;
    else if (near(r, g, b, 79, 174, 125)) teal++;
  }
  return {
    w,
    h,
    tealPx: teal,
    amberPx: amber,
    redPx: red,
    countText: document.getElementById("map-count").textContent.trim(),
    mockCount:
      window.__CURE_MOCK_ITEM_COUNT === undefined
        ? 8
        : window.__CURE_MOCK_ITEM_COUNT,
  };
});

console.log(JSON.stringify(mapStats, null, 2));

if (!(mapStats.w > 100 && mapStats.h > 100)) {
  console.error("FAIL: map canvas not sized to the panel");
  process.exit(1);
}
if (mapStats.tealPx < 30 || mapStats.redPx < 10) {
  console.error("FAIL: scan map missing risk-colored nodes/edges");
  process.exit(1);
}
const expectedCount = String(mapStats.mockCount) + " nodes";
if (mapStats.countText !== expectedCount) {
  console.error(`FAIL: map count "${mapStats.countText}" != "${expectedCount}"`);
  process.exit(1);
}
console.log("OK: scan map carries the settled network into the results view");
await browser.close();
