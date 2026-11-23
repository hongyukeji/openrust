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
                const response = await page.goto(`${base}${language}/digging-deeper/localization.html#database-translation-loader`);
                assert.equal(response.status(), 200);
                await page.locator("#database-translation-loader").waitFor();
                const body = await page.locator("main").innerText();
                for (const api of ["openrust-translation-database", "withTable", "withMaxRows", "withFallback", "load_database_translations", "Lang::load_from", "PostgreSQL 18", "128"]) {
                    assert(body.includes(api), `${language}: missing ${api}`);
                }
                assert.deepEqual(errors, []);
                console.log(`${mount} ${language}: database translation schema, source and limits PASS`);
            }
            await page.close();
        } finally {
            await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
        }
    }
} finally { await browser.close(); }
