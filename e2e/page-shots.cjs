// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Screenshot every dashboard route in light, dark and mobile for visual review.
//
//   PAQTRA_DEV_API=https://<lab>:<api-nodeport> npx --prefix web-ui vite --port 3000 &
//   PAQTRA_USER=admin PAQTRA_PASS=<password> node e2e/page-shots.cjs [/route ...]
//
// Writes $PAQTRA_SHOTS_DIR (default /tmp/paqtra-shots)/<route>-<variant>.png plus a -top.png
// crop, and fails on page errors or sideways scroll. Read-only: it only navigates and scrolls,
// never clicks controls.
const { chromium } = require('playwright');
const fs = require('node:fs');
const path = require('node:path');

const appSrc = fs.readFileSync(path.join(__dirname, '../web-ui/src/App.tsx'), 'utf8');
const ALL = [...appSrc.matchAll(/<Route path="(\/[^"]*)"/g)].map((m) => m[1]);
const routes = process.argv.slice(2).length ? process.argv.slice(2) : ALL;
const base = process.env.PAQTRA_WEB_URL || 'http://127.0.0.1:3000';
const out = process.env.PAQTRA_SHOTS_DIR || '/tmp/paqtra-shots';
const variants = (process.env.PAQTRA_SHOTS_VARIANTS || 'light,dark,mobile').split(',');
const settle = Number(process.env.PAQTRA_SHOTS_SETTLE_MS || 5000);
const slug = (r) => (r === '/' ? 'overview' : r.slice(1).replace(/\//g, '-'));

async function login(ctx) {
  if (process.env.PAQTRA_TOKEN) return process.env.PAQTRA_TOKEN;
  if (!process.env.PAQTRA_PASS) return '';
  const res = await ctx.request.post(`${base}/api/v1/auth/login`, {
    data: { username: process.env.PAQTRA_USER || 'admin', password: process.env.PAQTRA_PASS },
  });
  if (!res.ok()) throw new Error(`login failed: ${res.status()}`);
  return (await res.json()).token;
}

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, executablePath: process.env.PAQTRA_CHROMIUM_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const probe = await browser.newContext({ ignoreHTTPSErrors: true });
  const token = await login(probe);
  await probe.close();
  const failures = [];
  for (const route of routes) {
    for (const v of variants) {
      const mobile = v === 'mobile';
      const ctx = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, ignoreHTTPSErrors: true });
      const p = await ctx.newPage();
      const errors = [];
      p.on('pageerror', (e) => errors.push(e.message));
      await p.addInitScript(([t, theme]) => {
        if (t) {
          localStorage.setItem('paqtra-token', t);
          localStorage.setItem('paqtra-username', 'admin');
        }
        localStorage.setItem('paqtra-theme', theme);
      }, [token, v === 'dark' ? 'dark' : 'light']);
      await p.goto(`${base}${route}`);
      await p.waitForTimeout(settle);
      const height = await p.evaluate(() => document.documentElement.scrollHeight);
      for (let y = 0; y < height; y += 500) {
        await p.mouse.wheel(0, 500);
        await p.waitForTimeout(60);
      }
      await p.waitForTimeout(600);
      await p.evaluate(() => window.scrollTo(0, 0));
      const file = `${out}/${slug(route)}-${v}.png`;
      await p.screenshot({ path: file, fullPage: true });
      const vp = p.viewportSize();
      await p.screenshot({ path: file.replace(/\.png$/, '-top.png'), fullPage: true, clip: { x: 0, y: 0, width: vp.width, height: Math.min(height, mobile ? 1700 : 1400) } });
      const fits = await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth);
      if (!fits) failures.push(`${route}/${v}: scrolls sideways`);
      if (errors.length) failures.push(`${route}/${v}: ${errors.join('; ')}`);
      console.log(`${fits && !errors.length ? 'ok  ' : 'FAIL'} ${file}`);
      await ctx.close();
    }
  }
  await browser.close();
  if (failures.length) {
    console.error(failures.join('\n'));
    process.exit(1);
  }
})();
