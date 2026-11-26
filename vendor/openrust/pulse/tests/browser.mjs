import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const base=process.env.PULSE_TEST_URL;
assert(base,'PULSE_TEST_URL is required');
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try{
 const guest=await browser.newContext();
 for(const path of ['/operations/pulse','/operations/pulse/api/metrics']) assert.equal((await guest.request.get(base+path)).status(),403);
 await guest.close();
 const context=await browser.newContext({extraHTTPHeaders:{'x-test-operator':'operator'}});
 assert.equal((await context.request.get(base+'/api/probe')).status(),200);
 assert.equal((await context.request.get(base+'/operations/pulse/api/metrics?period=invalid')).status(),400);
 for(const mobile of [false,true]){
  const page=await context.newPage();await page.setViewportSize(mobile?{width:375,height:812}:{width:1440,height:1000});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  assert.equal((await page.goto(base+'/operations/pulse')).status(),200);
  await page.locator('#status').filter({hasText:'Updated'}).waitFor();
  assert.equal(await page.locator('.card').count(),10);
  assert.equal(await page.locator('progress').count(),5);
  assert.equal(await page.locator('svg').count(),3);
  for (const name of [/^CPU history for /, /^Memory history for /]) {
   const graph=page.getByRole('img',{name});
   assert(await graph.isVisible());
   assert.match(await graph.locator('path').getAttribute('d'), /^M [\d.]+ [\d.]+ h 0\.01$/);
  }
  assert(await page.getByText(/^Recently reported · Last reported /).isVisible());
  assert(await page.getByText(/^Stale · Last reported /).isVisible());
  assert.equal(await page.locator('svg polyline').count(),5);
  assert((await page.locator('#cards').innerText()).includes('ID: 42'));
  assert((await page.locator('#cards').innerText()).includes('<img src=x onerror=window.compromised=true>'));
  assert.equal(await page.locator('#cards img').count(),0);
  assert.equal(await page.evaluate(()=>window.compromised),undefined);
  const cache = page.locator('.card').filter({has: page.getByRole('heading',{name:'Cache interactions',exact:true})});
  assert.deepEqual(await cache.locator('.cache-summary dd').allTextContents(), ['~2', '~4', '33.33%']);
  assert.equal(await cache.locator('.cache-summary dd').first().getAttribute('title'), 'Sample rate: 0.5, raw count: 1');
  assert.equal(await cache.locator('td[data-column="hits"]').first().getAttribute('title'), 'Sample rate: 0.5, raw count: 1');
  assert.equal(await cache.locator('td[data-column="hit_rate"]').first().textContent(), '33.33%');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  for(const period of ['6h','24h','7d','1h']){
   const response=page.waitForResponse(r=>r.url().endsWith('/api/metrics?period='+period));
   await page.selectOption('#period',period);assert.equal((await response).status(),200);
  }
  const response=page.waitForResponse(r=>r.url().includes('/api/metrics?period=1h'));
  await page.click('#refresh');assert.equal((await response).status(),200);
  const slow = page.locator('.card').filter({has: page.getByRole('heading',{name:'Slow requests',exact:true})});
  assert((await slow.locator('tr').nth(1).innerText()).includes('window.compromised'));
  const filterResponse = page.waitForResponse(r => new URL(r.url()).searchParams.get('slow-requests') === 'count' && r.url().includes('/api/metrics'));
  await page.getByLabel('Order slow requests', {exact:true}).selectOption('count');
  assert.equal((await filterResponse).status(),200);
  await slow.locator('tr').nth(1).filter({hasText:'/frequent'}).waitFor();
  await page.getByLabel('Usage type', {exact:true}).selectOption('slow_requests');
  const usage = page.locator('.card').filter({has: page.getByRole('heading',{name:'Application usage',exact:true})});
  await usage.getByText('ID: 99',{exact:true}).waitFor();
  await page.reload();
  await slow.locator('tr').nth(1).filter({hasText:'/frequent'}).waitFor();
  await usage.getByText('ID: 99',{exact:true}).waitFor();
  await page.screenshot({path:process.env.PULSE_SCREENSHOT_DIR+`/dashboard-${mobile?'mobile':'desktop'}.png`,fullPage:true});
  assert.deepEqual(errors,[]);
  console.log(`native Pulse ${mobile?'mobile':'desktop'}: authorization, live metrics, 4 periods, SVG, meters, text escaping and refresh PASS`);
  await page.close();
 }
 await context.close();
}finally{await browser.close()}
