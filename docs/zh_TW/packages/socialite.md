# Socialite

Socialite 將第三方登入接入應用程式的 HTTP Client 與 Session。已驗收範圍現包括固定 Socialite 5.33.0 對應的 GitHub、GitLab、Bitbucket、LinkedIn（舊式／OpenID）、Slack（identity／OpenID）、X／Twitter OAuth2、Google 及 Facebook。Twitter OAuth1 與下方本機帳戶登入範例同樣可用。完整 OIDC RP 與 SSO 由 API-O01／02 獨立交付。

## 安裝 {#installation}

建置或執行應用程式時啟用可選 Cargo feature `socialite`。套件探索註冊 `SocialiteServiceProvider` 與應用內的 `SocialiteManager`，不會自動建立路由或本機使用者資料表。

```sh
cargo run --features socialite -- artisan serve
```

## 設定 {#configuration}

透過 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 設定 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 開頭的回呼拼接到合法絕對 HTTP(S) `app.url`，絕對回呼保留原值。`services.github.scopes` 向驅動預設值追加 scope。缺少或無效設定明確失敗。真實供應商驗收需要已註冊的 GitHub OAuth 應用程式和對應回呼。


以下驅動同樣設定 client id、client secret 和 redirect；環境變數前綴後接 `_CLIENT_ID`、`_CLIENT_SECRET`、`_REDIRECT_URI`：

| 驅動／services 鍵 | 環境變數前綴 | 預設 scopes |
| --- | --- | --- |
| `gitlab` | `GITLAB` | `read_user` |
| `bitbucket` | `BITBUCKET` | `email` |
| `linkedin` | `LINKEDIN` | `r_liteprofile r_emailaddress` |
| `linkedin-openid` | `LINKEDIN_OPENID` | `openid profile email` |
| `slack` | `SLACK` | `identity.basic,identity.email,identity.team,identity.avatar` |
| `slack-openid` | `SLACK_OPENID` | `openid email profile` |

`services.gitlab.host`／`GITLAB_HOST` 預設 `https://gitlab.com`。`GitlabProvider::setHost` 保留自託管路徑前綴並移除結尾斜線。固定上游資料路徑為 `/api/v3/user`，本批保留此契約。Slack 登入設定與既有 `slack.notifications` 共處同一 services 物件；真實供應商仍需分別註冊回呼並取得權限。

`x` 使用 `X_CLIENT_ID`、`X_CLIENT_SECRET`、`X_REDIRECT_URI`。`twitter` 使用對應的 `TWITTER_` 變數，並設定 `TWITTER_OAUTH=2` 選擇 OAuth2；預設值 1 選擇 Twitter OAuth1；只有整數 2 選擇 OAuth2，字串 `"2"` 仍選擇 OAuth1。明確的 `twitter-oauth-2` 驅動先讀取 `services.twitter`，僅在物件缺失或為 null 時回退 `services.twitter-oauth-2`。`x` 同樣從 `services.x` 回退 `services.x-oauth-2`；已存在但不完整的設定會失敗，不靜默改用另一組憑證。

`google` 讀取 `GOOGLE_CLIENT_ID`、`GOOGLE_CLIENT_SECRET`、`GOOGLE_REDIRECT_URI`，預設 scope 為 `openid profile email`。一般 access token 以 Bearer 呼叫 Google UserInfo，傳送 `prettyPrint=false`。更新回應僅在缺少 refresh token 欄位時保留傳入值；明確的 null 或輪換值按回應保留。

`facebook` 使用 `FACEBOOK_CLIENT_ID`、`FACEBOOK_CLIENT_SECRET`、`FACEBOOK_REDIRECT_URI`，預設 scope 為 `email`，固定 Graph 版本為 `v23.0`。`provider.usingGraphVersion("v99.0")` 修改目前請求的授權／token／profile 路徑。`fields(vec!["id".into(), "name".into()])` 替換 Graph 欄位；`asPopup()`、`reRequest()` 覆寫相應可選授權參數。Graph 請求傳送 `access_token`、fields，並在 client secret 非空且不等於 `"0"` 時傳送 HMAC-SHA256 `appsecret_proof`。授權碼交換不指定 Accept，回傳保留 `expires`，僅在 `expires_in` 缺失或為 null 時補入，不覆蓋已有值。

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
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let response = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .redirect().await.map_err(|_| HttpError::InvalidRequest)?;
        Ok(response.into())
    })
}

