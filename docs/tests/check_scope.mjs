import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {serve} from 'vitepress';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const root=fileURLToPath(new URL('../',import.meta.url));
const paths=[...readFileSync(new URL('../en/README.md',import.meta.url),'utf8').matchAll(/\]\(([^)]+\.md)\)/g)].map(m=>m[1]);
const expectedSupportPages=new Set(paths.filter(file=>readFileSync(new URL('../en/'+file,import.meta.url),'utf8').includes('{#support-status}'))).size;
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try{
 for(const base of ['/','/docs/']){
  const {server}=await serve({root,base,port:0});
  try{
   for(const language of ['en','zh-Hans','zh-Hant']){
    const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));let count=0;
    for(const file of new Set(paths)){
     const url=`http://127.0.0.1:${server.address().port}${base}${language}/${file==='index.md'?'':file.replace(/\.md$/,'.html')}`;
     const response=await page.goto(url);assert.equal(response.status(),200,url);
     const article=page.locator('.vp-doc');assert((await article.innerText()).trim().length>80,`${url}: empty article`);
     if(await article.locator('#support-status').count()){
      assert.equal(await article.locator('h2').count(),3,url);
      assert(await article.locator('a[href*=".html"]').count()>0,`${url}: no native reference`);count++;
     }
    }
    assert.equal(count,expectedSupportPages,`${language}: unexpected support status coverage`);assert.deepEqual(errors,[]);
    console.log(`${base} ${language}: every navigation route rendered; ${count} explicit support pages PASS`);await page.close();
   }
  }finally{await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()));}
 }
}finally{await browser.close();}
