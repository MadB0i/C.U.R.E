import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, "..", "dist");
const shotsDir = join(here, "..", "dev-screenshots");
const devPageUrl = pathToFileURL(join(distDir, "index.dev.html")).href;

mkdirSync(shotsDir, { recursive: true });

const errors = [];
function wireErrorCapture(page, label) {
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(`[${label}] console.error: ${msg.text()}`);
  });
  page.on("pageerror", (err) => {
    errors.push(`[${label}] pageerror: ${err.message}`);
  });
  page.on("requestfailed", (req) => {
    errors.push(`[${label}] requestfailed: ${req.url()} ${req.failure()?.errorText ?? ""}`);
  });
}

const shot = async (page, name) => {
  const target = join(shotsDir, name);
  await page.screenshot({ path: target });
  console.log("saved:", target);
};

async function runFlow(browser, label, reducedMotion) {
  const context = await browser.newContext({
    viewport: { width: 900, height: 600 },
    deviceScaleFactor: 1,
    reducedMotion: reducedMotion ? "reduce" : "no-preference",
  });
  const page = await context.newPage();
  wireErrorCapture(page, label);

  await page.goto(devPageUrl);
  await page.waitForSelector("#app:not(.hidden)", { state: "attached", timeout: 10000 });
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.click("#start-rescue-btn");

  const reduceActive = await page.evaluate(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
  if (reduceActive !== reducedMotion) {
    errors.push(`[${label}] expected reducedMotion=${reducedMotion}, page reports ${reduceActive}`);
  }

  if (!reducedMotion) {
    await page.waitForFunction(() => window.__cureMascotCount > 0, null, { timeout: 20000 });
    await page.waitForTimeout(420);
    const feedLines = await page.locator("#log li.item-line").count();
    if (feedLines < 1) {
      errors.push(`[${label}] no item-line entries in scan feed mid-scan`);
    }
    const nodeCount = await page.evaluate(() => window.__cureNodeCount || 0);
    if (nodeCount < 1) {
      errors.push(`[${label}] radar spawned no item nodes mid-scan`);
    }
    await shot(page, "01-midscan-radar.png");
  } else {
    await page.waitForFunction(
      () => document.querySelectorAll("#log li").length >= 6,
      null,
      { timeout: 20000 }
    );
    await page.waitForTimeout(300);
    await shot(page, "04-midscan-reduced-static.png");
  }

  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 20000 });
  await page.waitForSelector("#results-view .review-card", { state: "attached", timeout: 5000 });
  if (!reducedMotion) {
    const midStagger = await page.evaluate(
      () =>
        getComputedStyle(document.querySelector(".results-head")).opacity !== "1" ||
        document.querySelector(".review-card.reveal") !== null
    );
    console.log("stagger caught mid-flight:", midStagger ? "yes" : "(already settled)");
    await shot(page, "02-results-entrance.png");
  }

  await page.waitForTimeout(reducedMotion ? 500 : 1600);
  await shot(page, reducedMotion ? "06-results-reduced-motion.png" : "03-results-settled.png");

  // --- Scan Map panel: visible, labeled, and actually painted ---
  const mapOk = await page.evaluate(() => {
    const card = document.getElementById("map-card");
    const canvas = document.getElementById("map-canvas");
    const count = document.getElementById("map-count");
    if (!card || !canvas || !count) return { ok: false };
    const rect = canvas.getBoundingClientRect();
    let painted = -1;
    try {
      const d = canvas
        .getContext("2d")
        .getImageData(0, 0, canvas.width, canvas.height).data;
      painted = 0;
      for (let i = 3; i < d.length; i += 4) if (d[i] > 30) painted++;
    } catch (e) {
      /* leave -1 */
    }
    return {
      ok: true,
      hidden: card.classList.contains("hidden"),
      w: rect.width,
      h: rect.height,
      cw: canvas.width,
      ch: canvas.height,
      countText: count.textContent.trim(),
      painted,
    };
  });
  if (!mapOk.ok || mapOk.hidden || mapOk.w < 60 || mapOk.h < 60) {
    errors.push(`[${label}] scan map panel not visible: ${JSON.stringify(mapOk)}`);
  } else {
    if (!/^\d+ nodes$/.test(mapOk.countText)) {
      errors.push(`[${label}] map count text unexpected: "${mapOk.countText}"`);
    }
    if (!(mapOk.cw > 50 && mapOk.ch > 50)) {
      errors.push(`[${label}] map canvas not sized: ${mapOk.cw}x${mapOk.ch}`);
    }
    if (mapOk.painted <= 0) {
      errors.push(`[${label}] map canvas has no painted pixels`);
    }
    console.log(
      `scan map [${label}]: ${mapOk.countText}, canvas ${mapOk.cw}x${mapOk.ch}, paintedPx=${mapOk.painted}`
    );
  }

  const chipCount = await page.locator("#review-cards .chip").count();
  if (chipCount < 3) {
    errors.push(`[${label}] expected >=3 reason chips on review cards, found ${chipCount}`);
  }

  const btn = page.locator("#review-cards button.quarantine-btn").first();
  if ((await btn.count()) > 0 && (await btn.isVisible())) {
    await btn.click();
    await page.waitForTimeout(900);
    const label2 = (await btn.textContent()) ?? "";
    if (!/quarantined/i.test(label2)) {
      errors.push(`[${label}] quarantine button did not confirm (text: "${label2.trim()}")`);
    }
    if (!reducedMotion) {
      await shot(page, "04-after-quarantine.png");
    }
  } else {
    errors.push(`[${label}] no quarantine button found in review list`);
  }

  // --- Fix 1: footer buttons against mocked commands ---
  await page.evaluate(() => { window.__CURE_MOCK_FOOTER_ERRORS = false; });
  await page.click("#btn-quarantine-folder");
  await page.waitForFunction(
    () => {
      const el = document.getElementById("footbar-msg");
      return el.classList.contains("show") && !el.classList.contains("error");
    },
    null,
    { timeout: 4000 }
  );
  const qfText = (await page.textContent("#footbar-msg")) ?? "";
  if (!qfText.includes("opened")) {
    errors.push(`[${label}] quarantine-folder feedback unexpected: "${qfText.trim()}"`);
  }

  await page.click("#btn-view-log");
  await page.waitForFunction(
    () => document.getElementById("footbar-msg").textContent.includes("log opened"),
    null,
    { timeout: 4000 }
  );

  await page.evaluate(() => { window.__CURE_MOCK_FOOTER_ERRORS = true; });
  await page.click("#btn-view-log");
  await page.waitForFunction(
    () => {
      const el = document.getElementById("footbar-msg");
      return el.classList.contains("show") && el.classList.contains("error");
    },
    null,
    { timeout: 4000 }
  );
  const errText = ((await page.textContent("#footbar-msg")) ?? "").trim();
  if (!/^No /.test(errText)) {
    errors.push(`[${label}] expected "No ..." error message, got "${errText}"`);
  }
  if (!reducedMotion) {
    await shot(page, "09-footer-error.png");
  }

  await page.click("#btn-quarantine-folder");
  await page.waitForFunction(
    () => document.getElementById("footbar-msg").textContent.includes("quarantine folder yet"),
    null,
    { timeout: 4000 }
  );

  await page.evaluate(() => { window.__CURE_MOCK_FOOTER_ERRORS = false; });

  // exit must NOT close the dev harness page (mock resolves without quitting)
  await page.click("#btn-exit");
  await page.waitForTimeout(400);
  if (page.isClosed()) {
    errors.push(`[${label}] exit_app closed the mock harness page`);
  }

  await context.close();
}

