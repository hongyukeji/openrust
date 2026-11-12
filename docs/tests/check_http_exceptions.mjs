import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { serve } from "vitepress";

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));
const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const mount of ["/", "/docs/"]) {
        const { server } = await serve({ root, base: mount, port: 0 });
        try {
            for (const locale of ["en", "zh-hans", "zh-hant"]) {
                const page = await browser.newPage();
                try {
                    const url = `http://127.0.0.1:${server.address().port}${mount}${locale}/the-basics/errors.html`;
                    const response = await page.goto(url);
                    assert.equal(response.status(), 200);
                    assert.equal(await page.locator("#http-exceptions").count(), 1);
                    assert.ok(await page.locator('a[href="#http-exceptions"]').count());
                    const body = await page.locator("main").innerText();
                    assert.ok(body.includes("Err(abort"));
                    assert.ok(body.includes("retry-after"));
                    console.log(mount, locale, "PASS");
                } finally {
                    await page.close();
                }
            }
        } finally {
            await promisify(server.close).call(server);
        }
    }
} finally {
    await browser.close();
}
