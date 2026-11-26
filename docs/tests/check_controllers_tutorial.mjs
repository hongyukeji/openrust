import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {serve} from 'vitepress';

const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const root=process.env.ARTISAN_DOCS_ROOT || fileURLToPath(new URL('../',import.meta.url));
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
    for (const base of ['/','/docs/']) {
        const {server}=await serve({root,base,port:0});
        try {
            for (const locale of ['en','zh-Hans','zh-Hant']) {
                for (const width of [375,1440]) {
                    const page=await browser.newPage({viewport:{width,height:900}});
                    const errors=[];
                    page.on('pageerror',error=>errors.push(error.message));
                    const response=await page.goto(
                        `http://127.0.0.1:${server.address().port}${base}${locale}/the-basics/controllers.html#generating-form-requests`
                    );
                    assert.equal(response.status(),200);
                    const model=page.locator('#specifying-the-resource-model');
                    const requests=page.locator('#generating-form-requests');
                    assert.equal(await model.count(),1);
                    assert.equal(await requests.count(),1);
                    await requests.scrollIntoViewIfNeeded();
                    const text=await page.locator('.vp-doc').innerText();
                    assert(text.includes('make:controller PhotoController --model=Photo --resource'));
                    assert(text.includes('StorePhotoRequest'));
                    assert(text.includes('UpdatePhotoRequest'));
                    assert(text.includes('ResourceController::action_with_form_request'));
                    assert.equal(await page.locator('#customizing-missing-model-behavior').count(),1);
                    assert.equal(await page.locator('#soft-deleted-models').count(),1);
                    assert(text.includes('controller.missing(missing_item)'));
                    assert(text.includes('with_trashed_for'));
                    assert.deepEqual(errors,[]);
                    console.log(`${base} ${locale} ${width}: controller generation PASS`);
                    await page.close();
                }
            }
        } finally {await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
    }
} finally {await browser.close();}
