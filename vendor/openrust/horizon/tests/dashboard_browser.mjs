import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const mobile of [false, true]) {
        const context = await browser.newContext({ viewport: mobile ? { width: 375, height: 812 } : { width: 1440, height: 900 }, isMobile: mobile, hasTouch: mobile });
        const page = await context.newPage();
        assert.equal((await context.request.get(process.argv[2])).status(), 403);
        await context.setExtraHTTPHeaders({ 'x-test-operator': 'operator' });
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        const response = await page.goto(process.argv[2]);
        assert.equal(response.status(), 200);
        assert.equal(response.headers()['cache-control'], 'no-store');
        await page.locator('#queues tr').first().waitFor();
        assert((await page.locator('#status').innerText()).includes('Updated'));
        assert.equal(await page.locator('#queues tr').count(), 1);
        assert.equal(await page.locator('#jobs tr').count(), 1);
        assert(!(await page.locator('body').innerText()).includes('secret-never-exported'));
        assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
        await page.selectOption('#job-kind', 'completed');
        await page.locator('#job-list button').first().waitFor();
        await page.locator('#job-list button').first().click();
        await page.waitForFunction(() => document.querySelector('#job-detail').textContent.includes('secret-never-exported'));
        const tag='browser-probe-'+(mobile?'mobile':'desktop');
        await page.fill('#monitor-tag', tag);
        await page.locator('#monitor-form button').click();
        await page.waitForFunction(tag => document.querySelector('#monitoring').textContent.includes(tag),tag);
        await page.locator('#monitoring li').filter({hasText:tag}).getByRole('button',{name:'Stop monitoring'}).click();
        await page.waitForFunction(tag => !document.querySelector('#monitoring').textContent.includes(tag),tag);
        await page.locator('#load-metrics').click();
        await page.locator('#metric-list button').first().waitFor();
        await page.locator('#metric-list button').first().click();
        await page.waitForFunction(() => document.querySelector('#metric-detail').textContent.length>0);
        assert.equal(await page.evaluate(async () => (await fetch(location.pathname+'/api/pause',{method:'POST'})).status),403);
        assert.equal(await page.evaluate(async () => (await fetch(location.pathname+'/api/workload')).status),200);
        assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));

        // The refresh interval must work under the delivered nonce CSP.
        const first = await page.locator('#status').innerText();
        await page.waitForFunction(first => document.querySelector('#status').textContent !== first, first, { timeout: 8000 });
        assert.deepEqual(errors, []);
        console.log(`PASS: Horizon ${mobile ? 'mobile' : 'desktop'} authorization, job details, monitoring mutations, metrics, CSP refresh and layout`);
        await context.close();
    }
} finally { await browser.close(); }
