import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join } from "node:path";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const devPageUrl = pathToFileURL(join(here, "..", "dist", "index.dev.html")).href;

const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 900, height: 600 },
  deviceScaleFactor: 1,
});
page.on("console", (m) => console.log("[console]", m.type(), m.text()));
page.on("pageerror", (e) => console.log("[pageerror]", e.message));

await page.addInitScript(() => { window.__CURE_MOCK_ITEM_COUNT = 30; });
await page.goto(devPageUrl);

for (let i = 0; i < 24; i++) {
  await page.waitForTimeout(500);
  const s = await page.evaluate(() => ({
    nodes: window.__cureNodeCount || 0,
    pings: window.__curePingCount || 0,
    feed: document.querySelectorAll("#log li.item-line").length,
    resultsVisible: !document.getElementById("results-view").classList.contains("hidden"),
  }));
  console.log(`${(i + 1) * 0.5}s`, JSON.stringify(s));
}
await browser.close();
