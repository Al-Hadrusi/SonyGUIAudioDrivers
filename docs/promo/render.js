// Render the commercial to frames, then encode with ffmpeg.
// Run from the repository root:
//   node docs/promo/render.js
//   ffmpeg -framerate 30 -i docs/promo/frames/f%04d.jpg -c:v libx264 -preset slow -crf 18 \
//          -pix_fmt yuv420p -movflags +faststart headphones-control-commercial.mp4
// Music (original, synthesised; needs Python 3 + numpy), mixed in at -16 LUFS:
//   python3 docs/promo/music.py music.wav
//   ffmpeg -i headphones-control-commercial.mp4 -i music.wav -map 0:v -map 1:a -c:v copy \
//          -af loudnorm=I=-16:TP=-1.5 -c:a aac -b:a 192k -shortest final.mp4
// Needs Node.js with Playwright (npm install playwright && npx playwright install chromium).
const path = require('path');
const fs = require('fs');
const { chromium } = require('playwright');

(async () => {
  const fps = 30;
  const dir = __dirname;
  const out = path.join(dir, 'frames');
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  await page.goto('file://' + path.join(dir, 'promo.html'));
  await page.evaluate(() => document.fonts.ready);
  const total = Math.round((await page.evaluate(() => window.DURATION)) * fps);
  for (let f = 0; f < total; f++) {
    await page.evaluate(t => window.render(t), f / fps);
    await page.screenshot({ path: path.join(out, `f${String(f).padStart(4, '0')}.jpg`), type: 'jpeg', quality: 95 });
  }
  await browser.close();
  console.log(`${total} frames written to ${out}`);
})();
