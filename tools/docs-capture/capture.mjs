// Deterministic docs screenshots at 1440x900 against the mock backend
// (gui/dist/index.dev.html + mock-tauri.js). All data on screen is sample
// data — every published image gets a "sample data" caption.
import { chromium } from "playwright";
import { serveDist, openMock } from "./lib.mjs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "docs",
  "screenshots"
);

const server = await serveDist();
const port = server.address().port;
const browser = await chromium.launch();
const errors = [];
try {
  const page = await openMock(browser, { overlayHits: 1 });
  page.on("pageerror", (e) => errors.push(String(e)));

  const shot = (name) => page.screenshot({ path: path.join(OUT, name) });

  // 01 — idle / start screen.
  await page.goto(`http://127.0.0.1:${port}/index.dev.html`);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.waitForTimeout(900);
  await shot("01-idle.png");
  console.log("01-idle");

  // Start Rescue -> overlay review card (mock reports 1 hit).
  await page.click("#start-rescue-btn");
  await page.waitForSelector("text=overlay candidate:", { timeout: 15000 });
  await page.waitForTimeout(400);
  await shot("06-overlay-card.png");
  console.log("06-overlay-card");

  // Close the candidate (graceful). With a single candidate reviewed,
  // the UI auto-continues into the scan — no Continue click needed.
  await page.click("text=Close window");
  await page.waitForTimeout(1500);
  await shot("02-scanning.png");
  console.log("02-scanning");

  // Results view with risk levels.
  await page.waitForSelector("#nav-results:not([disabled])", { timeout: 60000 });
  await page.click("#nav-results");
  await page.waitForSelector(".quarantine-btn", { timeout: 15000 });
  await page.waitForTimeout(600);
  await shot("03-results.png");
  console.log("03-results");

  // Quarantine confirmation modal.
  await page.click(".quarantine-btn >> nth=0");
  await page.waitForSelector("#confirm-ok", { timeout: 15000 });
  await page.waitForTimeout(400);
  await shot("04-quarantine-confirm.png");
  console.log("04-quarantine-confirm");
  await page.click("#confirm-ok");
  await page.waitForSelector("text=Quarantined", { timeout: 15000 });
  await page.waitForTimeout(400);

  // Undo from the Quarantine view (row button.q-undo).
  await page.click("#nav-quarantine");
  await page.waitForSelector("button.q-undo", { timeout: 15000 });
  await page.waitForTimeout(400);
  await shot("05-undo.png");
  console.log("05-undo");
  await page.click("button.q-undo >> nth=0");
  await page.waitForFunction(
    () => document.body.textContent.includes("Restored:"),
    null,
    { timeout: 15000 }
  );
  await page.waitForTimeout(400);

  // Overview (status/about substitute — the app has no settings view).
  await page.click("#nav-overview");
  await page.waitForTimeout(600);
  await shot("07-overview.png");
  console.log("07-overview");

  console.log("page errors:", errors.length ? errors : "none");
} finally {
  await browser.close();
  server.close();
}
console.log("CAPTURE-OK");
