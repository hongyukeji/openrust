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
            const base = `http://127.0.0.1:${server.address().port}${mount}`;
            const page = await browser.newPage();
            const errors = [];
            page.on("pageerror", error => errors.push(error.message));
            for (const language of ["en", "zh-Hans", "zh-Hant"]) {
                const response = await page.goto(`${base}${language}/digging-deeper/sms.html#sms-driver-lifecycle`);
                assert.equal(response.status(), 200);
                await page.locator("#sms-driver-lifecycle").waitFor();
                const body = await page.locator("main").innerText();
                for (const api of ["SmsManager", "forgetDrivers", "get_default_driver", "SmsSender", "SmsChannel::using", "to_sms", "api_key"]) {
                    assert(body.includes(api), `${language}: missing ${api}`);
                }
                assert(!body.includes("to_sms_template"));
                assert(!body.includes("to_sms_message"));
                assert(body.includes("SMS_VONAGE_API_KEY"));
                assert(!body.includes("config/vonage.rs"));
                assert.deepEqual(errors, []);
                console.log(`${mount} ${language}: SMS driver lifecycle and channel documentation PASS`);
            }
            await page.close();
        } finally {
            await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
        }
    }
} finally { await browser.close(); }
