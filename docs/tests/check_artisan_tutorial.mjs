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
                    const errors=[];page.on('pageerror',error=>errors.push(error.message));
                    const response=await page.goto(`http://127.0.0.1:${server.address().port}${base}${locale}/digging-deeper/artisan.html#command-signatures`);
                    assert.equal(response.status(),200);
                    await page.locator('#command-signatures').scrollIntoViewIfNeeded();
                    assert.equal(await page.locator('h2#command-signatures').count(),1);
                    assert.equal(await page.locator('h2#typed-command-input').count(),1);
                    assert.equal(await page.locator('h2#isolatable-commands').count(),1);
                    assert.equal(await page.locator('h2#editing-command-input').count(),1);
                    assert.equal(await page.locator('h2#prompting-for-missing-input').count(),1);
                    assert.equal(await page.locator('h2#command-lifecycle').count(),1);
                    assert.equal(await page.locator('h2#writing-command-output').count(),1);
                    assert.equal(await page.locator('h2#output-style-verbosity').count(),1);
                    const text=await page.locator('.vp-doc').innerText();
                    for (const snippet of ['command_with','from_signature','Arc<Mailer>','register_injected','{--Q|queue=default}','register_typed','integer_strict','input.input()','register_isolated','add_isolatable','register_edited','set_option','register_prompted','PromptsForMissingInput','CommandRegistration','register_counter','AtomicUsize','register_output','AsyncCommandCallback','output_components','register_styled','OutputFormat::Raw','OutputVerbosity::Quiet']) assert(text.includes(snippet),`${locale}: ${snippet}`);
                    assert.deepEqual(errors,[]);
                    console.log(`${base} ${locale} ${width}: formal signatures and injected example PASS`);
                    await page.close();
                }
            }
        } finally {await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
    }
} finally {await browser.close();}
