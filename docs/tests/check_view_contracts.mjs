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
            for (const locale of ["en", "zh_CN", "zh_TW"]) {
                const page = await browser.newPage();
                try {
                    const url = `http://127.0.0.1:${server.address().port}${mount}${locale}/the-basics/views.html`;
                    const response = await page.goto(url);
                    assert.equal(response.status(), 200);
                    assert.equal(await page.locator("#view-contracts").count(), 1);
                    assert.ok(await page.locator('a[href="#view-contracts"]').count());
                    const body = await page.locator("main").innerText();
                    assert.ok(body.includes("render_card"));
                    assert.ok(body.includes("addNamespace"));
                    assert.equal(await page.locator("#native-view-values").count(), 1);
                    assert.ok(body.includes("ViewValue::Object(data)"));
                    assert.ok(body.includes("value.to_json().is_err()"));
                    assert.equal((await page.goto(url.replace("views.html", "blade.html"))).status(), 200);
                    const blade = await page.locator("main").innerText();
                    assert.ok(blade.includes("-INF") && blade.includes("NAN"));
                    await page.locator('main a[href$="views.html#native-view-values"]').first().click();
                    await page.waitForURL(url + "#native-view-values");
                    await page.locator("#native-view-values").waitFor();
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
