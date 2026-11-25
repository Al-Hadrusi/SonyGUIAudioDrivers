// Render thumbnail.html to thumbnail.png (1280x720). Run: node docs/promo/thumbnail.js
const path = require('path');
const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 });
  await page.goto('file://' + path.join(__dirname, 'thumbnail.html'));
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: path.join(__dirname, 'thumbnail.png') });
  await browser.close();
})();
