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
  assert.equal(response.status(),200);assert(response.headers()['cache-control'].split(',').map(value=>value.trim()).includes('no-store'));
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
  const isDumpPoll=request=>new URL(request.url()).pathname.endsWith('/telescope-api/dumps')&&new URL(request.url()).searchParams.get('take')==='1';
  let dumpPolls=0;
  page.on('request',request=>{if(isDumpPoll(request))dumpPolls++});
  const [dumpIndex]=await Promise.all([
   page.waitForResponse(response=>response.url().endsWith('/telescope-api/dumps')&&response.ok()),
   page.getByRole('button',{name:'Dumps',exact:true}).click(),
  ]);
  assert.equal((await dumpIndex.json()).status,'enabled');
  const dumpLabel=mobile?'mobile-refresh':'desktop-refresh';
  assert.equal((await context.request.get(new URL('/api/dump/'+dumpLabel,endpoint).href)).status(),200);
  await page.locator('#entries pre').filter({hasText:dumpLabel}).waitFor();
  const dump=await page.locator('#entries pre').filter({hasText:dumpLabel}).textContent();
  assert(dump.includes('<img src=x onerror=window.dumpCompromised=true>'));
  assert.equal(await page.locator('#entries img').count(),0);
  assert.equal(await page.evaluate(()=>window.dumpCompromised),undefined);
  assert(dumpPolls>0);

  await page.route('**/telescope-api/dumps?take=1',route=>route.continue({
   headers:{...route.request().headers(),'x-test-operator':'guest'},
  }),{times:1});
  await page.waitForResponse(response=>isDumpPoll(response.request())&&response.status()===403);
  await page.locator('#notice').filter({hasText:'Access denied'}).waitFor();
  const stoppedAt=dumpPolls;
  await page.waitForTimeout(2200);
  assert.equal(dumpPolls,stoppedAt);
  await Promise.all([
   page.waitForResponse(response=>isDumpPoll(response.request())&&response.ok()),
   page.getByRole('button',{name:'Refresh',exact:true}).click(),
  ]);
  await page.getByRole('button',{name:'Events',exact:true}).click();
  const leftAt=dumpPolls;
  await page.waitForTimeout(2200);
  assert.equal(dumpPolls,leftAt);

  const firstEntry=page.locator('#entries button').first();
  await firstEntry.waitFor();
  const beforeManual=await firstEntry.getAttribute('aria-label');
  const emitEvent=()=>context.request.get(new URL('/api/probe',endpoint).href);
  assert.equal((await emitEvent()).status(),200);
  const newEntries=page.getByRole('button',{name:'Load new entries',exact:true});
  await newEntries.waitFor({timeout:8000});
  assert.equal(await firstEntry.getAttribute('aria-label'),beforeManual);
  await newEntries.click();
  await page.waitForFunction(id=>{const current=document.querySelector('#entries button')?.getAttribute('aria-label');return current&&current!==id},beforeManual);
  assert(!(await newEntries.isVisible()));
  await page.getByRole('button',{name:'Auto load entries',exact:true}).click();
  await page.reload();
  const autoLoad=page.getByRole('button',{name:'Auto load entries',exact:true});
  assert.equal(await autoLoad.getAttribute('aria-pressed'),'true');
  await firstEntry.waitFor();
  const beforeAutomatic=await firstEntry.getAttribute('aria-label');
  assert.equal((await emitEvent()).status(),200);
  await page.waitForFunction(id=>{const current=document.querySelector('#entries button')?.getAttribute('aria-label');return current&&current!==id},beforeAutomatic);
  assert(!(await newEntries.isVisible()));
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  assert.deepEqual(errors,[]);
  console.log(`PASS Telescope ${mobile?'mobile':'desktop'} authorization, CSRF, XSS, details, monitoring, pause/resume, live dumps, polling cleanup, new entries, auto-load preference and layout`);
  await context.close();
 }
} finally { await browser.close(); }
