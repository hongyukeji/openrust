import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
 for(const mobile of [false,true]) {
  const context=await browser.newContext({viewport:mobile?{width:375,height:812}:{width:1440,height:900},isMobile:mobile,hasTouch:mobile});
  const endpoint=process.argv[2],errors=[];
  assert.equal((await context.request.get(endpoint)).status(),403);
  assert.equal((await context.request.get(endpoint+'/telescope-api/monitored-tags')).status(),403);
  await context.setExtraHTTPHeaders({'x-test-operator':'operator'});
  const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
  const response=await page.goto(endpoint+'/events/example');
  assert.equal(response.status(),200);assert.equal(response.headers()['cache-control'],'no-store');
  assert(response.headers()['content-security-policy'].includes("frame-ancestors 'none'"));
  await page.locator('#entries button').first().waitFor();
  assert((await page.locator('#entries').innerText()).includes('<img src=x onerror=window.compromised=true>'));
  assert.equal(await page.evaluate(()=>window.compromised),undefined);
  await page.locator('#entries button').first().click();
  await page.waitForFunction(()=>document.querySelector('#json').textContent.includes('observed'));
  assert.equal(await page.locator('#entries img').count(),0);
  assert((await page.locator('meta[name="csrf-token"]').getAttribute('content')).length>0);
  assert.equal((await context.request.post(endpoint+'/telescope-api/toggle-recording',{data:{}})).status(),419);
  const tag='browser-'+(mobile?'mobile':'desktop');
  await page.fill('#monitor-tag',tag);await page.locator('#monitor-form button').click();
  await page.locator('#monitored button').filter({hasText:tag}).waitFor();
  await page.locator('#monitored button').filter({hasText:tag}).click();
  await page.waitForFunction(tag=>!document.querySelector('#monitored').textContent.includes(tag),tag);
  await page.locator('#recording').click();await page.waitForFunction(()=>document.querySelector('#status').textContent==='paused');
  await page.locator('#recording').click();await page.waitForFunction(()=>document.querySelector('#status').textContent==='enabled');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  assert.deepEqual(errors,[]);
  console.log(`PASS Telescope ${mobile?'mobile':'desktop'} authorization, CSRF, XSS, details, monitoring, pause/resume and layout`);
  await context.close();
 }
} finally { await browser.close(); }
