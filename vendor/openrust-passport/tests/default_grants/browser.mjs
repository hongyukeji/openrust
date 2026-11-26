import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const {chromium,request}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const base=process.env.OPENRUST_BROWSER_BASE;
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
const device=await request.newContext({baseURL:base});
try {
 const context=await browser.newContext();
 const session=await (await context.request.get(`${base}/session`)).json();
 const login=await context.request.post(`${base}/login`,{data:{email:'alice@test',password:'password',_token:session.csrf}});
 assert.equal(login.status(),200);const {csrf}=await login.json();
 const page=await context.newPage();await page.goto(`${base}/fresh`);
 const call=async(path,options={})=>page.evaluate(async({path,options})=>{
  const response=await fetch(path,options);return {status:response.status,body:response.headers.get('content-type')?.includes('json')?await response.json():await response.text()};
 },{path,options});
 let response=await call('/cookie-resource',{headers:{'X-CSRF-TOKEN':csrf}});assert.equal(response.status,200);assert.equal(response.body.user,77);assert.equal(response.body.transient,true);
 assert.equal((await call('/cookie-resource')).status,401);
 const personal=await call('/pat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({_token:csrf,name:'Chromium',scopes:['read']})});assert.equal(personal.status,200);
 response=await call('/resource',{headers:{Authorization:`Bearer ${personal.body.accessToken}`}});assert.equal(response.status,200);assert.equal(response.body.transient,false);
 const machine=await device.post('/oauth/token',{form:{grant_type:'client_credentials',client_id:process.env.OPENRUST_BROWSER_MACHINE_ID,client_secret:process.env.OPENRUST_BROWSER_MACHINE_SECRET,scope:'read'}});assert.equal(machine.status(),200);const credentials=await machine.json();assert.equal(credentials.refresh_token,undefined);
 assert.equal((await device.get('/machine',{headers:{Authorization:`Bearer ${credentials.access_token}`}})).status(),200);
 assert.equal((await device.get('/resource',{headers:{Authorization:`Bearer ${credentials.access_token}`}})).status(),401);
 assert.equal((await call('/oauth/token/refresh',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({_token:csrf})})).status,200);
 assert.equal((await call('/logout',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({_token:csrf})})).status,204);
 assert.equal((await call('/cookie-resource',{headers:{'X-CSRF-TOKEN':csrf}})).status,401);
 console.log('Chromium Session / SPA cookie / CSRF / PAT / separate machine / logout PASS');
 await context.close();
} finally {await device.dispose();await browser.close();}
