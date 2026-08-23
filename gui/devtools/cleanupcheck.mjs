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

async function openCleanupPage(browser, init = {}) {
  const context = await browser.newContext({
    viewport: { width: 900, height: 600 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));
  await page.addInitScript((cfg) => {
    window.__CURE_MOCK_ITEM_COUNT = cfg.n ?? 8;
    window.__CURE_MOCK_CLEANUP_FAILURES = !!cfg.cleanupFailures;
  }, init);
  await page.goto(devPageUrl);
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  return { context, page, pageErrors };
}

const browser = await chromium.launch();

// ---------- happy path ----------
{
  const { context, page, pageErrors } = await openCleanupPage(browser);

  check(
    "panel headline",
    (await page.textContent("#cleanup-block h3")).trim() === "DISK CLEANUP"
  );

  // mock totals: cats 6871947674 + 283115776 + 0 + 32851861504,
  // downloads 48923136 + 12582912 + 2097152 -> 40070538154 B = "37.3 GB"
  const totalText = await page.textContent("#cleanup-total");
  check("total line", totalText.includes("37.3 GB"), totalText);
  check("total item count", totalText.includes("1248 items"), totalText);

  const cards = await page.$$eval("#cleanup-grid .cleanup-cat", (cards) =>
    cards.map((c) => ({
      key: c.dataset.key,
      disabled: c.disabled,
      on: c.classList.contains("on"),
      text: c.textContent,
    }))
  );
  check("four category cards", cards.length === 4, JSON.stringify(cards.map((c) => c.key)));
  check(
    "recycle bin empty+disabled",
    cards.find((c) => c.key === "recycle_bin")?.disabled === true
  );
  check(
    "populated cats default on",
    ["temp", "browser_cache", "windows_old"].every(
      (k) => {
        const c = cards.find((x) => x.key === k);
        return c && !c.disabled && c.on;
      }
    )
  );
  const tempCard = cards.find((c) => c.key === "temp");
  check("temp size label", tempCard.text.includes("6.4 GB"), tempCard.text);
  const oldCard = cards.find((c) => c.key === "windows_old");
  check("windows.old size label", oldCard.text.includes("30.6 GB"), oldCard.text);
  const cacheCard = cards.find((c) => c.key === "browser_cache");
  check("cache size label", cacheCard.text.includes("270.0 MB"), cacheCard.text);

  // downloads list
  const dlBoxes = await page.$$eval("#cleanup-dl-list input[type=checkbox]", (boxes) =>
    boxes.map((b) => ({ path: b.dataset.path, checked: b.checked }))
  );
  check("three download installers listed", dlBoxes.length === 3, String(dlBoxes.length));
  check("downloads unchecked by default", dlBoxes.every((b) => !b.checked));

  const btn = page.locator("#cleanup-btn");
  check(
    "clean button enabled (populated cats default on)",
    !(await btn.isDisabled())
  );

  // panel uses the graphite panel token
  const panelBg = await page.$eval("#cleanup-block", (el) =>
    getComputedStyle(el).backgroundColor
  );
  check("panel background token", panelBg === "rgb(19, 19, 21)", panelBg);

  // toggle a category off and back on; selection drives the button
  await page.click('.cleanup-cat[data-key="temp"]');
  let btnLabel = await btn.textContent();
  check("button enabled with remaining cats", !(await btn.isDisabled()));
  await page.click('.cleanup-cat[data-key="browser_cache"]');
  await page.click('.cleanup-cat[data-key="windows_old"]');
  check("button disabled when nothing selected", await btn.isDisabled());
  for (const key of ["temp", "browser_cache", "windows_old"]) {
    await page.click(`.cleanup-cat[data-key="${key}"]`);
  }
  btnLabel = await btn.textContent();
  check("button label reset", btnLabel.trim() === "Clean up", btnLabel);

  // tick one download -> arm flow
  await page.check('#cleanup-dl-list input[data-path*="setup_toolkit"]');
  check("button enabled after download tick", !(await btn.isDisabled()));

  await btn.click();
  btnLabel = (await btn.textContent()).trim();
  check("arm prompt appears", /^Really free /.test(btnLabel), btnLabel);
  const armedBg = await btn.evaluate((el) => getComputedStyle(el).backgroundColor);
  check("armed style is danger red", armedBg === "rgb(225, 89, 79)", armedBg);

  await btn.click(); // confirm
  await page.waitForFunction(
    () => document.getElementById("cleanup-status").textContent.length > 0,
    null,
    { timeout: 10000 }
  );
  const call = await page.evaluate(() => window.__CURE_LAST_CLEANUP_CALL);
  const expectedCats = ["browser_cache", "temp", "windows_old"];
  const dlSelection =
    call.download_paths || call.downloadPaths || [];
  check(
    "run_cleanup got selected categories only",
    JSON.stringify([...(call.categories || [])].sort()) === JSON.stringify(expectedCats),
    JSON.stringify(call)
  );
  check(
    "run_cleanup got exactly the ticked download",
    dlSelection.length === 1 && dlSelection[0].includes("setup_toolkit"),
    JSON.stringify(dlSelection)
  );

  const statusText = await page.textContent("#cleanup-status");
  check(
    "freed summary shown",
    statusText.startsWith("Freed 37.3 GB") && statusText.includes("deleted 4 of 4"),
    statusText
  );

  // auto-refresh brings fresh numbers back without wiping the result line
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  check(
    "result survives refresh",
    (await page.textContent("#cleanup-status")).startsWith("Freed")
  );
  const failuresHiddenAfterRun = await page.isHidden("#cleanup-failures");
  check("no failure list on clean run", failuresHiddenAfterRun);

  await page.screenshot({ path: join(shotsDir, "cleanup-900.png") });

  check("no page errors (happy path)", pageErrors.length === 0, pageErrors.join("; "));
  await context.close();
}

// ---------- locked-file failure path ----------
{
  const { context, page, pageErrors } = await openCleanupPage(browser, {
    cleanupFailures: true,
  });
  const btn = page.locator("#cleanup-btn");
  await btn.click();
  await btn.click();
  await page.waitForFunction(
    () => document.getElementById("cleanup-status").textContent.length > 0,
    null,
    { timeout: 10000 }
  );
  const statusText = await page.textContent("#cleanup-status");
  check(
    "failure count surfaced",
    statusText.includes("1 locked or failed"),
    statusText
  );
  // the post-run refresh briefly hides the panel body, then restores it with
  // fresh numbers AND the preserved result/failure details
  await page.waitForSelector("#cleanup-body:not(.hidden)", { timeout: 15000 });
  const failuresVisible = await page.isVisible("#cleanup-failures li");
  check("failure list rendered", failuresVisible);
  if (failuresVisible) {
    const failText = await page.textContent("#cleanup-failures li");
    check(
      "locked-file reason quoted",
      failText.includes("os error 32"),
      failText
    );
  }
  check(
    "failure list survives refresh",
    await page.isVisible("#cleanup-failures li")
  );
  check("no page errors (failure path)", pageErrors.length === 0, pageErrors.join("; "));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "ALL CHECKS PASSED" : failures + " CHECK(S) FAILED");
process.exit(failures === 0 ? 0 : 1);
