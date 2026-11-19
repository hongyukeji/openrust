# Socialite

Socialite 將第三方登入接入應用程式的 HTTP Client 與 Session。已驗收範圍現包括固定 Socialite 5.33.0 對應的 GitHub、GitLab、Bitbucket、LinkedIn（舊式／OpenID）、Slack（identity／OpenID）、X／Twitter OAuth2 及 Google。API-S01 整體仍在開發；Facebook、OAuth1 以及完整 OIDC／本機身分整合待後續交付。

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

`x` 使用 `X_CLIENT_ID`、`X_CLIENT_SECRET`、`X_REDIRECT_URI`。`twitter` 使用對應的 `TWITTER_` 變數，並設定 `TWITTER_OAUTH=2` 選擇 OAuth2；預設值 1 保留上游 OAuth1 選擇，該路徑尚未交付，明確失敗。明確的 `twitter-oauth-2` 驅動先讀取 `services.twitter`，僅在物件缺失或為 null 時回退 `services.twitter-oauth-2`。`x` 同樣從 `services.x` 回退 `services.x-oauth-2`；已存在但不完整的設定會失敗，不靜默改用另一組憑證。

`google` 讀取 `GOOGLE_CLIENT_ID`、`GOOGLE_CLIENT_SECRET`、`GOOGLE_REDIRECT_URI`，預設 scope 為 `openid profile email`。一般 access token 以 Bearer 呼叫 Google UserInfo，傳送 `prettyPrint=false`。更新回應僅在缺少 refresh token 欄位時保留傳入值；明確的 null 或輪換值按回應保留。

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
            .redirect().map_err(|_| HttpError::InvalidRequest)?;
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

## 測試 {#testing}

透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。驗收執行真實 HTTP、兩個獨立 Session 應用、PKCE、state 消費、scope／參數映射、已驗證信箱、更新和失敗路徑，固定 PHP 原始碼 SHA 保護對照夾具。

Socialite 使用共用客戶端顯式 `redactPayload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。受控驗收不冒稱真實 GitHub 登入，後者仍需要 OAuth 應用、憑據、回呼網域和帳戶。


`ProviderEndpoints::new(authorize, token, user).email(email)` 可傳給新增驅動的 `withEndpoints` 建構器，顯式替換端點。各驅動均以真實受控 HTTP 驗收 token 表單、認證標頭、資料查詢參數、可選信箱、Slack user／bot 憑據、refresh 與失效／撤銷憑據拒絕。六個驅動的映射、scope 及 Slack 專屬欄位與真實固定 PHP 方法對照。受控結果不冒稱已登入真實 GitLab、Bitbucket、LinkedIn 或 Slack 帳戶。

X／Twitter 測試使用真實 HTTP 與加密 Session Cookie，涵蓋預設 PKCE、跨應用拒絕、state 重放、RFC3986 參數、Basic 交換／更新、錯誤回應及遠端撤銷。映射、欄位和 HTTP 選項以固定 PHP 方法提供對照。真實 X／Twitter 授權仍需供應商應用憑證、登記回呼和帳戶。

`GoogleProvider::withEndpoints(endpoints, jwks)` 用於受控 UserInfo 與 JWKS 服務。真實 HTTP 測試使用原 PHP Socialite／Firebase JWT 方法對照 25 組簽章 JWT，包括錯誤簽章、issuer／audience、缺失或錯誤宣告、未來／過期時間、演算法混淆、金鑰輪換和跨 client 隔離。PEM 測試金鑰僅屬於這些夾具。真實 Google 帳戶授權仍需測試應用及回呼。