async function timedScan(browser, label, itemCount) {
  const context = await browser.newContext({
    viewport: { width: 900, height: 600 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  wireErrorCapture(page, label);
  await page.addInitScript((count) => {
    window.__CURE_MOCK_ITEM_COUNT = count;
  }, itemCount);
  const t0 = Date.now();
  await page.goto(devPageUrl);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 45000 });
  const ms = Date.now() - t0;
  console.log(`pacing [${label}] items=${itemCount}: ${ms}ms end-to-end`);
  await context.close();
  return ms;
}

  const browser = await chromium.launch();
try {
  await runFlow(browser, "motion", false);
  await runFlow(browser, "reduced", true);

  const largeCtx = await browser.newContext({
    viewport: { width: 1400, height: 900 },
    deviceScaleFactor: 1,
  });
  const pageLg = await largeCtx.newPage();
  wireErrorCapture(pageLg, "large");
  await pageLg.goto(devPageUrl);
  await pageLg.waitForSelector("#app:not(.hidden)", { state: "attached", timeout: 10000 });
  await pageLg.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await pageLg.click("#start-rescue-btn");
  await pageLg.waitForFunction(() => window.__cureMascotCount > 0, null, { timeout: 20000 });
  await pageLg.waitForTimeout(420);
  await shot(pageLg, "07-midscan-1400x900.png");
  await pageLg.waitForSelector("#results-view:not(.hidden)", { timeout: 20000 });
  await pageLg.waitForTimeout(1600);
  await shot(pageLg, "08-results-1400x900.png");
  await largeCtx.close();

  // --- Fix 2: adaptive pacing sanity check (small vs large item counts) ---
  const smallMs = await timedScan(browser, "timing-small", 8);
  const largeMs = await timedScan(browser, "timing-large", 150);
  console.log(
    `pacing delta: large(150) - small(8) = ${largeMs - smallMs}ms`
  );
  if (smallMs > 12000) errors.push(`small-count scan too slow: ${smallMs}ms`);
  if (largeMs > 15000) errors.push(`large-count scan too slow: ${largeMs}ms`);
  // NOTE: the small->large delta is dominated by mock-only overhead the real
  // backend does not have: ~3s of fixed stage delays plus per-cleaning beats.
  // The real backend quarantines synchronously between emits, so its cost is
  // ~= N * per_item_ms (e.g. 150 items @ 33ms = ~5s, matching target_total).
  if (largeMs - smallMs > 8000) {
    errors.push(
      `large-count pacing blew past small by ${largeMs - smallMs}ms (clamp not working?)`
    );
  }
} finally {
  await browser.close();
}

if (errors.length > 0) {
  console.error("\nFAILURES:");
  for (const e of errors) console.error("  -", e);
  process.exit(1);
}
console.log("\nOK: all screenshots captured, no console/page errors.");
