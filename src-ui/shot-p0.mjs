import { chromium } from 'playwright-core';

const exe = process.env.HOME + '/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome';
const browser = await chromium.launch({ executablePath: exe, args: ['--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 }, deviceScaleFactor: 2, locale: 'zh-CN' });
await page.goto('http://localhost:1420/', { waitUntil: 'networkidle' });
await page.waitForTimeout(1200);
// 强制暗色主题（与 DBX 截图对齐）
await page.evaluate(() => { localStorage.setItem('auroradb-theme', 'dark'); document.documentElement.setAttribute('data-theme', 'dark'); });
await page.waitForTimeout(600);
await page.screenshot({ path: '/home/code/sqlcustomer/.screenshots/p0/welcome-dark.png' });
// 打开"新建查询"标签看主工作区
const btn = page.locator('.header-btn').filter({ hasText: /新建查询|New Query/i }).first();
if (await btn.count()) { await btn.click(); await page.waitForTimeout(900); }
await page.screenshot({ path: '/home/code/sqlcustomer/.screenshots/p0/query-dark.png' });
await browser.close();
console.log('done');
