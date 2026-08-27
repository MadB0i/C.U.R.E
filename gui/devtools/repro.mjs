import { chromium } from 'playwright';
import { fileURLToPath } from 'url';
import path from 'path';

const htmlPath = 'file:///D:/Projects/CURE/gui/dist/index.dev.html';

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 600 } });

const errors = [];
page.on('console', (msg) => {
  if (msg.type() === 'error' || msg.type() === 'warning') {
    errors.push(`[console.${msg.type()}] ${msg.text()}`);
  }
});
page.on('pageerror', (err) => {
  errors.push(`[pageerror] ${err.message}\n${err.stack || ''}`);
});

const fs = await import('node:fs');
const mockSrc = fs.readFileSync('D:/Projects/CURE/gui/dist/mock-tauri.js', 'utf8');
await page.addInitScript(mockSrc);
await page.addInitScript(() => {
  window.__CURE_MOCK_ITEM_COUNT = 6;
});

await page.goto(htmlPath, { waitUntil: 'networkidle' });
await page.waitForTimeout(800);

const appHidden = await page.evaluate(() => document.getElementById('app').classList.contains('hidden'));
console.log('APP hidden?', appHidden);

// Click Rescue Scan
const scanBtn = await page.$('#start-rescue-btn, #start-scan-btn');
if (scanBtn) {
  console.log('Clicking scan button...');
  await scanBtn.click();
  await page.waitForTimeout(8000);
}

const resultsHidden = await page.evaluate(() => document.getElementById('results-view').classList.contains('hidden'));
console.log('RESULTS visible?', !resultsHidden);

// Capture what's on the results page
const resultsState = await page.evaluate(() => {
  const headline = document.getElementById('headline');
  const dot = document.getElementById('badge-dot');
  return {
    headline: headline ? headline.textContent : null,
    badgeDotClassAttr: dot ? dot.getAttribute('class') : null,
    badgeClassName: document.getElementById('badge') ? document.getElementById('badge').className : null,
  };
});
console.log('RESULTS state:', JSON.stringify(resultsState, null, 2));
await page.screenshot({ path: 'shot-results.png', fullPage: true });

// cleanup screenshot
await page.evaluate(() => document.getElementById('cleanup-back').click());
await page.waitForTimeout(300);
await page.screenshot({ path: 'shot-cleanup-idle.png', fullPage: true });

// trigger cleanup body
const scanDisk = await page.$('#cleanup-scan-btn');
if (scanDisk) await scanDisk.click();
await page.waitForTimeout(2500);
await page.screenshot({ path: 'shot-cleanup-body.png', fullPage: true });

// landing screenshot
await page.evaluate(() => document.getElementById('cleanup-back').click());
await page.waitForTimeout(300);
await page.evaluate(() => document.getElementById('landing-btn').click());
await page.waitForTimeout(400);
await page.screenshot({ path: 'shot-landing.png', fullPage: true });

// Now test cleanup flow
console.log('--- Testing cleanup flow ---');
const openCleanup = await page.$('#open-cleanup, #cleanup-link');
if (openCleanup) {
  await openCleanup.click();
  await page.waitForTimeout(500);
  const scanDisk = await page.$('#cleanup-scan-btn');
  if (scanDisk) {
    console.log('Clicking Scan disk...');
    await scanDisk.click();
    await page.waitForTimeout(3000);
    console.log('CLEANUP body visible?', !(await page.evaluate(() => document.getElementById('cleanup-body').classList.contains('hidden'))));
  }
}

console.log('\n===== CAPTURED ERRORS =====');
if (errors.length === 0) {
  console.log('(none)');
} else {
  errors.forEach((e) => console.log(e));
}

await browser.close();