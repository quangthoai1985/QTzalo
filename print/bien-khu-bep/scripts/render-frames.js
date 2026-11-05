const { chromium } = require('playwright');
(async () => {
  const [dir, out] = process.argv.slice(2);
  const b = await chromium.launch(); const p = await b.newPage({ viewport: { width: 1587, height: 1123 } });
  await p.goto('file://' + dir + '/signs.html'); await p.waitForLoadState('load');
  await p.pdf({ path: out, width: '420mm', height: '297mm', printBackground: true, preferCSSPageSize: true });
  const els = await p.$$('.p'); for (let i = 0; i < els.length; i++) await els[i].screenshot({ path: `${dir}/shot-${i + 1}.png` });
  await b.close();
})();
