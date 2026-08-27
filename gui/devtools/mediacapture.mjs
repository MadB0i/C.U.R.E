import { chromium } from 'playwright';
import fs from 'node:fs';

const htmlPath = 'file:///D:/Projects/CURE/gui/dist/index.dev.html';
const OUT = 'C:/Users/rupjy/AppData/Local/Temp/opencode/cure-media';
fs.mkdirSync(OUT, { recursive: true });
const frameDir = OUT + '/frames';
fs.mkdirSync(frameDir, { recursive: true });

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 600 } });

const mockSrc = fs.readFileSync('D:/Projects/CURE/gui/dist/mock-tauri.js', 'utf8');
await page.addInitScript(mockSrc);
await page.addInitScript(() => { window.__CURE_MOCK_ITEM_COUNT = 40; });

await page.goto(htmlPath, { waitUntil: 'networkidle' });
await page.waitForTimeout(1500);

// 1) SOCIAL PREVIEW (1280x640)
const wide = await browser.newPage({ viewport: { width: 1280, height: 640 } });
await wide.addInitScript(mockSrc);
await wide.addInitScript(() => { window.__CURE_MOCK_ITEM_COUNT = 40; });
await wide.goto(htmlPath, { waitUntil: 'networkidle' });
await wide.waitForTimeout(1500);
await wide.screenshot({ path: OUT + '/social-preview.png' });
console.log('social-preview.png saved');
await wide.close();

// 2) DEMO GIF FRAMES — scan animation
await page.click('#start-rescue-btn');
await page.waitForTimeout(250);
const totalMs = 6500;
const rate = 15; // fps
const count = Math.floor((totalMs / 1000) * rate);
for (let i = 0; i < count; i++) {
  const n = String(i).padStart(3, '0');
  await page.screenshot({ path: `${frameDir}/f${n}.png` });
  await page.waitForTimeout(1000 / rate);
}
console.log(`${count} frames captured`);
await browser.close();