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
                    const url = `http://127.0.0.1:${server.address().port}${mount}${locale}/digging-deeper/queues.html`;
                    const response = await page.goto(url);
                    assert.equal(response.status(), 200);
                    for (const anchor of ["queue-workers-and-deployment", "worker-status-and-exit-reasons", "pausing-and-resuming-queue-workers", "pausing-queues-in-code"]) {
                        assert.equal(await page.locator(`#${anchor}`).count(), 1);
                        assert.ok(await page.locator(`a[href="#${anchor}"]`).count());
                    }
                    const body = await page.locator("main").innerText();
                    for (const command of ["queue:restart", "queue:work --json --stop-when-empty", "queue:pause database:default", "queue:continue database:default", "queue:pause --all", "queue:resume --all"]) {
                        assert.ok(body.includes(`./artisan ${command}`), `${locale}: ${command}`);
                    }
                    assert.ok(body.includes("Queue::pauseFor"));
                    assert.ok(body.includes("WorkerQueueResumed"));
                    assert.ok(body.includes("restart_signal"));
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
