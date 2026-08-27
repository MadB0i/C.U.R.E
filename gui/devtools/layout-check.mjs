import { chromium } from 'playwright';

const htmlPath = 'file:///D:/Projects/CURE/gui/dist/index.dev.html';
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 600 } });

const errors = [];
page.on('console', (msg) => { if (msg.type() === 'error') errors.push(msg.text()); });
page.on('pageerror', (err) => errors.push(err.message));

const fs = await import('node:fs');
const mockSrc = fs.readFileSync('D:/Projects/CURE/gui/dist/mock-tauri.js', 'utf8');
await page.addInitScript(mockSrc);
await page.addInitScript(() => { window.__CURE_MOCK_ITEM_COUNT = 6; });
await page.goto(htmlPath, { waitUntil: 'networkidle' });
await page.waitForTimeout(1000);

// Results view layout checks
await page.click('#start-rescue-btn');
await page.waitForTimeout(8000);

const results = await page.evaluate(() => {
  const q = (s) => document.querySelector(s);
  const r = (el) => el ? { x: el.getBoundingClientRect().x, y: el.getBoundingClientRect().y, w: el.getBoundingClientRect().width, h: el.getBoundingClientRect().height } : null;
  const head = q('.results-head');
  const rak = q('.rakshak-sm');
  const hw = q('.headline-wrap');
  const stats = q('.stats');
  const mapCard = q('#map-card');
  const resultsMain = q('#results-main') || document.querySelector('.results-main');
  const dot = q('#badge-dot');
  const dotStroke = dot ? getComputedStyle(dot).fill : null;
  const scroller = q('#results-view .results-main');
  return {
    viewHidden: q('#results-view').classList.contains('hidden'),
    resultsMain: r(resultsMain || q('.results-main')),
    head: r(head),
    headPos: head ? getComputedStyle(head).position : null,
    rakshak: r(rak),
    headlineWrap: r(hw),
    stats: r(stats),
    mapCard: r(mapCard),
    mapVisible: mapCard ? !mapCard.classList.contains('hidden') : null,
    dotFill: dotStroke,
    stackGap: getComputedStyle(q('.results-stack')).gap,
    resultsMainOverflowY: scroller ? getComputedStyle(scroller).overflowY : null,
    bodyScrollHeight: document.body.scrollHeight,
  };
});
console.log('RESULTS LAYOUT:', JSON.stringify(results, null, 2));

// Check for element overlaps: rakshak vs headline
const overlap = await page.evaluate(() => {
  const rak = document.querySelector('.rakshak-sm').getBoundingClientRect();
  const hw = document.querySelector('.headline-wrap').getBoundingClientRect();
  const ox = Math.max(0, Math.min(rak.right, hw.right) - Math.max(rak.left, hw.left));
  const oy = Math.max(0, Math.min(rak.bottom, hw.bottom) - Math.max(rak.top, hw.top));
  return { overlapPx: ox * oy, rak: {l: rak.left, t: rak.top, r: rak.right, b: rak.bottom}, hw: {l: hw.left, t: hw.top, r: hw.right, b: hw.bottom} };
});
console.log('OVERLAP:', JSON.stringify(overlap, null, 2));

console.log('\nERRORS:', errors.length ? errors.join('\n') : '(none)');
await browser.close();