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
                    const page = await browser.newPage({
                        viewport: mobile ? { width: 375, height: 812 } : { width: 1440, height: 900 },
                        isMobile: mobile,
                    });
                    const errors = [];
                    page.on("pageerror", error => errors.push(error.message));
                    const response = await page.goto(`http://127.0.0.1:${server.address().port}${mount}${language}/packages/settings.html`);
                    assert.equal(response.status(), 200);
                    await page.locator("main h1").waitFor();
                    const main = page.locator("main");
                    assert.equal(await main.locator("h2").count(), 4);
                    for (const word of ["SettingsGroup", "registerSettings", "saveIn", "reEncrypt", "__settings_version", "Conflict"]) {
                        assert((await main.innerText()).includes(word), `${language}: missing ${word}`);
                    }
                    assert(await page.locator(`a[href$="/packages/settings.html"]`).count() > 0, "missing settings navigation");
                    assert(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), "page overflows viewport");
                    assert.deepEqual(errors, []);
                    await page.close();
                    console.log(`${mount} ${language} ${mobile ? "mobile" : "desktop"}: settings content, navigation and layout PASS`);
                }
            }
        } finally {
            await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
        }
    }
} finally { await browser.close(); }
