import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {serve} from 'vitepress';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
 for(const base of ['/', '/docs/']) {
  const {server}=await serve({root:fileURLToPath(new URL('../',import.meta.url)),base,port:0});
  try {
   for(const lang of ['en','zh_CN','zh_TW']) {
    const page=await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}${base}${lang}/packages/passport.html`);
    const article=await page.locator('.vp-doc').innerText();
    for(const term of ['passport:client','--redirect_uri','tokensFor','clientUuids','1000','useRefreshTokenModel','ScopeJsonOptions','authorizationView','code_verifier','S256','no-store','prompt=none','refresh_token','passport:purge','TokenGuard::token','GrantLifetime','auth:api','deviceUserCodeView','deviceAuthorizationView','authorization_pending','slow_down','expired_token','verification_uri_complete','urn:ietf:params:oauth:grant-type:device_code','client_credentials','PersonalAccessTokenFactory','getTokenFor','CheckTokenForAnyScope','EnsureClientIsResourceOwner','CreateFreshApiToken','X-CSRF-TOKEN','Refreshed.','Delivery::AfterCommit','actingAsClient','defaultScopes','enablePasswordGrant','enableImplicitGrant','findAndValidateForPassportUsing','PasswordGrantResolver','response_type=token','install:api --passport','passport:install','--without-migration-prompt','--length=2048','--composer']) assert(article.includes(term),`${lang}: missing ${term}`);
    assert.equal(await page.locator('.vp-doc h2').count(),23);
    console.log(base,lang,'Passport authorization code/PKCE/device documentation PASS');
    await page.close();
   }
  } finally {await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()));}
 }
} finally {await browser.close();}
