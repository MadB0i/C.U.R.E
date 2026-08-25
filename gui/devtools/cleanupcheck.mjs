import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, "..", "dist");
const shotsDir = join(here, "..", "dev-screenshots");
const devPageUrl = pathToFileURL(join(distDir, "index.dev.html")).href;

mkdirSync(shotsDir, { recursive: true });

let failures = 0;
function check(label, ok, extra = "") {
  console.log((ok ? "PASS" : "FAIL") + "  " + label + (ok ? "" : "  " + extra));
  if (!ok) failures += 1;
}

async function openPage(browser, init = {}, viewport = { width: 900, height: 600 }) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    reducedMotion: init.reduced ? "reduce" : "no-preference",
  });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));
  await page.addInitScript((cfg) => {
    window.__CURE_MOCK_ITEM_COUNT = cfg.n ?? 8;
    window.__CURE_MOCK_CLEANUP_FAILURES = !!cfg.cleanupFailures;
    window.__CURE_MOCK_CLEANUP_DELAY_MS = cfg.delayMs ?? 900;
  }, init);
  await page.goto(devPageUrl);
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  return { context, page, pageErrors };
}

const browser = await chromium.launch();

// ---------- separation + navigation ----------
{
  const { context, page, pageErrors } = await openPage(browser, { n: 8 });

  check(
    "old embedded cleanup panel is gone",
    (await page.$("#cleanup-block")) === null
  );
  check("ghost link visible in results view", await page.isVisible("#open-cleanup"));

  await page.click("#open-cleanup");
  await page.waitForSelector("#cleanup-view:not(.hidden)", { timeout: 15000 });
  const idlePill = await page.textContent("#cleanup-status-text");
  check("cleanup opens to idle state", idlePill.includes("ready when you are"), idlePill);
  await page.click("#cleanup-scan-btn");
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });

  const pillText = await page.textContent("#cleanup-status-text");
  check(
    "own status line shows disk-specific text",
    pillText.includes("Disk scan complete") && pillText.includes("37.3 GB"),
    pillText
  );
  const globalPill = await page.textContent("#status-text");
  check(
    "global pill untouched by disk mode",
    globalPill.includes("Scan complete"),
    globalPill
  );

  const cards = await page.$$eval("#cleanup-grid .cleanup-cat", (cards) => cards.length);
  check("four category cards in cleanup view", cards === 4, String(cards));
  const dlBoxes = await page.$$eval("#cleanup-dl-list input", (b) => b.length);
  check("downloads list present", dlBoxes === 3, String(dlBoxes));
  check("toss stage present in actions", await page.$("#toss-stage svg") !== null);

  // back navigation restores the security view + its pill state
  await page.click("#cleanup-back");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 15000 });
  const restoredPill = await page.textContent("#status-text");
  check("back restores security pill", restoredPill.includes("Scan complete"), restoredPill);
  check("cleanup view hidden after back", await page.isHidden("#cleanup-view"));

  // re-enter and run a cleanup with the toss mascot
  await page.click("#open-cleanup");
  await page.waitForSelector("#cleanup-idle:not(.hidden)", { timeout: 15000 });
  await page.click("#cleanup-scan-btn");
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  await page.check('#cleanup-dl-list input[data-path*="setup_toolkit"]');
  const btn = page.locator("#cleanup-btn");
  await btn.click();
  await btn.click();
  await page.waitForFunction(() => window.__cureTossSeen === true, null, { timeout: 5000 });
  const tossWasActive = await page.evaluate(
    () => window.__cureTossActive === true
  );
  check("toss mascot animated during run", tossWasActive);

  await page.waitForFunction(
    () => document.getElementById("cleanup-status").textContent.startsWith("Freed"),
    null,
    { timeout: 15000 }
  );
  const statusText = await page.textContent("#cleanup-status");
  check(
    "freed summary shown",
    statusText.startsWith("Freed 37.3 GB") && statusText.includes("deleted 4 of 4"),
    statusText
  );
  const diskPill = await page.textContent("#cleanup-status-text");
  check(
    "disk pill reflects result",
    diskPill.startsWith("Freed 37.3 GB"),
    diskPill
  );
  await page.waitForFunction(() => window.__cureTossActive === false, null, { timeout: 5000 });
  // post-run refresh keeps the result pill + status, numbers come back
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  check(
    "result pill survives refresh",
    (await page.textContent("#cleanup-status-text")).startsWith("Freed")
  );
  check(
    "result status survives refresh",
    (await page.textContent("#cleanup-status")).startsWith("Freed")
  );
  check("toss settled after result", true);
  check("no page errors", pageErrors.length === 0, pageErrors.join("; "));
  await context.close();
}

