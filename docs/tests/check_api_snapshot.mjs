import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { serve } from "vitepress";
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));
const included = !process.argv.includes("--without-export");
const apiPath = new URL("../.vitepress/dist/api/build-info.json", import.meta.url);
assert.equal(existsSync(apiPath), included, "unexpected API export in the site");
const manifest = included ? JSON.parse(readFileSync(apiPath, "utf8")) : null;
const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const mount of ["/", "/docs/"]) {
        const { server } = await serve({ root, base: mount, port: 0 });
        try {
            const base = `http://127.0.0.1:${server.address().port}${mount}`;
            const page = await browser.newPage();
            page.setDefaultTimeout(10000);
            const errors = [], failures = [];
            page.on("pageerror", error => errors.push(error.message));
            page.on("response", response => { if (response.url().startsWith(base) && response.status() >= 400) failures.push(response.url()); });
            for (const mobile of [false, true]) {
                await page.setViewportSize(mobile ? { width: 375, height: 812 } : { width: 1440, height: 900 });
                for (const language of ["en", "zh_CN", "zh_TW"]) {
                    assert.equal((await page.goto(`${base}${language}/reference/api.html`)).status(), 200);
                    assert.match(await page.locator("main").innerText(), /build_api.py/);
                    if (mobile) await page.locator(".VPLocalNav button.menu").click();
                    const link = page.locator('.VPSidebar a').filter({ hasText: /^Rustdoc$/ });
                    assert.equal(await link.count(), included ? 1 : 0);
                    if (included) {
                        await link.click();
                        await page.waitForURL(`${base}api/index.html`);
                        assert.match(await page.locator("main").innerText(), new RegExp(manifest.revision));
                    }
                    console.log(`${mount} ${language} ${mobile ? "mobile" : "desktop"} API navigation PASS`);
                }
            }
            await page.setViewportSize({ width: 1440, height: 900 });
            if (included) {
                for (const crate of manifest.crates) {
                    assert.equal((await page.goto(`${base}api/${crate.index}`)).status(), 200);
                    await page.locator("#main-content").waitFor();
                    const source = page.locator("a.src").first();
                    assert.equal(await source.count(), 1);
                    await source.click();
                    await page.waitForURL(url => url.pathname.startsWith(`${mount}api/src/`));
                    assert(await page.locator("pre").count() > 0);
                }
                await page.goto(`${base}api/openrust/index.html?search=Application`);
                await page.waitForFunction(() => document.querySelector("#search")?.textContent.includes("Application"));
                const result = page.locator('#search a[href*="struct.Application"]').first();
                await result.click();
                await page.waitForURL(url => url.pathname.endsWith("struct.Application.html"));
                assert.match(await page.locator("#main-content").innerText(), /Application/);
            }
            assert.deepEqual(errors, []);
            assert.deepEqual(failures, []);
            console.log(`${mount}: three API guides; ${manifest?.crates.length || 0} crates, source links and search PASS`);
            await page.close();
        } finally { await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())); }
    }
} finally { await browser.close(); }
