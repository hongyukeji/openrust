import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {serve} from 'vitepress';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
 for(const base of ['/', '/docs/']) {
  const {server}=await serve({root:fileURLToPath(new URL('../',import.meta.url)),base,port:0});
  try {
   for(const lang of ['en','zh_CN','zh_TW']) {
    const page=await browser.newPage();
    const errors=[];
    page.on('pageerror',error=>errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}${base}${lang}/packages/fortify.html`);
    const article=await page.locator('.vp-doc').innerText();
    for(const term of ['Fortify 1.43.0','API-F01','API-F02','fortify:install','FortifyResourcesServiceProvider','fortify-options.two-factor-authentication','RateLimiter::forAsync','updateFromRequest','current_password:web','Password::default()','password.reset','/two-factor-challenge','/reset-password','/email/verify/{id}/{hash}','confirmed-password-status']) assert(article.includes(term),`${lang}: missing ${term}`);
    assert.equal(await page.locator('.vp-doc h2').count(),11);
    for(const href of await page.locator('.vp-doc a[href]').evaluateAll(nodes=>nodes.map(n=>n.getAttribute('href')))) {
      if(href.startsWith('#') || /^(https?:|mailto:)/.test(href)) continue;
      const target=new URL(href,page.url());
      assert(target.pathname.startsWith(base+lang+'/'),`${lang}: wrong base ${href}`);
      const response=await page.request.get(target.href);
      assert.equal(response.status(),200,`${lang}: broken ${href}`);
    }
    assert.deepEqual(errors,[]);
    console.log(base,lang,'Fortify content/navigation PASS');
    await page.close();
   }
  } finally {await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()));}
 }
} finally {await browser.close();}
