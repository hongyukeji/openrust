# Socialite

Socialite 將第三方登入接入應用程式的 HTTP Client 與 Session。供應商 API 對應 Socialite 5.33.0，包括 GitHub、GitLab、Bitbucket、LinkedIn（舊式／OpenID）、Slack（identity／OpenID）、X／Twitter OAuth2、Google 和 Facebook，也支援 Twitter OAuth1 和下方本機帳戶登入範例。完整 OIDC RP 與 SSO 使用獨立的 [OIDC 套件](oidc.md)。

## 安裝 {#installation}

建置或執行應用程式時啟用可選 Cargo feature `socialite`。套件探索註冊 `SocialiteServiceProvider` 與應用內的 `SocialiteManager`，不會自動建立路由或本機使用者資料表。

```sh
cargo run --features socialite -- artisan serve
```

## 設定 {#configuration}

透過 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 設定 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 開頭的回呼拼接到合法絕對 HTTP(S) `app.url`，絕對回呼保留原值。`services.github.scopes` 向驅動預設值追加 scope。缺少或無效設定明確失敗。供應商登入需要已註冊的 GitHub OAuth 應用程式和對應回呼。

以下驅動同樣設定 client id、client secret 和 redirect；環境變數前綴後接 `_CLIENT_ID`、`_CLIENT_SECRET`、`_REDIRECT_URI`：

| 驅動／services 鍵 | 環境變數前綴 | 預設 scopes |
| --- | --- | --- |
| `gitlab` | `GITLAB` | `read_user` |
| `bitbucket` | `BITBUCKET` | `email` |
| `linkedin` | `LINKEDIN` | `r_liteprofile r_emailaddress` |
| `linkedin-openid` | `LINKEDIN_OPENID` | `openid profile email` |
| `slack` | `SLACK` | `identity.basic,identity.email,identity.team,identity.avatar` |
| `slack-openid` | `SLACK_OPENID` | `openid email profile` |

`services.gitlab.host`／`GITLAB_HOST` 預設 `https://gitlab.com`。`GitlabProvider::setHost` 保留自託管路徑前綴並移除結尾斜線。資料端點為 `/api/v3/user`。Slack 登入設定與既有 `slack.notifications` 共處同一 services 物件；真實供應商仍需分別註冊回呼並取得權限。

`x` 使用 `X_CLIENT_ID`、`X_CLIENT_SECRET`、`X_REDIRECT_URI`。`twitter` 使用對應的 `TWITTER_` 變數，並設定 `TWITTER_OAUTH=2` 選擇 OAuth2；預設值 1 選擇 Twitter OAuth1；只有整數 2 選擇 OAuth2，字串 `"2"` 仍選擇 OAuth1。明確的 `twitter-oauth-2` 驅動先讀取 `services.twitter`，僅在物件缺失或為 null 時回退 `services.twitter-oauth-2`。`x` 同樣從 `services.x` 回退 `services.x-oauth-2`；已存在但不完整的設定會失敗，不靜默改用另一組憑證。

`google` 讀取 `GOOGLE_CLIENT_ID`、`GOOGLE_CLIENT_SECRET`、`GOOGLE_REDIRECT_URI`，預設 scope 為 `openid profile email`。一般 access token 以 Bearer 呼叫 Google UserInfo，傳送 `pretty_print=false`。更新回應僅在缺少 refresh token 欄位時保留傳入值；明確的 null 或輪換值按回應保留。

`facebook` 使用 `FACEBOOK_CLIENT_ID`、`FACEBOOK_CLIENT_SECRET`、`FACEBOOK_REDIRECT_URI`，預設 scope 為 `email`，固定 Graph 版本為 `v23.0`。`provider.usingGraphVersion("v99.0")` 修改目前請求的授權／token／profile 路徑。`fields(vec!["id".into(), "name".into()])` 替換 Graph 欄位；`asPopup()`、`reRequest()` 覆寫相應可選授權參數。Graph 請求傳送 `access_token`、fields，並在 client secret 非空且不等於 `"0"` 時傳送 HMAC-SHA256 `appsecret_proof`。授權碼交換不指定 Accept，回傳保留 `expires`，僅在 `expires_in` 缺失或為 null 時補入，不覆蓋已有值。

### 微信與 QQ {#wechat-and-qq}

微信登入統一放在 `services.wechat`，依應用類型劃分；QQ 單獨放在 `services.qq`。網頁驅動名稱沿用 SocialiteProviders，設定與原始碼統一歸入微信分組：

