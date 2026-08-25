import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, "..", "dist");
const shotsDir = join(here, "..", "dev-screenshots");
const devPageUrl = pathToFileURL(join(distDir, "index.dev.html")).href;

mkdirSync(shotsDir, { recursive: true });

const TARGET_CHIPS = [
  "Invalid Signature",
  "Known Malware Hash",
  "Valid Signature",
  "Unsigned Binary",
];

async function checkChips(viewport, items) {
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 2,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  await page.addInitScript((cfg) => {
    window.__CURE_MOCK_ITEM_COUNT = cfg.n;
    window.__CURE_MOCK_ALL_SAFE = false;
  }, { n: items });
  await page.goto(devPageUrl);
  await page.waitForSelector("#start-rescue-btn", { timeout: 15000 });
  await page.click("#start-rescue-btn");
  await page.waitForSelector("#results-view:not(.hidden)", { timeout: 60000 });
  await page.waitForTimeout(1900);

  const report = await page.evaluate((targets) => {
    const out = { found: {}, problems: [], allChips: [] };
    // flagged high-risk entries can be auto-quarantined into the cleaned
    // block, so scan every review-card in the document, not just one list
    const cards = [...document.querySelectorAll(".review-card")];
    for (const card of cards) {
      const cardBox = card.getBoundingClientRect();
      for (const chip of card.querySelectorAll(".chip")) {
        const text = chip.textContent.trim();
        const cs = getComputedStyle(chip);
        const box = chip.getBoundingClientRect();
        const rec = {
          text,
          tone: chip.className.replace("chip", "").trim() || "(none)",
          w: Math.round(box.width),
          h: Math.round(box.height),
          clippedText: chip.scrollWidth > chip.clientWidth + 1,
          // note: cards live in a scrollable list, so being below the fold
          // is fine — the real failure modes are clipped glyphs and chips
          // escaping their card's box
          insideCard:
            box.left >= cardBox.left - 1 &&
            box.right <= cardBox.right + 1 &&
            box.top >= cardBox.top - 1 &&
            box.bottom <= cardBox.bottom + 1,
          uppercased: cs.textTransform === "uppercase",
        };
        out.allChips.push(rec);
        const hit = targets.find(
          (t) => text.toUpperCase() === t.toUpperCase()
        );
        if (hit && !out.found[hit]) {
          out.found[hit] = { ...rec };
        }
        if (rec.clippedText)
          out.problems.push(`text clipped in chip "${text}"`);
        if (!rec.insideCard)
          out.problems.push(`chip "${text}" overflows its review card`);
      }
    }
    return out;
  }, TARGET_CHIPS);

  // element-level zoom crops of the cards carrying the new engine chips
  for (const t of ["Invalid Signature", "Known Malware Hash"]) {
    const card = page
      .locator(".review-card", { has: page.locator(".chip", { hasText: t }) })
      .first();
    if ((await card.count()) > 0) {
      await card.scrollIntoViewIfNeeded();
      await page.waitForTimeout(300);
      await card.screenshot({
        path: join(
          shotsDir,
          `chips-${viewport.width}-${t.toLowerCase().replace(/\s+/g, "-")}.png`
        ),
      });
    }
  }

  console.log(`\n=== ${viewport.width}x${viewport.height} (items=${items}) ===`);
  for (const t of TARGET_CHIPS) {
    const f = report.found[t];
    if (f) {
      console.log(
        `FOUND "${f.text}" tone=${f.tone} ${f.w}x${f.h}px uppercase=${f.uppercased}`
      );
    } else {
      console.log(`MISSING "${t}" (not present in this fixture run)`);
    }
  }
  const distinct = [...new Set(report.allChips.map((c) => c.text))];
  console.log("all distinct chips:", JSON.stringify(distinct));
  if (report.problems.length === 0) {
    console.log("OK: no clipping / no overflow on any chip");
  } else {
    for (const p of report.problems) console.log("PROBLEM:", p);
  }

  await context.close();
  return report.problems.length === 0;
}

const browser = await chromium.launch();
const ok900 = await checkChips({ width: 900, height: 600 }, 11);
const ok1920 = await checkChips({ width: 1920, height: 1080 }, 14);
await browser.close();

if (!ok900 || !ok1920) {
  console.error("FAIL");
  process.exit(1);
}
console.log("\nPASS at both sizes");
