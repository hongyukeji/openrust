import assert from "node:assert/strict";
import {createRequire} from "node:module";
import {fileURLToPath} from "node:url";
import {serve} from "vitepress";
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root=fileURLToPath(new URL("../",import.meta.url));
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
    for(const base of ["/","/docs/"]) {
        const {server}=await serve({root,base,port:0});
        try {
            for(const locale of ["en","zh-Hans","zh-Hant"]) {
                const page=await browser.newPage();
                try {
                    for(const [kind,type] of [["client","WebhookConfigRepository"],["server","SigningContext"]]) {
                        const response=await page.goto(`http://127.0.0.1:${server.address().port}${base}${locale}/packages/webhook-${kind}#signatures`);
                        assert.equal(response.status(),200);
                        await page.locator("#signatures").waitFor({state:"visible"});
                        for(const anchor of ["support-status","available-entry-points","scope"])
                            assert.equal(await page.locator(`h2#${anchor}`).count(),1);
                        assert.ok((await page.locator(".vp-doc").innerText()).includes(type));
                        if(kind==="client") {
                            assert.equal(await page.locator("h3#receipts").count(),1);
                            for(const entry of ["WebhookProcessor","DatabaseWebhookStore","WebhookJobDispatcher"])
                                assert.ok((await page.locator(".vp-doc").innerText()).includes(entry));
                        }
                        assert.ok((await page.locator(".vp-doc").innerText()).includes("vendor:publish"));
                        console.log(`${base}${locale}/webhook-${kind}: 范围与公开示例 PASS`);
                    }
                } finally {await page.close();}
            }
        } finally {await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
    }
} finally {await browser.close();}
