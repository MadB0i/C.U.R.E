import { chromium } from "playwright";
import { fileURLToPath } from "node:url";
import path from "node:path";
import os from "node:os";
const dist = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");
const url = "file:///" + path.join(dist, "index.dev.html").replace(/\\/g, "/");
const shots = process.env.CURE_SHOTS_DIR || path.join(os.tmpdir(), "cure-v4-shots");
const { mkdirSync } = await import("node:fs");
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch();
const failures = [];
for (const [w, h] of [[1280,720],[1366,768],[1920,1080]]) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  const errs = [];
  page.on("pageerror", (e) => errs.push("pageerror: " + e.message));
  page.on("console", (m) => { if (m.type() === "error") errs.push("console: " + m.text()); });
  await page.goto(url);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(1800);
  await page.screenshot({ path: `${shots}/v4-results-${w}x${h}.png` });
  // open a finding's confirm modal for the modal shot (largest viewport only)
  if (w === 1920) {
    await page.click("#review-cards .quarantine-btn >> nth=0").catch(() => {});
    await page.waitForSelector("#confirm-overlay:not(.hidden)", { timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(400);
    await page.screenshot({ path: `${shots}/v4-modal-${w}x${h}.png` });
    await page.click("#confirm-cancel").catch(() => {});
  }
  // walk every nav view, screenshot overview + incident, assert no errors
  for (const nav of ["#nav-overview", "#nav-incident", "#nav-processes", "#nav-audit", "#nav-quarantine", "#nav-cleanup", "#nav-eventlog", "#nav-canary"]) {
    await page.click(nav).catch(() => {});
    await page.waitForTimeout(350);
  }
  await page.click("#nav-overview");
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${shots}/v4-overview-${w}x${h}.png` });
  await page.click("#nav-incident");
  await page.waitForTimeout(800);
  await page.screenshot({ path: `${shots}/v4-incident-${w}x${h}.png` });
  const ov = await page.evaluate(() => ({
    metrics: document.querySelectorAll("#ov-metrics .metric-card").length,
    panels: ["#ov-coverage-panel", "#ov-report-panel", "#ov-actions-panel", "#ov-incident-panel"].map((s) => !!document.querySelector(s)),
  }));
  console.log(`${w}x${h}: overview metrics=${ov.metrics} panels=${ov.panels.map(Number).join("")} errs=${errs.length}`);
  if (ov.metrics !== 8) failures.push(`overview metrics=${ov.metrics} @${w}x${h}`);
  if (ov.panels.some((p) => !p)) failures.push(`overview panel missing @${w}x${h}`);
  if (errs.length) failures.push(`errors @${w}x${h}: ` + errs.join("; "));
  await page.close();
}
await browser.close();
console.log(failures.length ? "FAILURES:\n" + failures.join("\n") : "ALL VIEWS SMOKE OK");
process.exit(failures.length ? 1 : 0);
