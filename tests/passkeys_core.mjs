import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { createServer } from 'node:http';
import assert from 'node:assert/strict';
const require=createRequire(import.meta.url);
const { chromium }=require(process.env.OPENRUST_PLAYWRIGHT_MODULE || 'playwright');
const driver=spawn(process.argv[2],['--protocol'],{stdio:['pipe','pipe','inherit']});
const lines=createInterface({input:driver.stdout});const pending=[];
lines.on('line',line=>pending.shift()(JSON.parse(line)));
const call=value=>new Promise(resolve=>{pending.push(resolve);driver.stdin.write(JSON.stringify(value)+'\n')});
const good=async value=>{const r=await call(value);assert.equal(r.error,undefined,JSON.stringify(r));return r.ok};
const bad=async value=>{const r=await call(value);assert.equal(typeof r.error,'string',JSON.stringify(r));return r.error};
const server=createServer((req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><title>WebAuthn acceptance</title>')});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const port=server.address().port,origin=`http://localhost:${port}`;
const browser=await chromium.launch({executablePath:process.env.OPENRUST_BROWSER_PATH,headless:true});
try {
 const page=await browser.newPage();await page.goto(origin);
 const cdp=await page.context().newCDPSession(page);await cdp.send('WebAuthn.enable');
 let authenticator=(await cdp.send('WebAuthn.addVirtualAuthenticator',{options:{protocol:'ctap2',transport:'internal',hasResidentKey:true,hasUserVerification:true,isUserVerified:true,automaticPresenceSimulation:true}})).authenticatorId;
 await good({op:'init',origin});
 const algorithm=process.env.OPENRUST_TEST_PASSKEY_ALGORITHM;
 const create=options=>{
  if(algorithm!==undefined){
   assert.ok(['-7','-257'].includes(algorithm),'supported test algorithm');
   options.pubKeyCredParams=options.pubKeyCredParams.filter(value=>value.alg===Number(algorithm));
   assert.equal(options.pubKeyCredParams.length,1,'requested algorithm must be offered by the native verifier');
  }
  return page.evaluate(async options=>(await navigator.credentials.create({publicKey:PublicKeyCredential.parseCreationOptionsFromJSON(options)})).toJSON(),options);
 };
 const get=options=>page.evaluate(async options=>(await navigator.credentials.get({publicKey:PublicKeyCredential.parseRequestOptionsFromJSON(options)})).toJSON(),options);
 const first=await create(await good({op:'registration'}));const id=await good({op:'register',credential:first});
 const stored=await good({op:'stored',id});
 assert.match(stored.aaguid,/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
 const legacy=await good({op:'stored',id,legacy:true});assert.equal(legacy.aaguid,null);
 console.log('Real registration AAGUID extraction and stored credentials without AAGUID remain usable PASS');
 assert.equal(id,first.rawId);await bad({op:'register',credential:first});
 console.log('Chrome resident registration, 64-byte user handle, none attestation and one-time native state PASS');
 const exclusion=await good({op:'registration',exclude:true});assert.equal(exclusion.excludeCredentials[0].id,id);
 // Duplicate IDs are excluded by the server state as well as the browser hint.
 const mismatch=structuredClone(first);mismatch.id='eA';await bad({op:'register',credential:mismatch});
 let options=await good({op:'verification'});assert.equal(options.allowCredentials.length,0);
 let assertion=await get(options);const verified=await good({op:'verify',credential:assertion});assert.ok(verified.counter>0);assert.equal(verified.handle,'a'.repeat(64));
 await bad({op:'verify',credential:assertion});
 console.log('Discoverable signed authentication, UV, user binding, counter update and consumed-state replay rejection PASS');
 const checks=[
  ['wrong challenge',c=>{const d=JSON.parse(Buffer.from(c.response.clientDataJSON,'base64url'));d.challenge='eA';c.response.clientDataJSON=Buffer.from(JSON.stringify(d)).toString('base64url')}],
  ['wrong origin',c=>{const d=JSON.parse(Buffer.from(c.response.clientDataJSON,'base64url'));d.origin='https://evil.example';c.response.clientDataJSON=Buffer.from(JSON.stringify(d)).toString('base64url')}],
  ['cross origin',c=>{const d=JSON.parse(Buffer.from(c.response.clientDataJSON,'base64url'));d.crossOrigin=true;c.response.clientDataJSON=Buffer.from(JSON.stringify(d)).toString('base64url')}],
  ['wrong RP hash',c=>{const d=Buffer.from(c.response.authenticatorData,'base64url');d[0]^=1;c.response.authenticatorData=d.toString('base64url')}],
  ['missing UV',c=>{const d=Buffer.from(c.response.authenticatorData,'base64url');d[32]&=~4;c.response.authenticatorData=d.toString('base64url')}],
  ['wrong user handle',c=>c.response.userHandle=Buffer.from('other-user').toString('base64url')],
  ['missing discoverable user handle',c=>delete c.response.userHandle],
  ['wrong signature',c=>{const d=Buffer.from(c.response.signature,'base64url');d[d.length-1]^=1;c.response.signature=d.toString('base64url')}],
  ['wrong type',c=>c.type='password'],['mismatched raw ID',c=>c.id='eA'],
 ];
 for(const [label,mutate] of checks){options=await good({op:'verification'});assertion=await get(options);mutate(assertion);await bad({op:'verify',credential:assertion});console.log(`${label} rejected PASS`)}
 // Change the actual browser origin while keeping authenticator signatures valid.
 const other=createServer((req,res)=>res.end('<!doctype html><title>Other origin</title>'));
 await new Promise(resolve=>other.listen(0,'127.0.0.1',resolve));
 try {
  await page.goto(`http://localhost:${other.address().port}`);
  options=await good({op:'verification'});assertion=await get(options);await bad({op:'verify',credential:assertion});
  console.log('Valid signature from unconfigured origin/port rejected PASS');
 } finally {other.close();}
 await page.goto(origin);
 options=await good({op:'verification',allow:true});assert.equal(options.allowCredentials[0].id,id);
 assertion=await get(options);await good({op:'verify',credential:assertion,discoverable:false});
 await cdp.send('WebAuthn.setUserVerified',{authenticatorId:authenticator,isUserVerified:false});
 options=await good({op:'verification'});options.userVerification='discouraged';assertion=await get(options);await bad({op:'verify',credential:assertion});
 await cdp.send('WebAuthn.setUserVerified',{authenticatorId:authenticator,isUserVerified:true});
 console.log('Valid signature without user verification rejected PASS');
 // Persisted key at a rolled-back signature counter must fail even on a fresh challenge.
 const credentials=(await cdp.send('WebAuthn.getCredentials',{authenticatorId:authenticator})).credentials;
 const credential=credentials[0];await cdp.send('WebAuthn.removeCredential',{authenticatorId:authenticator,credentialId:credential.credentialId});
 await cdp.send('WebAuthn.addCredential',{authenticatorId:authenticator,credential:{...credential,signCount:0}});
 options=await good({op:'verification'});assertion=await get(options);await bad({op:'verify',credential:assertion});
 console.log('Known-user allow list and valid signed counter rollback rejection PASS');
 console.log('Native WebAuthn core with real Chrome virtual authenticator PASS');
} finally {await browser.close();server.close();driver.stdin.end();}
