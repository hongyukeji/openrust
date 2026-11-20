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
    await page.goto(`http://127.0.0.1:${server.address().port}${base}${lang}/packages/socialite.html`);
    const article=await page.locator('.vp-doc').innerText();
    for(const term of ['miniApp','codeToSession','WEIXIN_MINI_APP_APPID','WEIXIN_MINI_APP_SECRET','services.weixin_mini_app.app_id','session_key','session.user()','wx.login()', 'weixinweb','weixin','WEIXINWEB','WEIXIN','QQ','snsapi_base','snsapi_login','setOpenId','withUnionId','GitHub','SocialiteManager','SocialiteServiceProvider','GITHUB_CLIENT_ID','GITHUB_REDIRECT_URI','client_secret','enablePKCE','stateless','getId','getRaw','refreshToken','userFromToken','user:email','setScopes','ProviderDefinition','redactPayload','Zeroizing','GitLab','Bitbucket','LinkedIn','Slack','linkedin-openid','slack-openid','getAccessToken','asBotUser','/api/v3/user','ProviderEndpoints','TWITTER_OAUTH','twitter-oauth-2','X_CLIENT_ID','RFC3986','state=state','GOOGLE_CLIENT_ID','GoogleProvider','jsonwebtoken','prettyPrint','NumericDate','FACEBOOK_CLIENT_ID','FacebookProvider','withNonce','lastToken','appsecret_proof','v23.0','RS256','oauth1','oauth2','userFromTokenAndSecret','tokenSecret','TWITTER_SCOPE','extendOAuth1','login_bound_user','social_identities','provider_user_id','VARBINARY','loginUsingId','logoutAndInvalidate']) assert(article.includes(term),`${lang}: missing ${term}`);
    assert.equal(await page.locator('.vp-doc h2').count(),8);
    assert.equal(await page.locator('.vp-doc h3#wechat-mini-program').count(),1);
    console.log(base,lang,'Socialite GitHub/shared OAuth2 documentation PASS');
    await page.close();
   }
  } finally {await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()));}
 }
} finally {await browser.close();}
