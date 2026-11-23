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
   for(const lang of ['en','zh-Hans','zh-Hant']) {
    const page=await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}${base}${lang}/packages/passport.html`);
    const article=await page.locator('.vp-doc').innerText();
    for(const term of ['passport:client','--redirect_uri','tokens_for','client_uuids','1000','use_refresh_token_model','ScopeJsonOptions','authorization_view','code_verifier','S256','no-store','prompt=none','refresh_token','passport:purge','TokenGuard::token','GrantLifetime','auth:api','device_user_code_view','device_authorization_view','authorization_pending','slow_down','expired_token','verification_uri_complete','urn:ietf:params:oauth:grant-type:device_code','client_credentials','PersonalAccessTokenFactory','get_token_for','CheckTokenForAnyScope','EnsureClientIsResourceOwner','CreateFreshApiToken','X-CSRF-TOKEN','Refreshed.','Delivery::AfterCommit','acting_as_client','default_scopes','enable_password_grant','enable_implicit_grant','find_and_validate_for_passport_using','PasswordGrantResolver','response_type=token','install:api --passport','passport:install','--without-migration-prompt','--length=2048','--composer','use_auth_code_model','use_device_code_model','auth_codes_for','ignore_routes','auth:sanctum','register_sanctum_model']) assert(article.includes(term),`${lang}: missing ${term}`);
    assert.equal(await page.locator('.vp-doc h2').count(),26);
    console.log(base,lang,'Passport authorization code/PKCE/device documentation PASS');
    await page.close();
   }
  } finally {await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()));}
 }
} finally {await browser.close();}
