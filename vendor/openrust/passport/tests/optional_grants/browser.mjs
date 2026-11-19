import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const {chromium,request}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const base=process.env.OPENRUST_BROWSER_BASE;
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
const machine=await request.newContext({baseURL:base});
try {
 const context=await browser.newContext();const page=await context.newPage();
 await page.goto(base+'/session');const session=JSON.parse(await page.locator('body').innerText());
 assert.equal((await context.request.post(base+'/login',{data:{email:'alice@test',password:'password',_token:session.csrf}})).status(),200);
 const callback=base+'/session?existing=1';
 const query=new URLSearchParams({response_type:'token',client_id:process.env.OPENRUST_BROWSER_IMPLICIT_ID,redirect_uri:callback,scope:'read',state:'browser & optional',prompt:'consent'});
 await page.goto(base+'/oauth/authorize?'+query);
 await page.getByRole('button',{name:'Approve',exact:true}).click();
 await page.waitForURL(url=>url.pathname==='/session'&&url.hash.includes('access_token='));
 const result=await page.evaluate(async()=>{
  const params=new URLSearchParams(location.hash.slice(1));
  const response=await fetch('/resource',{headers:{Authorization:'Bearer '+params.get('access_token')}});
  return {state:params.get('state'),type:params.get('token_type'),refresh:params.has('refresh_token'),status:response.status,user:(await response.json()).user};
 });
 assert.deepEqual(result,{state:'browser & optional',type:'Bearer',refresh:false,status:200,user:77});
 const response=await machine.post('/oauth/token',{form:{grant_type:'password',client_id:process.env.OPENRUST_BROWSER_PASSWORD_ID,client_secret:process.env.OPENRUST_BROWSER_PASSWORD_SECRET,username:'bob@test',password:'password',scope:'read'}});
 assert.equal(response.status(),200);const token=await response.json();assert(token.refresh_token);
 const resource=await machine.get('/resource',{headers:{Authorization:'Bearer '+token.access_token}});assert.equal(resource.status(),200);assert.equal((await resource.json()).user,88);
 console.log('Chromium optional implicit consent / fragment / resource / separate password client PASS');await context.close();
} finally {await machine.dispose();await browser.close();}
