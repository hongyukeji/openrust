import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)(process.env.DOCS_PLAYWRIGHT_MODULE);
const origin=process.env.USER_STARTER_ORIGIN,other=process.env.USER_STARTER_OTHER_ORIGIN,
 mock=process.env.USER_STARTER_MOCK,mode=process.env.USER_STARTER_MODE,
 evidence=process.env.USER_STARTER_EVIDENCE;
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
const results=[];mkdirSync(evidence,{recursive:true});
async function context(language,mobile,subject){
 const context=await browser.newContext({viewport:mobile?{width:375,height:812}:{width:1440,height:900},extraHTTPHeaders:{'Accept-Language':language}});
 await context.addCookies([{name:'fixture_subject',value:subject,domain:'127.0.0.1',path:'/authorize',sameSite:'Lax'}]);
 await context.route(origin+'/auth/*/callback',async route=>{
  const response=await route.fetch({url:route.request().url().replace(origin,other),maxRedirects:0});
  await route.fulfill({response});
 });
 return context;
}
async function clearCache(){
 const response=await fetch(mock+'/clear-cache',{method:'POST'});assert.equal(response.status,204);
}
async function loginPassword(page,password){
 await page.goto(origin+'/user/confirm-password');
 await page.locator('input[name="password"]').fill(password);
 await page.locator('form[action$="/user/confirm-password"] button').click();
 await page.waitForURL(url=>!url.pathname.endsWith('/user/confirm-password'));
}
async function link(page,provider,confirm){
 await page.goto(origin+'/user/connected-accounts');
 await page.locator(`form[action$="/${provider}"]:not(:has(input[name="_method"])) button`).click();
 await page.locator('form[action$="/connected-accounts/confirm"]').waitFor();
 await page.locator(`form[action$="/connected-accounts/confirm"] button[value="${confirm?'1':'0'}"]`).click();
 await page.waitForURL(url=>url.pathname==='/user/connected-accounts');
}
try {
 if(mode==='full')for(const language of ['en','zh-Hans','zh-Hant'])for(const mobile of [false,true]){
  await clearCache();const subject=`browser-${language}-${mobile?'mobile':'desktop'}`;
  const ctx=await context(language,mobile,subject),page=await ctx.newPage(),errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  await page.goto(origin+'/register');
  await page.locator('input[name="name"]').fill('OAuth Browser Account');
  await page.locator('input[name="email"]').fill(subject+'@example.test');
  await page.locator('input[name="password"]').fill('browser-password-123');
  await page.locator('input[name="password_confirmation"]').fill('browser-password-123');
  await page.locator('form[action$="/register"] button').click();await page.waitForURL(url=>url.pathname==='/home');
  await loginPassword(page,'browser-password-123');
  await link(page,'google',true);
  await link(page,'github',false);
  assert.equal(await page.locator('form[action$="/github"] input[name="_method"]').count(),0);
  await link(page,'github',true);
  assert.equal(await page.locator('input[name="_method"][value="DELETE"]').count(),2);
  await page.goto(origin+'/home');
  assert.ok((await page.locator('body').innerText()).includes(process.env.USER_STARTER_APP_NAME));
  await page.locator('form[action$="/logout"] button').click();
  await page.goto(origin+'/login');
  await page.locator('a[href$="/auth/google/redirect"]').click();await page.waitForURL(url=>url.pathname==='/home');
  await page.goto(origin+'/user/connected-accounts');
  await page.locator('form[action$="/google"] button').click();
  await page.waitForURL(url=>url.pathname==='/user/confirm-password');
  await page.locator('input[name="password"]').fill('browser-password-123');
  await page.locator('form[action$="/user/confirm-password"] button').click();
  await page.waitForURL(url=>url.pathname==='/user/connected-accounts');
  await page.locator('form[action$="/google"] button').click();
  await page.waitForURL(url=>url.pathname==='/user/connected-accounts');
  assert.equal(await page.locator('form[action$="/google"] input[name="_method"]').count(),0);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  assert.deepEqual(errors,[]);
  await page.screenshot({path:`${evidence}/${language}-${mobile?'mobile':'desktop'}.png`,fullPage:true});
  results.push({language,mobile,providerLink:true,explicitDeny:true,relogin:true,reconfirmation:true,unlink:true,callbackNodes:2});await ctx.close();
 }
 await clearCache();
 const ctx=await context('zh-Hans',false,`api-${mode}`),page=await ctx.newPage(),requests=[];
 page.on('request',request=>{if(new URL(request.url()).pathname.startsWith('/api/admin/'))requests.push({cookie:request.headers().cookie??null,csrf:request.headers()['x-csrf-token']??null});});
 const code=readFileSync(process.env.USER_STARTER_CLIENT_EXAMPLE,'utf8').replace("const prefix = '/api';",`const prefix = '${origin}/api/admin';`)+ '\nwindow.socialiteClient={beginProvider,completeProvider};';
 await page.goto(mock+'/frontend');await page.addScriptTag({type:'module',content:code});await page.waitForFunction(()=>Boolean(window.socialiteClient));
 await page.evaluate(({returnUri})=>{void window.socialiteClient.beginProvider('google',returnUri,'native-client');},{returnUri:mock+'/frontend'});
 await page.waitForURL(url=>url.origin===mock&&url.pathname==='/frontend'&&url.hash.startsWith('#code='));
 await page.addScriptTag({type:'module',content:code});await page.waitForFunction(()=>Boolean(window.socialiteClient));
 const result=await page.evaluate(async({origin})=>{
  const response=await window.socialiteClient.completeProvider();
  if(!response.data?.token)throw new Error('Expected completed token grant');
  const current=await fetch(origin+'/api/admin/user',{credentials:'omit',headers:{Accept:'application/json',Authorization:`Bearer ${response.data.token}`}});
  if(current.status!==200)throw new Error('Protected current-user request failed');
  const body=await current.json();return {email:body.data.email,tokenType:response.data.token_type,fragmentCleared:location.hash===''};
 },{origin});
 assert.equal(result.tokenType,'Bearer');assert.equal(result.fragmentCleared,true);assert.equal(result.email,`api-${mode}@example.test`);
 assert.ok(requests.length>=3);assert.ok(requests.every(request=>request.cookie===null&&request.csrf===null));
 results.push({mode,documentedClient:true,cors:true,bearer:true,callbackNodes:2,cookie:false,csrf:false,fragmentCleared:true});
 if(mode==='api'){
  const response=await page.request.get(origin+'/user/connected-accounts',{headers:{Accept:'application/json'}});assert.equal(response.status(),404);
 }
 await ctx.close();
 console.log(`PASS ${results.length} installed ${mode} browser workflows; documented S256 client and shared MySQL callback nodes`);
}finally{
 writeFileSync(`${evidence}/browser-results.json`,JSON.stringify(results,null,2));await browser.close();
}
