// Smoke: load the mock GUI, screenshot the idle/start screen.
import { chromium } from "playwright";
import { serveDist, openMock } from "./lib.mjs";

const server = await serveDist();
const port = server.address().port;
const browser = await chromium.launch();
try {
  const page = await openMock(browser);
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${port}/index.dev.html`);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.waitForTimeout(800);
  await page.screenshot({ path: "smoke-idle.png" });
  console.log("page errors:", errors.length ? errors : "none");
} finally {
  await browser.close();
  server.close();
}
console.log("SMOKE-OK");