| 驅動／入口 | 設定路徑 | 登入用途 | 環境變數前綴 |
| --- | --- | --- | --- |
| `weixinweb` | `services.wechat.web` | 微信網站掃碼 | `WECHAT_WEB` |
| `weixin` | `services.wechat.official_account` | 微信公眾號網頁授權 | `WECHAT_OFFICIAL_ACCOUNT` |
| `miniApp()` | `services.wechat.mini_app` | 微信小程式登入 | `WECHAT_MINI_APP` |
| `qq` | `services.qq` | QQ 互聯網站登入 | `QQ` |

網頁微信與 QQ 的前綴後設定 `_CLIENT_ID`、`_CLIENT_SECRET`、`_REDIRECT_URI`，對應 `client_id`、`client_secret`、`redirect`。微信填寫各自應用的 AppID／AppSecret，分別登記對應回呼網域。小程式使用下方的 AppID／secret 設定，無需回呼位址；三種微信入口使用各自獨立的應用憑證。

沿用下方跳轉／回呼範例，將 `github` 換成對應驅動。網站掃碼使用 `snsapi_login`，公眾號預設使用 `snsapi_userinfo`；靜默授權可用 `provider.setScopes(vec!["snsapi_base".into()])`，傳回 OpenID，不讀取暱稱與頭像。網頁登入保留預設 Session state 檢查，不要呼叫 `stateless()`。

微信授權碼交換會保存當前請求的 OpenID。直接使用 `userFromToken` 時，先呼叫 `provider.setOpenId(openid)`；權杖與 OpenID 必須屬於同一應用及使用者。QQ 先查詢 OpenID 再讀取資料；需要 UnionID 時呼叫 `provider.withUnionId(true)`，仍取決於供應商授予的權限。微信與 QQ 的 UnionID 可從 `user.attributes.get("unionid")` 讀取，暱稱與頭像使用標準 User 方法；這些驅動不傳回電子郵件。

本地帳戶以可信的應用／供應商 namespace 加 OpenID 綁定，不單憑暱稱或 UnionID 自動合併帳戶；註冊與綁定流程由應用決定。網站微信與公眾號的 OpenID 不能預設相同。`refreshToken` 按各自的重新整理端點執行，供應商拒絕、過期、撤銷或缺少身分欄位會傳回錯誤。正式使用仍須設定真實應用憑證、獲准回呼，並向供應商完成授權聯調。

升級既有設定時，將 `services.weixinweb`、`services.weixin`、`services.weixin_mini_app` 分別移至 `services.wechat.web`、`services.wechat.official_account`、`services.wechat.mini_app`。環境變數前綴對應從 `WEIXINWEB_`、`WEIXIN_`、`WEIXIN_MINI_APP_` 改為 `WECHAT_WEB_`、`WECHAT_OFFICIAL_ACCOUNT_`、`WECHAT_MINI_APP_`；舊位置不再讀取。自訂驅動型別從 `openrust_socialite::wechat::{WeixinProvider, WeixinWebProvider, MiniApp, Session}` 匯入。

### 微信小程式 {#wechat-mini-program}

小程式登入採用 EasyWeChat 風格的 `miniApp().codeToSession(code)`。設定 `WECHAT_MINI_APP_APPID` 與 `WECHAT_MINI_APP_SECRET`，對應 `services.wechat.mini_app.app_id`／`secret`；填寫小程式自己的 AppID／AppSecret，無需 redirect。此入口隨 `socialite` feature 提供，也可從 `Socialite::miniApp()` 取得。

小程式呼叫 `wx.login()`，將取得的 `code` 透過 HTTPS POST 傳給應用自己的登入介面；伺服器呼叫以下方法換取可信身分，再依下方本機帳戶範例綁定使用者，並簽發應用自己的 Sanctum／Passport 權杖或登入狀態。客戶端傳入的 OpenID、UnionID、暱稱及頭像不能作為登入憑證。

```rust
use openrust_socialite::{Result, SocialiteManager, two::User};

pub async fn mini_login(manager: &SocialiteManager, code: &str) -> Result<User> {
    let session = manager.miniApp()?.codeToSession(code).await?;
    Ok(session.user())
}
```

