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
            for (const locale of ["en", "zh-Hans", "zh-Hant"]) {
                const page = await browser.newPage();
                try {
                    for (const [chapter, anchor, sample] of [
                        ["filesystem", "custom-filesystems", "FilesystemAdapter::custom"],
                        ["sms", "custom-sms-providers", "ExampleSmsProvider"],
                    ]) {
                        const response = await page.goto(`http://127.0.0.1:${server.address().port}${base}${locale}/digging-deeper/${chapter}#${anchor}`);
                        assert.equal(response.status(), 200);
                        await page.locator(`#${anchor}`).waitFor({ state: "visible" });
                        assert.ok((await page.locator(".vp-doc").innerText()).includes(sample));
                        assert.ok((await page.locator(".vp-doc").innerText()).includes("Worker"));
                        assert.equal(await page.locator(".vp-doc div[class*='language-rust']").count() > 0, true);
                        console.log(`${base}${locale}/${chapter}: 自定义驱动示例 PASS`);
                    }
                } finally {
                    await page.close();
                }
            }
        } finally {
            await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
        }
    }
} finally {
    await browser.close();
}
