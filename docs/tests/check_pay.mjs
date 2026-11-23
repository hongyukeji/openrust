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
            for (const language of ["en", "zh-Hans", "zh-Hant"]) {
                for (const mobile of [false, true]) {
                    const page = await browser.newPage({ viewport: mobile ? { width: 375, height: 812 } : { width: 1440, height: 900 }, isMobile: mobile, hasTouch: mobile });
                    const errors = [];
                    page.on("pageerror", error => errors.push(error.message));
                    const response = await page.goto(`http://127.0.0.1:${server.address().port}${mount}${language}/packages/pay.html#callbacks`);
                    assert.equal(response.status(), 200);
                    await page.locator("#callbacks").waitFor();
                    const body = await page.locator("main").innerText();
                    for (const text of ["PayServiceProvider", "PAY_WECHAT_PUBLIC_KEY_ID", "PAY_ALIPAY_SELLER_ID", "Amount::from_fen", "DatabaseNotifications", "NotificationConflict", "OutcomeUnknown", "forget_drivers", "get_default_driver", "Scene::Scan", "Stripe", "Paddle"]) {
                        assert(body.includes(text), `${language}: missing ${text}`);
                    }
                    for (const anchor of ["configuration", "create-payment", "query-and-refund", "callbacks", "gateway-lifecycle"]) {
                        assert.equal(await page.locator(`#${anchor}`).count(), 1);
                    }
                    assert.equal(await page.locator('.VPSidebar a[href$="/packages/pay.html"]').count(), 1);
                    assert.deepEqual(errors, []);
                    console.log(`${mount} ${language} ${mobile ? "mobile" : "desktop"}: payment guide PASS`);
                    await page.close();
                }
            }
        } finally { await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())); }
    }
} finally { await browser.close(); }