`Session.openid` 為使用者識別，`unionid` 僅在微信回傳時存在；`session_key` 為 `Zeroizing<String>`，僅供伺服器使用。Session 不實作序列化或 Debug，`session.user()` 產生的 Socialite User 僅含 OpenID／可選 UnionID，raw 與 attributes 均不含會話金鑰，credentials 為空。伺服器不得將金鑰傳回小程式，也不能將它當作應用存取權杖。

每次呼叫獨立交換 code，不快取身分、不自動重試；code 的有效期與單次使用由微信驗證，失效後重新呼叫 `wx.login()`。無網頁跳轉、OAuth state、access token 或 refresh token；暱稱、頭像及手機號碼授權須另外處理。OpenID 依小程式 AppID 區分，不自動與公眾號／網站帳戶合併。微信錯誤碼、缺少身分／金鑰、非法 JSON 與非成功 HTTP 回應均傳回錯誤；預設逾時 5 秒。應用負責註冊並保護登入介面、限制請求頻率，正式使用前須以真實小程式憑證完成真機聯調。

## 認證 {#authentication}

在應用 web 群組註冊跳轉和回呼 GET 路由。下方回呼傳回遠端身分；映射應用使用者及登入本機 Guard 由應用完成。遠端帳戶按供應商和供應商 ID 識別。

```rust
use openrust::{
    contracts::container::Container as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_socialite::SocialiteManager;

pub fn redirect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let response = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .redirect().await.map_err(|_| HttpError::InvalidRequest)?;
        Ok(response.into())
    })
}

pub fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let user = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .user().await.map_err(|_| HttpError::InvalidRequest)?;
        Response::json(StatusCode::OK, serde_json::json!({
            "id": user.get_id(), "name": user.get_name(), "email": user.getEmail(),
        }))
    })
}
```

`driver("github", request)` 與 `with("github", request)` 借用目前請求，使用者快取僅存於該借用期，不在 manager 全域快取。`redirect` 傳回原生跳轉回應。Session 儲存隨機 40 字元 state，回呼先消費它再交換 code；缺少、不符、陣列、空值及重播均失敗。`enablePKCE()` 可選；在跳轉與回呼兩次 provider 上同時啟用，儲存 96 字元 verifier 並傳送 S256 challenge 與 verifier。

manager 與 facade 回傳 `Provider`，共同入口為 `redirect().await?` 和 `user().await?`。OAuth1 跳轉前需透過 HTTP 取得臨時憑證，因此統一跳轉入口為非同步。使用 `scopes`、`stateless`、`refreshToken` 等 OAuth2 方法前，執行 `manager.driver("github", request)?.oauth2()?`；Twitter 的 `scope` 和 `userFromTokenAndSecret` 則使用 `.oauth1()?`。協定不符明確回傳錯誤。具體 OAuth2 provider 仍保留同步 `redirect()`。

Twitter OAuth1 使用可選 `services.twitter.scope`／`TWITTER_SCOPE` 或 `.scope("write")`，與 OAuth2 的 `scopes` 分開。scope 非空且不等於 `"0"` 時，臨時憑證請求透過 query 傳送 `x_auth_access_type`。Session `oauth.temp` 儲存臨時憑證；回呼要求同時存在 `oauth_token` 和 `oauth_verifier`，HTTP 交換前嚴格核對臨時識別碼，讀取但不消費儲存的憑證。臨時 token 的單次使用由遠端伺服器執行。此跳轉／回呼流程要求 Session，OAuth1 不提供 `stateless()`。

## 可選參數 {#optional-parameters}

跳轉或交換 code 前呼叫 `provider.with(serde_json::json!({"prompt":"select_account","login_hint":"alice@example.test"}).as_object().unwrap().clone())`。參數按固定上游順序覆蓋普通欄位，支援字串、布林和數字純量。`redirectUrl` 修改目前 provider 的回呼。

Slack identity 預設使用 `user_scope`，同時將 `scope` 設為空。在跳轉和回呼的 provider 都執行 `provider.asBotUser()` 後，使用 `scope` 及 token 回應頂層憑據；普通使用者憑據取自 `authed_user`。預設 user-scope 欄位在 `with` 參數後套用，與上游一致。`BitbucketProvider` 另有顯式 `provider.getAccessToken(code).await?` 路徑，會追加 HTTP Basic 客戶端認證；普通 `user()` 使用繼承的 token 回應路徑。

## 存取範圍 {#access-scopes}

