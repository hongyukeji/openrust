import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE || 'playwright');
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
  const page=await browser.newPage();const messages=[],errors=[];
  page.on('console',message=>messages.push({type:message.type(),text:message.text()}));
  page.on('pageerror',error=>errors.push(error.message));
  await page.setContent(await readFile(process.argv[2],'utf8'));
  assert.deepEqual(errors,[]);assert.equal(await page.evaluate(()=>globalThis.injected),undefined);
  assert(messages.some(message=>message.text.includes('stream-15')));
  assert(messages.some(message=>message.text.includes('request-15')));
  assert(messages.some(message=>message.type==='warning'));
  assert(messages.some(message=>message.type==='startGroupCollapsed'));
  assert(messages.some(message=>message.text.includes('Context')));
  assert(!messages.some(message=>message.text.includes('request-14')));
  assert(process.argv[3], '需要实际定制处理器响应以验证CSS参数');
  const styled=await browser.newPage(),argumentsList=[];
  styled.on('console',message=>argumentsList.push(Promise.all(message.args().map(value=>value.jsonValue()))));
  await styled.setContent(await readFile(process.argv[3],'utf8'));
  const values=(await Promise.all(argumentsList)).flat();
  assert(values.some(value=>value==='color:blue'));
  assert(values.some(value=>typeof value==='string' && value.includes('typed public handler')));
  console.log('实际HTTP响应：浏览器控制台、CSS标记、上下文字段、级别和脚本转义通过。');
} finally {await browser.close();}
