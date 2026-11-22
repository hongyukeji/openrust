import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { serve } from "vitepress";

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));
const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const base of ["/", "/docs/"]) {
        const { server } = await serve({ root, base, port: 0 });
        try {
            for (const language of ["en", "zh-Hans", "zh-Hant"]) {
                const page = await browser.newPage();
                await page.goto(`http://127.0.0.1:${server.address().port}${base}${language}/digging-deeper/artisan.html#complete-installation`);
                await page.locator("#complete-installation").waitFor();
                const text = await page.locator(".vp-doc").innerText();
                for (const expected of ["--extensions=all", "auth:sanctum", "views=false", "ResetPassword::createUrlUsing", "precognitive", "Passport", "OIDC"]) {
                    assert.ok(text.includes(expected), `${language}: missing ${expected}`);
                }
                assert.equal(await page.locator("#complete-installation").count(), 1);
                await page.close();
                console.log(`${base} ${language} init complete profile / prerequisites PASS`);
            }
        } finally {
            await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
        }
    }
} finally {
    await browser.close();
}