GitHub 預設 `user:email`。`scopes(["read:user".into()])` 去重合併，`setScopes(vec!["read:user".into()])` 替換；`getScopes()` 讀取清單。GitHub 使用逗號分隔。包含 `user:email` 時查詢 `/user/emails` 並選擇 primary 且 verified 的信箱，失敗或無匹配傳回 `None`；移除該 scope 則保留 profile 的信箱。

GitLab、Bitbucket、LinkedIn 兩個驅動以空格分隔 scopes；Slack identity 用逗號，Slack OpenID 用空格。Bitbucket 選擇型別為 `email` 且 primary、confirmed 的信箱，信箱介面失敗傳回 `None`。舊式 LinkedIn 分別載入本地化資料和信箱，含 `r_liteprofile` 時請求 `vanityName`，信箱請求失敗正常傳播。

## 無狀態認證 {#stateless-authentication}

呼叫 `user()` 前執行 `provider.stateless()`，適用於呼叫方明確在 Session 之外管理認證狀態的情境。它關閉 state 校驗，不會建立本機使用者或 Session。啟用 PKCE 後仍需 Session，與上游兩個獨立開關一致。

X／Twitter OAuth2 在重新導向和回呼預設啟用 PKCE，因此即使 `stateless()` 也需要 Session。預設 scope 為 `users.read users.email tweet.read`，授權查詢使用 RFC3986 編碼。stateless 模式在可選參數之後寫入字面 `state=state`，回呼仍關閉 state 檢查。授權碼交換同時使用 HTTP Basic 與表單 client secret；更新使用 Basic，表單只有 `grant_type`、`refresh_token`、`client_id`，不含表單 client secret。使用者映射讀取 `data.confirmed_email`，不採用未驗證電子郵件欄位。

## 讀取使用者詳情 {#retrieving-user-details}

使用者提供 `get_id`、`getNickname`、`get_name`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 數字／字串型別。`userFromToken(token)` 使用既有 token 讀取身分，`refreshToken(token)` 交換更新權杖。憑據顯式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字串使用 `Zeroizing<String>`，不會自動序列化或由 Debug 輸出。遠端 access token 不是 Passport／Sanctum 憑據。

舊式 LinkedIn 增加 `first_name`、`last_name`、`avatar_original`，按偏好 locale 和上游嚴格的 100／800 像素選擇圖片。LinkedIn OpenID 增加 given／family name、`email_verified` 和原始圖片；Slack 兩種驅動暴露 `organization_id`。`linkedin-openid`、`slack-openid` 按固定套件使用 access token 存取固定 UserInfo 介面，不驗證 ID Token、不執行 Discovery，也不是完整 OIDC RP；這些能力使用 [OIDC 套件](oidc.md)。

Google 按固定驅動將長度大於 100 位元組且恰有兩個句點的 token 識別為 JWT。它讀取固定 Google JWKS 端點，透過採用 AWS-LC 的 `jsonwebtoken` 11.1.0 校驗金鑰演算法和簽章，要求 issuer 嚴格等於 `https://accounts.google.com`，audience 為與 client ID 相同的單一字串。存在的 `exp`／`nbf` 與 `iat` 回退按固定 PHP NumericDate 行為驗證。JWT 驗證失敗不會退回 Bearer UserInfo。此 Socialite 路徑按上游接受缺少 `exp`、數字日期字串與 null 日期，不執行 Discovery、不要求 nonce，也不是完整 OIDC RP；完整 RP 校驗使用 [OIDC 套件](oidc.md)。Google raw 資料還補入 `id`、`verified_email`、`link`，並映射 `avatar_original`。

Facebook Limited Login 從未驗證的 token header 偵測非 null 的 `kid`，隨後要求所選 Facebook JWKS 金鑰的 RS256 簽章、嚴格的 `https://www.facebook.com` issuer、單一 client audience 與匹配 nonce。在 `userFromToken(token).await?` 或回呼 `user()` 前呼叫 `provider.withNonce(expected)`；未明確指定時，從 `with` 參數的純量 `nonce` 讀取期待值。明確的 `withNonce` 優先，nonce 必須來自應用保存的授權嘗試。`withNonce` 只設定期待值，不自動產生、儲存或加入授權 nonce。簽章／宣告／nonce 失敗不會退回 Graph。請求選項與 `lastToken()` 留在目前 provider，保留的 token 使用 zeroize 清理。此行為對應固定 Socialite Limited Login（日期可選），不替代 完整的 [OIDC RP 協定](oidc.md)。

### 登入已綁定的本機帳戶

