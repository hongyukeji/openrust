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
    await page.goto(`http://127.0.0.1:${server.address().port}${base}${lang}/packages/oidc.html`);
    const article=await page.locator('.vp-doc').innerText();
    for(const term of ['OidcManager','Oidc','OIDC_ISSUER','OIDC_CLIENT_ID','OIDC_REPLAY_STORE','PKCE','maxAge','VerifiedIdentity','issuer','subject','login_bound_user','loginUsingId','VARBINARY','allowed_origins','RS256','auth_time','API-O02','OIDC_POST_LOGOUT_REDIRECT_URI','LogoutHint','rememberLogout','takeLogoutHint','logoutAndInvalidate','logoutUrl','completeLogout','end_session_endpoint','RP-Initiated Logout']) assert(article.includes(term),`${lang}: missing ${term}`);
    assert.equal(await page.locator('.vp-doc h2').count(),8);
    console.log(base,lang,'OIDC documentation PASS');
    await page.close();
   }
  } finally {await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()));}
 }
} finally {await browser.close();}