// ---------- failure path in new view ----------
{
  const { context, page, pageErrors } = await openPage(browser, {
    n: 8,
    cleanupFailures: true,
    delayMs: 600,
  });
  await page.click("#open-cleanup");
  await page.waitForSelector("#cleanup-idle:not(.hidden)", { timeout: 15000 });
  await page.click("#cleanup-scan-btn");
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  const btn = page.locator("#cleanup-btn");
  await btn.click();
  await btn.click();
  await page.waitForFunction(
    () => document.getElementById("cleanup-status").textContent.length > 0,
    null,
    { timeout: 15000 }
  );
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  check(
    "failure count surfaced",
    (await page.textContent("#cleanup-status")).includes("1 locked or failed")
  );
  check("failure list rendered", await page.isVisible("#cleanup-failures li"));
  const pill = await page.textContent("#cleanup-status-text");
  check("warn pill on failures", pill.includes("locked or failed"), pill);
  check("no page errors", pageErrors.length === 0, pageErrors.join("; "));
  await context.close();
}

// ---------- reduced motion: static end-state only ----------
{
  const { context, page, pageErrors } = await openPage(browser, {
    n: 20,
    reduced: true,
  });
  await page.waitForFunction(
    () => (window.__cureMascotCount || 0) > 0,
    null,
    { timeout: 30000 }
  );
  const active = await page.evaluate(() => window.__cureMascotActive === true);
  check("reduced motion: mascot count increments, never animates", !active);
  await page.click("#open-cleanup");
  await page.waitForSelector("#cleanup-idle:not(.hidden)", { timeout: 15000 });
  await page.click("#cleanup-scan-btn");
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  const btn = page.locator("#cleanup-btn");
  await btn.click();
  await btn.click();
  await page.waitForFunction(
    () => document.getElementById("cleanup-status").textContent.startsWith("Freed"),
    null,
    { timeout: 15000 }
  );
  const tossActive = await page.evaluate(() => window.__cureTossActive === true);
  check("reduced motion: toss loop never runs", !tossActive);
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  check(
    "reduced motion: mascot shown in static pose",
    await page.isVisible("#toss-stage svg")
  );
  check("no page errors", pageErrors.length === 0, pageErrors.join("; "));
  await context.close();
}

// ---------- manual launch: landing state, no auto-scan ----------
{
  const context = await browser.newContext({
    viewport: { width: 900, height: 600 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  await page.addInitScript(() => {
    window.__CURE_MOCK_ITEM_COUNT = 8;
  });
  await page.goto(devPageUrl);
  await page.waitForSelector("#landing-view:not(.hidden)", { timeout: 15000 });
  check("manual launch shows landing", await page.isVisible("#start-rescue-btn"));
  check("scan view stays hidden pre-consent", await page.isHidden("#scan-view"));
  await page.waitForTimeout(2500);
  const feedLines = await page.$$eval("#log li", (lis) => lis.length);
  check("no background scan ran while idle", feedLines === 0, String(feedLines));
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  check("start button triggers the security scan", true);
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "ALL CHECKS PASSED" : failures + " CHECK(S) FAILED");
process.exit(failures === 0 ? 0 : 1);