以穩定的 `(provider, provider_user_id)` 尋找既有本機帳戶。namespace 來自可信應用設定，識別供應商／issuer；不同安裝或租戶使用不同 namespace。遠端 ID 作為不透明識別碼保留，`007`、`7`、`ABC` 和 `abc` 可以代表不同帳戶。已驗證信箱本身不授權綁定帳戶。此範例拒絕未知綁定，應用程式透過獨立的認證與驗證流程處理註冊或綁定。

建立二進位唯一鍵與外鍵表。`user_id` 型別須與本機 `users.id` 相同，以下採用預設無號 BIGINT 主鍵：

```sql
CREATE TABLE social_identities (
    provider VARBINARY(128) NOT NULL,
    provider_user_id VARBINARY(255) NOT NULL,
    user_id BIGINT UNSIGNED NOT NULL,
    UNIQUE (provider, provider_user_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
```

將以下動作放入應用動作目錄，在回呼 `user().await?` 成功後呼叫 `login_bound_user(request, "github", &user).await?`。回傳 false 時回應 403，否則跳轉至固定本機帳戶頁。向 `AuthManager` 註冊既有使用者模型，並為 `auth.guards.web` 設定其 Eloquent provider。動作使用原生 Session Guard，登入時輪換 Session 識別碼並替換 CSRF token。綁定查詢不建立帳戶、不更新信箱、不儲存遠端 access token。

```rust
use openrust::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::DB,
    http::{HttpError, Request},
};
use openrust_socialite::two::User;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Application recipe: existing bindings only; namespace comes from trusted application configuration.
pub async fn login_bound_user(
    request: &mut Request,
    namespace: &str,
    remote: &User,
) -> Result<bool, HttpError> {
    let subject = match remote.get_id() {
        serde_json::Value::String(id) if !id.is_empty() => id.clone(),
        serde_json::Value::Number(id) if id.is_i64() || id.is_u64() => id.to_string(),
        _ => return Ok(false),
    };
    if subject.len() > 255 {
        return Ok(false);
    }
    let Some(binding) = DB::table("social_identities")
        .map_err(internal)?
        .whereExactText("provider", namespace)
        .whereExactText("provider_user_id", &subject)
        .first()
        .await
        .map_err(internal)?
    else {
        return Ok(false);
    };
    let id = AuthIdentifier::try_from(binding.get("user_id").ok_or(HttpError::InvalidRequest)?)
        .map_err(internal)?;
    {
        let mut guard = Auth::session_guard(request, Some("web")).map_err(internal)?;
        if guard
            .login_using_id(&id, false)
            .await
            .map_err(internal)?
            .is_none()
        {
            return Ok(false);
        }
    }
    request.session_mut()?.regenerate_token()?;
    Ok(true)
}
```

接入多個供應商時，在跳轉時儲存所選 namespace，回呼前消費並核對；為各供應商註冊固定路由，讓應用程式控制回呼 namespace。保留正常 state 校驗，不接受回呼傳入本機使用者 ID。本機登出使用受 CSRF 保護的 POST 路由，執行 `Auth::session_guard(request, Some("web"))?.logout_and_invalidate().await?`，結束目前應用 Session；供應商 token 撤銷與聯邦登出遵循各自協定。


## 測試 {#testing}

透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。

Socialite 使用共用客戶端顯式 `redact_payload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。

`ProviderEndpoints::new(authorize, token, user).email(email)` 可傳給新增驅動的 `withEndpoints` 建構器，顯式替換端點。

使用供應商授權前，先註冊應用程式和回呼並設定憑證。測試登入流程時使用專用帳戶。

`GoogleProvider::withEndpoints(endpoints, jwks)` 用於受控 UserInfo 與 JWKS 服務。

`FacebookProvider::withEndpoints(endpoints, jwks)` 為受控測試替換三個 Graph／授權端點與 JWKS；包含 `/v23.0/` 的路徑隨 `usingGraphVersion` 變化。

Twitter OAuth1 傳送 HMAC-SHA1 簽章請求，存取憑證另含明確的 `user.credentials.tokenSecret`。`userFromTokenAndSecret(token, secret).await?` 直接呼叫 profile。回呼映射包含 URL 欄位、location、description 與 `avatar_original`；憑證直取僅保留 League extra 欄位，與上游一致。成功 profile 按憑證對快取於目前請求內，任一憑證變化就重新取得。`one::TwitterProvider::withEndpoints` 與 `manager.extendOAuth1` 提供受控端點。
