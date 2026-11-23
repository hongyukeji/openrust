import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { serve } from 'vitepress';
const require = createRequire(import.meta.url);
const {chromium} = require(process.env.DOCS_PLAYWRIGHT_MODULE || 'playwright');
const root = fileURLToPath(new URL('../',import.meta.url));
const browser = await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
    for(const base of ['/', '/docs/']) {
        const {server} = await serve({root,base,port:0});
        try {
            for(const locale of ['en','zh-Hans','zh-Hant']) {
                const page = await browser.newPage();
                try {
                    const response = await page.goto(`http://127.0.0.1:${server.address().port}${base}${locale}/packages/precognition.html`);
                    assert.equal(response.status(),200);
                    const text = await page.locator('main').innerText();
                    for(const expected of ['Precognition-Validate-Only','204','422','validation_rules','passed_validation','validateFiles()','precognitive: false','middleware_name("precognitive")']) assert(text.includes(expected),expected);
                    for(const suffix of ['validation.html','middleware.html']) {
                        const href = await page.locator(`main a[href$="${suffix}"]`).first().getAttribute('href');
                        assert.equal((await page.request.get(new URL(href,page.url()).href)).status(),200);
                    }
                    assert(await page.locator('main').evaluate(el => el.scrollWidth <= el.clientWidth));
                    console.log(base,locale,'Precognition PASS');
                } finally { await page.close(); }
            }
        } finally { await promisify(server.close).call(server); }
    }
} finally { await browser.close(); }
