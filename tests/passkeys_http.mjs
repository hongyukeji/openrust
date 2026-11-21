import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const require=createRequire(import.meta.url);
const { chromium }=require(process.env.OPENRUST_PLAYWRIGHT_MODULE || 'playwright');
const origin=process.argv[2],bundle=await readFile(process.argv[3],'utf8');
const customModel=process.env.OPENRUST_TEST_PASSKEY_MODEL==='1';
const browser=await chromium.launch({executablePath:process.env.OPENRUST_BROWSER_PATH,headless:true});
try {
 const context=await browser.newContext();
 const page=await context.newPage();
 await page.route('**/passkeys-acceptance-client.js',route=>route.fulfill({contentType:'application/javascript',body:bundle}));
 await page.goto(origin+'/fortify-acceptance-state');
 await page.setContent('<!doctype html><meta name="csrf-token"><title>Passkeys HTTP acceptance</title>');
 await page.evaluate(async()=>{const module=await import('/passkeys-acceptance-client.js');window.Passkeys=module.Passkeys;});
 const cdp=await context.newCDPSession(page);await cdp.send('WebAuthn.enable');
 const authenticator=(await cdp.send('WebAuthn.addVirtualAuthenticator',{options:{protocol:'ctap2',transport:'internal',hasResidentKey:true,hasUserVerification:true,isUserVerified:true,automaticPresenceSimulation:true}})).authenticatorId;
 const request=(method,path,data,csrf=true)=>page.evaluate(async({method,path,data,csrf})=>{
  const r=await fetch(path,{method,headers:{Accept:'application/json',...(data===undefined?{}:{'Content-Type':'application/json'}),...(csrf?{'X-CSRF-TOKEN':window.csrf||''}:{})},body:data===undefined?undefined:JSON.stringify(data)});
  const raw=await r.text();return {status:r.status,data:raw?JSON.parse(raw):null};
 },{method,path,data,csrf});
 const expect=async(method,path,data,status=200,csrf=true)=>{const r=await request(method,path,data,csrf);assert.equal(r.status,status,JSON.stringify({method,path,...r}));return r.data};
 const state=async()=>{const s=await expect('GET','/fortify-acceptance-state');await page.evaluate(s=>{window.csrf=s.csrf;document.querySelector('meta[name="csrf-token"]').content=s.csrf},s);return s};
 const create=o=>page.evaluate(async o=>(await navigator.credentials.create({publicKey:PublicKeyCredential.parseCreationOptionsFromJSON(o)})).toJSON(),o);
 const get=o=>page.evaluate(async o=>(await navigator.credentials.get({publicKey:PublicKeyCredential.parseRequestOptionsFromJSON(o)})).toJSON(),o);
 await state();await expect('GET','/user/passkeys/options',undefined,401);
 await expect('POST','/login',{email:'new@example.test',password:'third-secret'});
 const logged=await state();assert.ok(logged.user);assert.equal(logged.confirmed,null);
 await expect('GET','/user/passkeys/options',undefined,423);
 await expect('POST','/user/confirm-password',{password:'third-secret'},201);await state();
 const withoutCsrf=await context.request.post(origin+'/user/passkeys',{data:{},headers:{Accept:'application/json'}});assert.equal(withoutCsrf.status(),419);
 await expect('POST','/user/passkeys',{},422);
 const registered=await page.evaluate(()=>window.Passkeys.register({name:'Chrome resident key'}));
 assert.equal(registered.status,'passkey-registered');assert.equal(registered.name,customModel?'CHROME RESIDENT KEY':'Chrome resident key');
 const list=await expect('GET','/passkeys-acceptance-list');assert.equal(list.length,1);assert.equal(String(list[0].id),registered.id);assert.ok(list[0].credential_id);
 assert.ok(Object.hasOwn(list[0],'authenticator'));
 if(customModel){assert.equal(String(list[0].pk),registered.id);assert.equal(list[0].custom_marker,'selected-model');assert.equal(Object.hasOwn(list[0],'credential'),false);}
 const rawId=list[0].credential_id;
 console.log('Installed default provider, migration, real account, CSRF, confirmation gate, pinned JS registration and native model listing PASS');
 const exclude=(await expect('GET','/user/passkeys/options')).options;assert.equal(exclude.excludeCredentials[0].id,rawId);
 const confirmation=await page.evaluate(()=>window.Passkeys.verify({routes:{options:'/passkeys/confirm/options',submit:'/passkeys/confirm'}}));
 assert.ok(confirmation.redirect);assert.ok((await state()).confirmed);
 console.log('Pinned JS passkey confirmation and native password-confirmed timestamp PASS');
 // One issued challenge is consumed at most once even by two concurrent requests.
 const confirmOptions=(await expect('GET','/passkeys/confirm/options')).options;
 const assertion=await get(confirmOptions);
 const isolated=await browser.newContext();
 const isolatedState=await (await isolated.request.get(origin+'/fortify-acceptance-state',{headers:{Accept:'application/json'}})).json();
 const stolen=await isolated.request.post(origin+'/passkeys/login',{data:{credential:assertion},headers:{Accept:'application/json','X-CSRF-TOKEN':isolatedState.csrf}});
 assert.equal(stolen.status(),422);await isolated.close();
 console.log('A signed assertion cannot use ceremony state from a different Session PASS');
 const statuses=(await Promise.all([request('POST','/passkeys/confirm',{credential:assertion}),request('POST','/passkeys/confirm',{credential:assertion})])).map(r=>r.status).sort();
 assert.deepEqual(statuses,[200,422]);
 console.log('Concurrent single-use challenge, signature counter persistence and replay rejection PASS');
 // A second account cannot confirm or delete the first account's credential.
 await expect('POST','/logout',{},204);await state();
 await expect('POST','/register',{name:'Bob',email:'bob@example.test',password:'bob-secret',password_confirmation:'bob-secret'},201);await state();
 await expect('POST','/user/confirm-password',{password:'bob-secret'},201);await state();
 await expect('DELETE','/user/passkeys/'+registered.id,{},403);
 const otherOptions=(await expect('GET','/passkeys/confirm/options')).options;
 // Empty allowCredentials on an account without keys must not permit a different owner's key.
 const otherAssertion=await get(otherOptions);await expect('POST','/passkeys/confirm',{credential:otherAssertion},422);
 await expect('POST','/logout',{},204);const guest=await state();
 const loggedIn=await page.evaluate(()=>window.Passkeys.verify({remember:true}));assert.ok(loggedIn.redirect);
 const after=await state();assert.equal(after.email,'new@example.test');assert.notEqual(after.session,guest.session);assert.notEqual(after.csrf,guest.csrf);
 console.log('Cross-account confirmation/deletion denied; pinned JS discoverable login, remember and Session/CSRF rotation PASS');
 // Invalid shape does not consume the saved registration options.
 await expect('GET','/passkeys/confirm/options');
 await expect('POST','/passkeys/confirm',{credential:{}},422);
 // Timeout is enforced on server state, not merely supplied as a browser hint.
 await expect('GET','/passkeys-acceptance-policy/expire');
 const expired=(await expect('GET','/passkeys/confirm/options')).options;expired.timeout=60000;
 const expiredAssertion=await get(expired);await expect('POST','/passkeys/confirm',{credential:expiredAssertion},422);
 await expect('GET','/passkeys-acceptance-policy/normal');
 console.log('Malformed request rejection and server challenge expiry PASS');
 // Management requires confirmation again after passkey login.
 await expect('DELETE','/user/passkeys/'+registered.id,{},423);
 await page.evaluate(()=>window.Passkeys.verify({routes:{options:'/passkeys/confirm/options',submit:'/passkeys/confirm'}}));await state();
 await expect('DELETE','/user/passkeys/'+registered.id,{});
 assert.equal((await expect('GET','/passkeys-acceptance-list')).length,0);
 await expect('POST','/logout',{},204);await state();
 const requestOptions=(await expect('GET','/passkeys/login/options')).options;
 const removed=await get(requestOptions);await expect('POST','/passkeys/login',{credential:removed},422);
 console.log('Deletion, missing credential rejection and persisted model lifecycle PASS');
 const limited=await browser.newContext();const limitedPage=await limited.newPage();await limitedPage.goto(origin+'/fortify-acceptance-state');
 const limits=await limitedPage.evaluate(async()=>{const result=[];for(let i=0;i<11;i++){const r=await fetch('/passkeys/login/options',{headers:{Accept:'application/json'}});result.push(r.status);}return result});
 assert.deepEqual(limits,[...Array(10).fill(200),429]);await limited.close();
 console.log('Published named passkey limiter allows ten requests and rejects the eleventh PASS');
 // Two independent Sessions/challenges with equal signed counters contend on
 // the credential row. Atomic challenge consumption cannot mask this race.
 await expect('POST','/register',{name:'Race',email:'race@example.test',password:'race-secret',password_confirmation:'race-secret'},201);await state();
 await expect('POST','/user/confirm-password',{password:'race-secret'},201);await state();
 await page.evaluate(()=>window.Passkeys.register({name:'Concurrent key'}));
 const raceRecord=(await expect('GET','/passkeys-acceptance-list'))[0];
 const clients=await Promise.all([browser.newContext(),browser.newContext()]);
 const signed=[];
 const originals=(await cdp.send('WebAuthn.getCredentials',{authenticatorId:authenticator})).credentials;
 const raceCredential=originals.find(c=>Buffer.from(c.credentialId,'base64').toString('base64url')===raceRecord.credential_id);assert.ok(raceCredential);
 for(const client of clients){
  const state=await (await client.request.get(origin+'/fortify-acceptance-state',{headers:{Accept:'application/json'}})).json();
  const options=(await (await client.request.get(origin+'/passkeys/login/options',{headers:{Accept:'application/json'}})).json()).options;
  options.allowCredentials=[{type:'public-key',id:raceRecord.credential_id}];
  await cdp.send('WebAuthn.removeCredential',{authenticatorId:authenticator,credentialId:raceCredential.credentialId});
  await cdp.send('WebAuthn.addCredential',{authenticatorId:authenticator,credential:raceCredential});
  signed.push({state,credential:await get(options)});
 }
 const results=await Promise.all(clients.map((client,i)=>client.request.post(origin+'/passkeys/login',{data:{credential:signed[i].credential},headers:{Accept:'application/json','X-CSRF-TOKEN':signed[i].state.csrf}})));
 assert.deepEqual(results.map(r=>r.status()).sort(),[200,422]);
 for(const client of clients)await client.close();
 assert.equal((await expect('POST','/passkeys-acceptance-cascade',{})).remaining,0);
 console.log('Independent challenges with equal signed counters serialize under the database row lock; user deletion cascades PASS');

 if(customModel){
  const events=await expect('GET','/passkeys-acceptance-policy/events');
  assert.equal(events.registered_event,2);assert.ok(events.verified_event>=4,JSON.stringify(events));assert.equal(events.deleted_event,1);assert.equal(events.deleted,1);assert.ok(events.saved>=events.registered_event+events.verified_event);
  console.log('Custom Eloquent table/primary key/mutator/hidden/appends, dynamic user relation, native observers and typed event context PASS');
 }
 console.log('Fortify Passkeys installed application / SQL database / real Chrome / pinned official JS PASS');
} finally {await browser.close();}
