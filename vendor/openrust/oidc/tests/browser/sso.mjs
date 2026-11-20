import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
const a=process.env.OIDC_RP_A,b=process.env.OIDC_RP_B,op=process.env.OIDC_OP;
const context=await browser.newContext();
const page=await context.newPage();
page.setDefaultTimeout(10000);
const get=async (base,path)=>context.request.get(base+path,{maxRedirects:0});
const who=async base=>(await get(base,'/who')).json();
const control=async mode=>(await get(op,'/fixture/control'+(mode===undefined?'':'?mode='+encodeURIComponent(mode)))).json();
const post=async(base,path,csrf)=>context.request.post(base+path,{form:{},headers:{'x-csrf-token':csrf},maxRedirects:0});
const callbackURL=base=>url=>url.origin===base&&url.pathname==='/callback';
async function signIn(base,path='/login',expectPrompt=false){
 const response=await page.goto(base+path);
 if(expectPrompt){
  assert.equal(new URL(page.url()).origin,op);
  await page.getByLabel('Username').fill('alice');
  await page.getByLabel('Password',{exact:true}).fill('correct');
  await Promise.all([page.waitForURL(callbackURL(base)),page.getByRole('button',{name:'Sign in',exact:true}).click()]);
 }else assert(callbackURL(base)(new URL(page.url())),'Expected a silent OP redirect back to RP');
 assert.equal((await who(base)).user,base===a?'101':'201');
 return response;
}
try{
 await signIn(a,'/login',true);
 assert.equal((await control()).signins,1);
 const firstCallback=page.url();
 assert.equal((await get(a,new URL(firstCallback).pathname+new URL(firstCallback).search)).status(),400);
 await signIn(b);
 assert.equal((await control()).signins,1);
 assert.equal((await who(a)).user,'101');
 assert.equal((await who(b)).user,'201');
 console.log('PASS browser SSO: distinct client IDs, database schemas and local users share one OP sign-in');

 // Foreign encrypted Session cookies cannot cross application keys/stores.
 const cookies=await context.cookies();const ac=cookies.find(c=>c.name==='rp_a_session'),bc=cookies.find(c=>c.name==='rp_b_session');
 await context.addCookies([{...bc,value:ac.value}]);
 assert.equal((await who(b)).user,null);
 await context.addCookies([bc]);assert.equal((await who(b)).user,'201');
 console.log('PASS browser cross-application cookies fail');
 const concurrentA=await context.newPage(),concurrentB=await context.newPage();
 await Promise.all([concurrentA.goto(a+'/login'),concurrentB.goto(b+'/login')]);
 assert.equal((await who(a)).user,'101');assert.equal((await who(b)).user,'201');
 assert.equal((await control()).signins,1);await concurrentA.close();await concurrentB.close();
 console.log('PASS concurrent RP authorization uses independent client-bound state');

 await signIn(b,'/login?force=1',true);assert.equal((await control()).signins,2);
 console.log('PASS prompt=login requires actual OP credential entry');

 // A failing callback in B cannot replace its existing local account, or affect A.
 await control('other-subject');const unbound=await page.goto(b+'/login');assert.equal(unbound.status(),403);
 assert.equal((await who(b)).user,'201');assert.equal((await who(a)).user,'101');await control('');
 console.log('PASS unknown issuer/subject binding does not merge by verified email');
 for(const mode of ['other-issuer','old-auth']){
  await control(mode);const rejected=await page.goto(b+'/login?fresh=1');assert.equal(rejected.status(),400);
  assert.equal((await who(a)).user,'101');assert.equal((await who(b)).user,'201');
 }
 await control('');console.log('PASS other-tenant issuer and stale auth_time fail without replacing local accounts');

 let before=await who(a);assert.equal((await post(a,'/logout/local','wrong')).status(),419);assert.equal((await who(a)).user,'101');
 assert.equal((await post(a,'/logout/local',before.csrf)).status(),200);assert.equal((await who(a)).user,null);assert.equal((await who(b)).user,'201');
 await signIn(a);assert.equal((await control()).signins,2);
 console.log('PASS CSRF and local logout: RP B and OP Session remain valid');

 for(const mode of ['no-logout','unavailable','logout-query']){
  await control(mode);const user=await who(a);const response=await post(a,'/logout/federated',user.csrf);assert.equal(response.status(),200);assert.equal((await response.json()).federated,false);assert.equal((await who(a)).user,null);
  await control('');await signIn(a);
 }
 console.log('PASS unsupported, unavailable and conflicting logout metadata still permit local logout');

 // RP A ends the OP Session; B retains its own native Session. Access tokens survive.
 const token=(await control()).last_token;assert(token);
 before=await who(a);const response=await post(a,'/logout/federated',before.csrf);assert.equal(response.status(),302);
 const logoutURL=response.headers()['location'];assert.equal(new URL(logoutURL).origin,op);
 // B has no matching logout state; this cannot end its local session.
 const state=new URL(logoutURL).searchParams.get('state');assert.equal((await get(b,'/signed-out?state='+state)).status(),400);assert.equal((await who(b)).user,'201');
 const completed=await page.goto(logoutURL);assert.equal(completed.status(),200);const completedURL=page.url();assert.equal((await completed.json()).federated,true);
 assert.equal((await who(a)).user,null);assert.equal((await who(b)).user,'201');
 assert.equal((await get(a,new URL(completedURL).pathname+new URL(completedURL).search)).status(),400);
 const bearer=await context.request.get(op+'/userinfo',{headers:{authorization:'Bearer '+token}});assert.equal(bearer.status(),200);
 console.log('PASS RP initiated logout validates one-time state; OP logout preserves RP B and existing bearer token');

 // Silent authorization fails without an OP Session. Real sign-in is required again.
 const silent=await page.goto(a+'/login?silent=1');assert.equal(silent.status(),400);assert.equal((await who(a)).user,null);
 await signIn(a,'/login',true);assert.equal((await control()).signins,3);
 console.log('PASS post-logout prompt=none rejects, then a new OP sign-in succeeds');

 // Both callbacks load the same database Session before the atomic replay claim.
 before=await who(a);const racing=await post(a,'/logout/federated',before.csrf);assert.equal(racing.status(),302);
 const raceState=new URL(racing.headers()['location']).searchParams.get('state');
 const raced=await Promise.all([get(a,'/signed-out?race=1&state='+raceState),get(a,'/signed-out?race=1&state='+raceState)]);
 assert.deepEqual(raced.map(r=>r.status()).sort(),[200,400]);assert.equal((await who(a)).user,null);
 await signIn(a);
 for(const mode of ['expired','config','redirect']){
  before=await who(a);const attempt=await post(a,'/logout/federated',before.csrf);assert.equal(attempt.status(),302);
  const state=new URL(attempt.headers()['location']).searchParams.get('state');
  assert.equal((await get(a,'/signed-out?state='+state+'&alter='+mode)).status(),400);
  assert.equal((await get(a,'/signed-out?state='+state)).status(),400);
  assert.equal((await who(a)).user,null);await signIn(a);
 }
 console.log('PASS concurrent, expired, changed-client and changed-redirect logout state are single use');

 // A malformed return consumes its logout state and cannot restore local login.
 before=await who(a);const prepare=await post(a,'/logout/federated',before.csrf);assert.equal(prepare.status(),302);
 const remote=await context.request.get(prepare.headers()['location'],{maxRedirects:0});assert.equal(remote.status(),302);
 const target=new URL(remote.headers()['location']);const valid=target.toString();target.searchParams.set('state','wrong');
 assert.equal((await page.goto(target.toString())).status(),400);assert.equal((await page.goto(valid)).status(),400);assert.equal((await who(a)).user,null);
 console.log('PASS malformed logout state is consumed and never logs a user in');

 await get(op,'/fixture/control?revoke=all');assert.equal((await context.request.get(op+'/userinfo',{headers:{authorization:'Bearer '+token}})).status(),401);assert.equal((await who(b)).user,'201');
 const bUser=await who(b);assert.equal((await post(b,'/logout/local',bUser.csrf)).status(),200);assert.equal((await who(b)).user,null);
 console.log('PASS provider token revocation and native RP Session logout have distinct boundaries');
}finally{await context.close();await browser.close();}
