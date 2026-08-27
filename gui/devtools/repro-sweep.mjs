import { chromium } from 'playwright';

const htmlPath = 'file:///D:/Projects/CURE/gui/dist/index.dev.html';
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 600 } });

const errors = [];
const warns = [];
page.on('console', (msg) => {
  if (msg.type() === 'error') errors.push(msg.text());
  if (msg.type() === 'warning') warns.push(msg.text());
});
page.on('pageerror', (err) => errors.push('PAGEERROR: ' + err.message + ' @ ' + (err.stack || '').split('\n')[1] || ''));

const fs = await import('node:fs');
const mockSrc = fs.readFileSync('D:/Projects/CURE/gui/dist/mock-tauri.js', 'utf8');
await page.addInitScript(mockSrc);
await page.addInitScript(() => {
  window.__CURE_MOCK_ITEM_COUNT = 8;
  window.__CURE_MOCK_SWEEP = true; // add process + ransom findings
});
await page.goto(htmlPath, { waitUntil: 'networkidle' });
await page.waitForTimeout(1000);

// Run rescue scan with sweep findings
await page.click('#start-rescue-btn');
await page.waitForTimeout(10000);

const state = await page.evaluate(() => {
  const v = document.getElementById('results-view');
  return {
    visible: !v.classList.contains('hidden'),
    headline: document.getElementById('headline').textContent,
    reviewCards: document.querySelectorAll('#review-cards .review-card').length,
    cleanedCards: document.querySelectorAll('#cleaned-cards .review-card').length,
    procBlockHidden: document.getElementById('process-block').classList.contains('hidden'),
    procCards: document.querySelectorAll('#process-block .review-card').length,
    ransomBlockHidden: document.getElementById('ransom-block').classList.contains('hidden'),
    ransomCards: document.querySelectorAll('#ransom-block .entry-card').length,
    badgeDot: document.getElementById('badge-dot').getAttribute('class'),
    pill: (document.getElementById('status-pill').className || ''),
    pillText: document.getElementById('status-text').textContent,
    rakshakStatusText: document.getElementById('rakshak-status').textContent,
  };
});
console.log('SWEEP RESULTS:', JSON.stringify(state, null, 2));

// Now run kill-process flow
if (state.procCards > 0) {
  await page.evaluate(() => {
    const search = (el) => {
      if (el.classList && el.classList.contains('kill-btn')) return el;
      for (const c of el.children) { const r = search(c); if (r) return r; }
      return null;
    };
    const btn = document.querySelector('#process-block .kill-btn, #kill-procs-btn');
    return btn ? btn.click() : null;
  });
  await page.waitForTimeout(4000);
  const afterKill = await page.evaluate(() => ({
    killed: document.querySelectorAll('.proc-killed').length,
    pillNow: document.getElementById('status-text').textContent,
  }));
  console.log('AFTER KILL:', JSON.stringify(afterKill));
}

// cleanup flow with sweep active
await page.click('#open-cleanup');
await page.waitForTimeout(400);
await page.click('#cleanup-scan-btn');
await page.waitForTimeout(2500);
const cleanup = await page.evaluate(() => ({
  bodyVisible: !document.getElementById('cleanup-body').classList.contains('hidden'),
  cats: document.querySelectorAll('.cleanup-cat').length,
}));
console.log('CLEANUP:', JSON.stringify(cleanup));

console.log('\n===== ERRORS =====');
console.log(errors.length ? errors.join('\n----\n') : '(none)');
console.log('\n===== WARNINGS =====');
console.log(warns.length ? warns.join('\n----\n') : '(none)');
await browser.close();