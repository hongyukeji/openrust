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
  for (const [name, value, unit, samples] of [[/^CPU history for /, 25, '%', 2], [/^Memory history for /, 2048, ' MiB', 1]]) {
   const graph=page.getByRole('img',{name});
   assert(await graph.isVisible());
   const path=await graph.locator('path').getAttribute('d');
   assert.equal(path.match(/M [\d.]+ [\d.]+ h 0\.01/g).length,samples);
   assert.equal(await graph.locator('rect').count(),samples);
   const target=graph.locator('rect').last();
   await target.hover();
   const tooltip=await target.locator('title').textContent();
   const measurement=await page.evaluate(value=>value.toLocaleString(),value);
   assert(tooltip.endsWith(' · '+measurement+unit));
   assert.match(tooltip.split(' · ')[0], /\d+:\d+:\d+/);
  }
  assert(await page.getByText(/^Recently reported · Last reported /).isVisible());
  assert(await page.getByText(/^Stale · Last reported /).isVisible());
  for (const [name, percentage] of [['Memory · 2GB / 8GB', '25'], ['/ · 100MB / 200MB', '50']]) {
   const meter=page.getByRole('progressbar',{name});
   assert(await meter.isVisible());
   assert.equal(await meter.getAttribute('value'),percentage);
  }
  const queue = page.getByRole('img',{name:'default queue activity',exact:true});
  assert.equal(await queue.locator('path[data-queue-series]').count(),5);
  const peak = page.locator('.queue-peak');
  assert.equal(await peak.textContent(), 'Peak: ~8');
  assert.equal(await peak.getAttribute('title'), 'Sample rate: 0.4, Raw value: 3');
  const queueReading=page.locator('.queue-reading');
  const sampledBucket=queue.locator('rect').filter({hasText:/Queued: ~7\.5/});
  assert.equal(await sampledBucket.count(),1);
  await queue.focus();
  await queue.press('Home');
  assert.match(await queueReading.textContent(),/Queued: ~/);
  await queue.press('Escape');
  assert(await queueReading.isHidden());

  assert((await page.locator('#cards').innerText()).includes('ID: 42'));
  assert((await page.locator('#cards').innerText()).includes('<img src=x onerror=window.compromised=true>'));
  assert.equal(await page.locator('#cards img').count(),0);
  assert.equal(await page.evaluate(()=>window.compromised),undefined);
  const cache = page.locator('.card').filter({has: page.getByRole('heading',{name:'Cache interactions',exact:true})});
  assert.deepEqual(await cache.locator('.cache-summary dd').allTextContents(), ['~2', '~4', '33.33%']);
  assert.equal(await cache.locator('.cache-summary dd').first().getAttribute('title'), 'Sample rate: 0.5, raw count: 1');
  assert.equal(await cache.locator('td[data-column="hits"]').first().getAttribute('title'), 'Sample rate: 0.5, raw count: 1');
  assert.equal(await cache.locator('td[data-column="hit_rate"]').first().textContent(), '33.33%');
  for (const [name,value,rate] of [
   ['Application usage','~3',0.4], ['Slow requests','~4',0.25], ['Slow queries','~2',0.5],
   ['Slow jobs','~5',0.2], ['Slow outgoing requests','~8',0.125], ['Exceptions','~3',0.4],
  ]) {
   const table=page.locator('.card').filter({has:page.getByRole('heading',{name,exact:true})});
   const count=table.locator('td[data-column="count"]').first();
   assert.equal(await count.textContent(), value);
   assert.equal(await count.getAttribute('title'), `Sample rate: ${rate}, Raw value: 1`);
  }
  const jobs=page.locator('.card').filter({has:page.getByRole('heading',{name:'Application usage · Jobs',exact:true})});
  const jobCount=jobs.locator('td[data-column="count"]').first();
  assert.equal(await jobCount.textContent(),'1');
  assert.equal(await jobCount.getAttribute('title'),null);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  for(const [period,link] of [['6h','6_hours'],['24h','24_hours'],['7d','7_days'],['1h',null]]){
   const response=page.waitForResponse(r=>r.url().endsWith('/api/metrics?period='+period));
   await page.selectOption('#period',period);assert.equal((await response).status(),200);
   assert.equal(new URL(page.url()).searchParams.get('period'),link);
  }
  const back=page.waitForResponse(r=>r.url().endsWith('/api/metrics?period=7d'));
  await page.goBack();assert.equal((await back).status(),200);
  assert.equal(await page.locator('#period').inputValue(),'7d');
  const forward=page.waitForResponse(r=>r.url().endsWith('/api/metrics?period=1h'));
  await page.goForward();assert.equal((await forward).status(),200);
  assert.equal(await page.locator('#period').inputValue(),'1h');
  const response=page.waitForResponse(r=>r.url().includes('/api/metrics?period=1h'));
  await page.click('#refresh');assert.equal((await response).status(),200);
  const slow = page.locator('.card').filter({has: page.getByRole('heading',{name:'Slow requests',exact:true})});
  assert((await slow.locator('tr').nth(1).innerText()).includes('window.compromised'));
  const filterResponse = page.waitForResponse(r => new URL(r.url()).searchParams.get('slow-requests') === 'count' && r.url().includes('/api/metrics'));
  await page.getByLabel('Order slow requests', {exact:true}).selectOption('count');
  assert.equal((await filterResponse).status(),200);
  await slow.locator('tr').nth(1).filter({hasText:'/frequent'}).waitFor();
  assert.equal(await slow.locator('td[data-column="count"]').first().textContent(),'~8');
  await page.getByLabel('Usage type', {exact:true}).selectOption('slow_requests');
  const usage = page.locator('.card').filter({has: page.getByRole('heading',{name:'Application usage',exact:true})});
  await usage.getByText('ID: 99',{exact:true}).waitFor();
  assert.equal(await usage.locator('td[data-column="count"]').first().textContent(),'~4');
  await page.reload();
  await slow.locator('tr').nth(1).filter({hasText:'/frequent'}).waitFor();
  await usage.getByText('ID: 99',{exact:true}).waitFor();
  await page.screenshot({path:process.env.PULSE_SCREENSHOT_DIR+`/dashboard-${mobile?'mobile':'desktop'}.png`,fullPage:true});
  for (const [link,value] of [['1_hour','1h'],['6_hours','6h'],['24_hours','24h'],['7_days','7d']]) {
   assert.equal((await context.request.get(base+'/operations/pulse/api/metrics?period='+link)).status(),200);
   const query=page.waitForResponse(r=>r.url().endsWith('/api/metrics?period='+value));
   assert.equal((await page.goto(base+'/operations/pulse?period='+link)).status(),200);
   assert.equal((await query).status(),200);
   assert.equal(await page.locator('#period').inputValue(),value);
  }
  assert.deepEqual(errors,[]);
  console.log(`native Pulse ${mobile?'mobile':'desktop'}: authorization, live metrics, 4 periods, SVG, meters, text escaping and refresh PASS`);
  await page.close();
 }
 const polling=await context.newPage();
 const time=new Date('2026-11-26T00:00:00Z');
 await polling.clock.install({time});
 await polling.clock.pauseAt(new Date(time.getTime()+1000));
 let polls=0;
 polling.on('request', r=>{if(r.url().includes('/api/metrics')) polls++});
 assert.equal((await polling.goto(base+'/operations/pulse')).status(),200);
 await polling.locator('#status').filter({hasText:'Updated'}).waitFor();
 assert.equal(polls,1);
 const graph=polling.getByRole('img',{name:/^CPU history for /});
 const reading=polling.locator('[data-server-trend="cpu:acceptance"] + output');
 const oldGraph=await graph.elementHandle();
 await graph.focus();
 assert.match(await reading.textContent(), /25%$/);
 await graph.press('ArrowLeft');
 assert.match(await reading.textContent(), /13%$/);
 await polling.clock.runFor(4999);
 assert.equal(polls,1);
 const automatic=polling.waitForResponse(r=>r.url().includes('/api/metrics'));
 await polling.clock.runFor(1);
 assert.equal((await automatic).status(),200);
 assert.equal(polls,2);
 await polling.waitForFunction(element=>!element.isConnected,oldGraph);
 assert(await graph.evaluate(element=>element===document.activeElement));
 assert.match(await reading.textContent(), /13%$/);
 await graph.press('End');
 assert.match(await reading.textContent(), /25%$/);
 await graph.press('Escape');
 assert(await reading.isHidden());
 console.log('native Pulse dashboard: initial request and five-second automatic refresh PASS');
 console.log('native Pulse dashboard: keyboard explanation and focus survive refresh PASS');
 await polling.close();
 await context.close();

 const touch=await browser.newContext({hasTouch:true,isMobile:true,viewport:{width:375,height:812},
  extraHTTPHeaders:{'x-test-operator':'operator'}});
 const phone=await touch.newPage();
 assert.equal((await phone.goto(base+'/operations/pulse')).status(),200);
 const touchGraph=phone.getByRole('img',{name:/^CPU history for /});
 const touchReading=phone.locator('[data-server-trend="cpu:acceptance"] + output');
 await touchGraph.locator('rect').first().tap();
 assert(await touchReading.isVisible());
 assert.match(await touchReading.textContent(), /13%$/);
 await touchGraph.locator('rect').last().tap();
 assert.match(await touchReading.textContent(), /25%$/);
 const touchQueue=phone.getByRole('img',{name:'default queue activity',exact:true});
 const queueReading=phone.locator('.queue-reading');
 await touchQueue.locator('rect').filter({hasText:/Queued: ~7\.5/}).tap();
 assert.match(await queueReading.textContent(),/Queued: ~7\.5/);
 console.log('native Pulse dashboard: taps read individual samples PASS');
 await touch.close();
}finally{await browser.close()}
