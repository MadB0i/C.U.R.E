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
    Math.abs(r - tr) < 60 && Math.abs(g - tg) < 60 && Math.abs(b - tb) < 60;
  for (let i = 0; i < img.length; i += 4) {
    if (img[i + 3] < 40) continue;
    const r = img[i], g = img[i + 1], b = img[i + 2];
    if (near(r, g, b, 255, 196, 107)) amber++;
    else if (near(r, g, b, 255, 93, 110)) red++;
    else if (near(r, g, b, 77, 227, 176)) teal++;
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
await browser.close();
