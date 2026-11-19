import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { readFile } from 'node:fs/promises';
const root = process.env.OPENRUST_PRECOGNITION_BROWSER_ROOT;
assert(root, 'Set OPENRUST_PRECOGNITION_BROWSER_ROOT to the pinned npm fixture');
const { chromium } = await import(pathToFileURL(`${root}/node_modules/playwright/index.mjs`));
const { build } = await import(pathToFileURL(`${root}/node_modules/esbuild/lib/main.js`));
const pkg = JSON.parse(await readFile(`${root}/node_modules/laravel-precognition/package.json`, 'utf8'));
assert.equal(pkg.version, '2.0.0');
const bundle = await build({stdin:{contents:'import {client, createValidator} from "laravel-precognition"; window.precognition = {client, createValidator};', resolveDir:root},bundle:true,write:false,format:'iife',platform:'browser'});
const browser = await chromium.launch({headless:true,executablePath:process.env.DOCS_BROWSER_PATH});
try {
    const page = await browser.newPage();
    const base = process.argv[2];
    await page.goto(`${base}/api/closure`);
    await page.addScriptTag({content:bundle.outputFiles[0].text});
    const result = await page.evaluate(async () => {
        const {client, createValidator} = window.precognition;
        const events = [];
        await client.post('/api/form', {name:'Ada'}, {only:['name'],onPrecognitionSuccess:r => events.push(['success',r.status])});
        await client.post('/api/form', {name:''}, {only:['name'],onValidationError:r => events.push(['invalid',r.status,Object.keys(r.data.errors)])});
        await client.post('/api/form', {name:'Ada',confirmation:true}, {precognitive:false,onSuccess:r => events.push(['submit',r.status])});
        const file = new File(['hello'], 'example.txt', {type:'text/plain'});
        await client.post('/api/form', {attachment:file}, {only:['attachment'],headers:{'X-Upload':'yes'},onPrecognitionSuccess:r => events.push(['file',r.status])});
        await client.post('/api/form', {attachment:'text'}, {only:['attachment'],headers:{'X-Upload':'yes'},onValidationError:r => events.push(['not-file',r.status])});
        const run = (data, files) => new Promise((resolve, reject) => {
            const validator = createValidator(http => http.post('/api/form', data, {headers:{'X-Upload':'yes'}}), data).setTimeout(0);
            if(files) validator.validateFiles();
            const timer = setTimeout(() => reject(new Error('Validator did not settle')),5000);
            validator.on('validatingChanged', () => {
                if(!validator.validating()) { clearTimeout(timer); resolve({errors:validator.errors(),valid:validator.valid()}); }
            });
            validator.touch('attachment').validate();
        });
        const omitted = await run({attachment:file},false);
        const uploaded = await run({attachment:file},true);
        return {events,omitted,uploaded};
    });
    assert.deepEqual(result.events,[['success',204],['invalid',422,['name']],['submit',200],['file',204],['not-file',422]]);
    assert.deepEqual(result.omitted.errors, {attachment:['The attachment field is required.']});
    assert.deepEqual(result.uploaded.errors, {});
    assert(result.uploaded.valid.includes('attachment'));
    console.log(JSON.stringify({helper:pkg.version,chromium:browser.version(),...result}));
} finally { await browser.close(); }
