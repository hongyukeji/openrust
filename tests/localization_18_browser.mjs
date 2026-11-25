// 十八语静态交付、语言偏好与布局验收；原生译文质量需另行审校。
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {createRequire} from 'node:module';
const root=process.cwd();
const stage=path.resolve(process.env.LOCALE_ACCEPTANCE_DIR);
assert(stage.startsWith(path.join(root,'target/tmp/')));
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const deployment=path.join(stage,'deployment');fs.mkdirSync(deployment,{recursive:true});
for(const dir of ['resources','lang','public'])fs.cpSync(path.join(root,dir),path.join(deployment,dir),{recursive:true});
fs.copyFileSync(process.env.LOCALE_BINARY,path.join(deployment,'main'));
fs.mkdirSync(path.join(deployment,'storage'),{recursive:true});fs.mkdirSync(path.join(stage,'tmp'),{recursive:true});
const probe=net.createServer();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));
const log=fs.openSync(path.join(stage,'browser-server.log'),'w');
const child=spawn(path.join(deployment,'main'),[],{cwd:deployment,env:{PATH:'',TMPDIR:path.join(stage,'tmp'),APP_BASE_PATH:deployment,APP_ENV:'production',APP_DEBUG:'false',APP_NAME:'OpenRust',APP_SUPPORTED_LOCALES:'*',APP_KEY:'base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=',APP_HOST:'127.0.0.1',APP_PORT:String(port),APP_DISABLED_RUNNERS:'queue,schedule,reverb,grpc',SESSION_DRIVER:'array',CACHE_STORE:'array',LOG_CHANNEL:'null'},stdio:['ignore',log,log]});
const locales=['en','zh-Hans','zh-Hant','ja','ko','fr','de','es','pt','it','ru','ar','hi','id','vi','th','tr','nl'];
let browser;const results=[];
try {
 const base=`http://127.0.0.1:${port}`;
 for(let i=0;;i++) {try {assert.equal((await fetch(base+'/up')).status,200);break;}catch(e){if(i>150||child.exitCode!==null)throw e;await new Promise(r=>setTimeout(r,100));}}
 browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH,headless:true});
 for(const locale of locales)for(const mobile of [false,true])for(const colorScheme of ['light','dark']) {
  const context=await browser.newContext({locale,colorScheme,viewport:mobile?{width:390,height:844}:{width:1440,height:900},isMobile:mobile,hasTouch:mobile});
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  const response=await page.goto(base);assert.equal(response.status(),200);assert.equal(response.headers()['content-language'],locale);
  assert.equal(await page.locator('html').getAttribute('lang'),locale);
  assert.equal(await page.locator('html').getAttribute('dir'),locale==='ar'?'rtl':null);
  const welcome=JSON.parse(fs.readFileSync(path.join(root,`vendor/openrust/framework/src/localization/lang/${locale}/welcome.json`),'utf8'));
  assert.equal(await page.locator('h1').innerText(),welcome.heading);
  const docs=['en','zh-Hans','zh-Hant'].includes(locale)?locale:'en';assert((await page.locator('.steps a').first().getAttribute('href')).includes(`/docs/${docs}/README.md`));
  const summary=page.locator('.language-switcher').locator('..').locator('summary');
  if(mobile)await summary.tap();else {await summary.focus();await page.keyboard.press('Enter');}
  for(const choice of locales){const name=JSON.parse(fs.readFileSync(path.join(root,`vendor/openrust/framework/src/localization/lang/${choice}/languages.json`),'utf8')).name;assert.equal(await page.locator(`.language-switcher button[value="${choice}"]`).innerText(),name);}
  assert.equal(await page.locator('.language-switcher').evaluate(e=>getComputedStyle(e).overflowY),'auto');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  const selected=locale==='ja'?'fr':'ja';
  await Promise.all([page.waitForNavigation(),(mobile?page.locator(`.language-switcher button[value="${selected}"]`).tap():page.locator(`.language-switcher button[value="${selected}"]`).press('Enter'))]);
  assert.equal(await page.locator('html').getAttribute('lang'),selected);assert.equal(await page.locator('.language-switcher').getAttribute('data-mode'),'fixed');
  await page.goto(base);assert.equal(await page.locator('html').getAttribute('lang'),selected);
  await page.locator('.language-switcher').locator('..').locator('summary').click();
  await Promise.all([page.waitForNavigation(),page.locator('.language-switcher button[value=""]').click()]);
  assert.equal(await page.locator('html').getAttribute('lang'),locale);assert.equal(await page.locator('.language-switcher').getAttribute('data-mode'),'auto');
  assert.deepEqual(errors,[]);if(mobile&&colorScheme==='light')await page.screenshot({path:path.join(stage,`${locale}-mobile.png`),fullPage:true});
  results.push({locale,mobile,colorScheme,passed:true});console.log(locale,mobile?'mobile':'desktop',colorScheme,'PASS');await context.close();
 }
 fs.writeFileSync(path.join(stage,'browser-results.json'),JSON.stringify(results,null,2));
}finally{if(browser)await browser.close();child.kill('SIGINT');await new Promise(r=>child.once('exit',r));fs.closeSync(log);}