pub fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let user = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .user().await.map_err(|_| HttpError::InvalidRequest)?;
        Response::json(StatusCode::OK, serde_json::json!({
            "id": user.getId(), "name": user.getName(), "email": user.getEmail(),
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

使用者提供 `getId`、`getNickname`、`getName`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 數字／字串型別。`userFromToken(token)` 使用既有 token 讀取身分，`refreshToken(token)` 交換更新權杖。憑據顯式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字串使用 `Zeroizing<String>`，不會自動序列化或由 Debug 輸出。遠端 access token 不是 Passport／Sanctum 憑據。


舊式 LinkedIn 增加 `first_name`、`last_name`、`avatar_original`，按偏好 locale 和上游嚴格的 100／800 像素選擇圖片。LinkedIn OpenID 增加 given／family name、`email_verified` 和原始圖片；Slack 兩種驅動暴露 `organization_id`。`linkedin-openid`、`slack-openid` 按固定套件使用 access token 存取固定 UserInfo 介面，不驗證 ID Token、不執行 Discovery，也不是完整 OIDC RP；這些屬於 API-O01。

Google 按固定驅動將長度大於 100 位元組且恰有兩個句點的 token 識別為 JWT。它讀取固定 Google JWKS 端點，透過採用 AWS-LC 的 `jsonwebtoken` 11.1.0 校驗金鑰演算法和簽章，要求 issuer 嚴格等於 `https://accounts.google.com`，audience 為與 client ID 相同的單一字串。存在的 `exp`／`nbf` 與 `iat` 回退按固定 PHP NumericDate 行為驗證。JWT 驗證失敗不會退回 Bearer UserInfo。此 Socialite 路徑按上游接受缺少 `exp`、數字日期字串與 null 日期，不執行 Discovery、不要求 nonce，也不是完整 OIDC RP；API-O01 有獨立協定範圍。Google raw 資料還補入 `id`、`verified_email`、`link`，並映射 `avatar_original`。

Facebook Limited Login 從未驗證的 token header 偵測非 null 的 `kid`，隨後要求所選 Facebook JWKS 金鑰的 RS256 簽章、嚴格的 `https://www.facebook.com` issuer、單一 client audience 與匹配 nonce。在 `userFromToken(token).await?` 或回呼 `user()` 前呼叫 `provider.withNonce(expected)`；未明確指定時，從 `with` 參數的純量 `nonce` 讀取期待值。明確的 `withNonce` 優先，nonce 必須來自應用保存的授權嘗試。`withNonce` 只設定期待值，不自動產生、儲存或加入授權 nonce。簽章／宣告／nonce 失敗不會退回 Graph。請求選項與 `lastToken()` 留在目前 provider，保留的 token 使用 zeroize 清理。此行為對應固定 Socialite Limited Login（日期可選），不替代 API-O01 的完整 RP 協定。

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
    let subject = match remote.getId() {
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
        let mut guard = Auth::sessionGuard(request, Some("web")).map_err(internal)?;
        if guard
            .loginUsingId(&id, false)
            .await
            .map_err(internal)?
            .is_none()
        {
            return Ok(false);
        }
    }
    request.session_mut()?.regenerateToken()?;
    Ok(true)
}
```

接入多個供應商時，在跳轉時儲存所選 namespace，回呼前消費並核對；為各供應商註冊固定路由，讓應用程式控制回呼 namespace。保留正常 state 校驗，不接受回呼傳入本機使用者 ID。本機登出使用受 CSRF 保護的 POST 路由，執行 `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`，結束目前應用 Session；供應商 token 撤銷與聯邦登出遵循各自協定。

MySQL 8.4 驗收應用使用上述動作、真實 Eloquent 使用者、資料庫 Session、HTTP token／profile 端點和兩個隔離 schema。涵蓋同信箱不合併、供應商／subject 大小寫及數值區別、並行登入、唯一綁定、固定 Session 拒絕、恢復、CSRF／登出輪換、跨應用 Cookie、缺少／刪除帳戶、錯誤／重播 state、遠端撤銷及資料庫失敗。此為受控供應商應用整合驗收，真實供應商帳戶授權仍需要憑證與登記回呼。

## 測試 {#testing}

透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。驗收執行真實 HTTP、兩個獨立 Session 應用、PKCE、state 消費、scope／參數映射、已驗證信箱、更新和失敗路徑，固定 PHP 原始碼 SHA 保護對照夾具。

Socialite 使用共用客戶端顯式 `redactPayload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。受控驗收不冒稱真實 GitHub 登入，後者仍需要 OAuth 應用、憑據、回呼網域和帳戶。


`ProviderEndpoints::new(authorize, token, user).email(email)` 可傳給新增驅動的 `withEndpoints` 建構器，顯式替換端點。各驅動均以真實受控 HTTP 驗收 token 表單、認證標頭、資料查詢參數、可選信箱、Slack user／bot 憑據、refresh 與失效／撤銷憑據拒絕。六個驅動的映射、scope 及 Slack 專屬欄位與真實固定 PHP 方法對照。受控結果不冒稱已登入真實 GitLab、Bitbucket、LinkedIn 或 Slack 帳戶。

X／Twitter 測試使用真實 HTTP 與加密 Session Cookie，涵蓋預設 PKCE、跨應用拒絕、state 重放、RFC3986 參數、Basic 交換／更新、錯誤回應及遠端撤銷。映射、欄位和 HTTP 選項以固定 PHP 方法提供對照。真實 X／Twitter 授權仍需供應商應用憑證、登記回呼和帳戶。

`GoogleProvider::withEndpoints(endpoints, jwks)` 用於受控 UserInfo 與 JWKS 服務。真實 HTTP 測試使用原 PHP Socialite／Firebase JWT 方法對照 25 組簽章 JWT，包括錯誤簽章、issuer／audience、缺失或錯誤宣告、未來／過期時間、演算法混淆、金鑰輪換和跨 client 隔離。PEM 測試金鑰僅屬於這些夾具。真實 Google 帳戶授權仍需測試應用及回呼。

`FacebookProvider::withEndpoints(endpoints, jwks)` 為受控測試替換三個 Graph／授權端點與 JWKS；包含 `/v23.0/` 的路徑隨 `usingGraphVersion` 變化。21 組原 PHP Limited Login 對照涵蓋 nonce 優先級、issuer／audience、簽章／過期、缺失金鑰及輪換；實際 Graph HTTP 驗證 HMAC proof、欄位、過期轉換、請求隔離及去識別觀察。真實 Facebook 授權仍需應用、憑證、登記回呼及帳戶。

Twitter OAuth1 傳送 HMAC-SHA1 簽章請求，存取憑證另含明確的 `user.credentials.tokenSecret`。`userFromTokenAndSecret(token, secret).await?` 直接呼叫 profile。回呼映射包含 URL 欄位、location、description 與 `avatar_original`；憑證直取僅保留 League extra 欄位，與上游一致。成功 profile 按憑證對快取於目前請求內，任一憑證變化就重新取得。`one::TwitterProvider::withEndpoints` 與 `manager.extendOAuth1` 提供受控端點。固定 League 1.12.0／Socialite PHP 對照及真實 HTTP／Session 測試涵蓋簽章、空 verifier、scope、憑證快取、跨應用拒絕、遠端重播拒絕和失敗去敏；真實 Twitter 授權仍需要應用程式、憑證、登記回呼及帳戶。
