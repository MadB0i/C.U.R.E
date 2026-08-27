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
await page.waitForTimeout(1200);

// 1) LANDING
await page.screenshot({ path: 'shot-landing.png' });

// 2) RESCUE SCAN -> RESULTS
const scanBtn = await page.$('#start-rescue-btn');
await scanBtn.click();
await page.waitForTimeout(8000);
const dotAttr = await page.evaluate(() => document.getElementById('badge-dot').getAttribute('class'));
console.log('badge-dot class attr:', dotAttr);
await page.screenshot({ path: 'shot-results.png' });

// 3) CLEANUP
const openCleanup = await page.$('#open-cleanup');
if (openCleanup) await openCleanup.click();
await page.waitForTimeout(400);
await page.screenshot({ path: 'shot-cleanup-idle.png' });
const scanDisk = await page.$('#cleanup-scan-btn');
if (scanDisk) await scanDisk.click();
await page.waitForTimeout(2500);
await page.screenshot({ path: 'shot-cleanup-body.png' });

console.log('\n===== CAPTURED ERRORS =====');
if (errors.length === 0) {
  console.log('(none)');
} else {
  errors.forEach((e) => console.log(e));
}
await browser.close();