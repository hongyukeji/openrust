import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { serve } from "vitepress";
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));
const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const mount of ["/", "/docs/"]) {
        const { server } = await serve({ root, base: mount, port: 0 });
        try {
            for (const language of ["en", "zh_CN", "zh_TW"]) {
                for (const mobile of [false, true]) {
                for (const topic of ["billing", "cashier-paddle"]) {
                    const page = await browser.newPage({ viewport: mobile ? { width: 375, height: 812 } : { width: 1440, height: 900 }, isMobile: mobile, hasTouch: mobile });
                    const errors = [];
                    page.on("pageerror", error => errors.push(error.message));
                    const response = await page.goto(`http://127.0.0.1:${server.address().port}${mount}${language}/packages/${topic}.html#webhooks`);
                    assert.equal(response.status(), 200);
                    await page.locator("#webhooks").waitFor();
                    const body = await page.locator("main").innerText();
                    for (const text of ["openrust-cashier", "Billable", "BillingStore", "Cashier", "OutcomeUnknown", "EventConflict", "Stripe", "Paddle"]) {
                        assert(body.includes(text), `${language}: ${topic} missing ${text}`);
                    }
                    for (const anchor of topic === "billing" ? ["configuration", "customers-and-checkout", "subscriptions", "payments-and-invoices", "webhooks", "errors-and-api"] : ["configuration", "checkout", "subscriptions", "webhooks"]) {
                        assert.equal(await page.locator(`#${anchor}`).count(), 1);
                    }
                    assert.equal(await page.locator(`.VPSidebar a[href$="/packages/${topic}.html"]`).count(), 1);
                    assert.deepEqual(errors, []);
                    console.log(`${mount} ${language} ${mobile ? "mobile" : "desktop"}: ${topic} billing guide PASS`);
                    await page.close();
                }
                }
            }
        } finally { await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())); }
    }
} finally { await browser.close(); }
