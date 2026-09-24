// Scripted ≤60s demo: idle → Start Rescue → overlay close → scan →
// results → quarantine → undo. Records video (webm) for GIF/MP4 conversion.
import { chromium } from "playwright";
import { serveDist, openMock } from "./lib.mjs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "docs",
  "media"
);

const server = await serveDist();
const port = server.address().port;
const browser = await chromium.launch();
const errors = [];
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    recordVideo: { dir: OUT, size: { width: 1440, height: 900 } },
  });
  const page = await context.newPage();
  await page.addInitScript(() => {
    window.__CURE_MOCK_OVERLAY_HITS = 1;
  });
  page.on("pageerror", (e) => errors.push(String(e)));

  await page.goto(`http://127.0.0.1:${port}/index.dev.html`);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.waitForTimeout(1500); // idle beat

  await page.click("#start-rescue-btn");
  await page.waitForSelector("text=overlay candidate:", { timeout: 15000 });
  await page.waitForTimeout(1200); // card beat
  await page.click("text=Close window");
  // single candidate -> auto-continues into the scan
  await page.waitForSelector("#nav-results:not([disabled])", { timeout: 60000 });
  await page.click("#nav-results");
  await page.waitForSelector(".quarantine-btn", { timeout: 15000 });
  await page.waitForTimeout(1200); // results beat
  await page.click(".quarantine-btn >> nth=0");
  await page.waitForSelector("#confirm-ok", { timeout: 15000 });
  await page.waitForTimeout(800); // confirm beat
  await page.click("#confirm-ok");
  await page.waitForSelector("text=Quarantined", { timeout: 15000 });
  await page.waitForTimeout(800);
  await page.click("#nav-quarantine");
  await page.waitForSelector("button.q-undo", { timeout: 15000 });
  await page.waitForTimeout(600);
  await page.click("button.q-undo >> nth=0");
  await page.waitForFunction(
    () => document.body.textContent.includes("Restored:"),
    null,
    { timeout: 15000 }
  );
  await page.waitForTimeout(1200); // restored beat

  console.log("page errors:", errors.length ? errors : "none");
  await context.close();
} finally {
  await browser.close();
  server.close();
}
console.log("DEMO-OK");
