# Socialite

Socialite 將第三方登入接入應用程式的 HTTP Client 與 Session。目前已驗收固定 Socialite 5.33.0 對應的 GitHub 驅動和共用 OAuth2 流程。API-S01 整體仍在開發；其他供應商及 OAuth1／OIDC 專屬路徑待後續交付。

## 安裝 {#installation}

建置或執行應用程式時啟用可選 Cargo feature `socialite`。套件探索註冊 `SocialiteServiceProvider` 與應用內的 `SocialiteManager`，不會自動建立路由或本機使用者資料表。

```sh
cargo run --features socialite -- artisan serve
```

## 設定 {#configuration}

透過 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 設定 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 開頭的回呼拼接到合法絕對 HTTP(S) `app.url`，絕對回呼保留原值。`services.github.scopes` 向驅動預設值追加 scope。缺少或無效設定明確失敗。真實供應商驗收需要已註冊的 GitHub OAuth 應用程式和對應回呼。

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

## 存取範圍 {#access-scopes}

GitHub 預設 `user:email`。`scopes(["read:user".into()])` 去重合併，`setScopes(vec!["read:user".into()])` 替換；`getScopes()` 讀取清單。GitHub 使用逗號分隔。包含 `user:email` 時查詢 `/user/emails` 並選擇 primary 且 verified 的信箱，失敗或無匹配傳回 `None`；移除該 scope 則保留 profile 的信箱。

## 無狀態認證 {#stateless-authentication}

呼叫 `user()` 前執行 `provider.stateless()`，適用於呼叫方明確在 Session 之外管理認證狀態的情境。它關閉 state 校驗，不會建立本機使用者或 Session。啟用 PKCE 後仍需 Session，與上游兩個獨立開關一致。

## 讀取使用者詳情 {#retrieving-user-details}

使用者提供 `getId`、`getNickname`、`getName`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 數字／字串型別。`userFromToken(token)` 使用既有 token 讀取身分，`refreshToken(token)` 交換更新權杖。憑據顯式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字串使用 `Zeroizing<String>`，不會自動序列化或由 Debug 輸出。遠端 access token 不是 Passport／Sanctum 憑據。

## 測試 {#testing}

透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。驗收執行真實 HTTP、兩個獨立 Session 應用、PKCE、state 消費、scope／參數映射、已驗證信箱、更新和失敗路徑，固定 PHP 原始碼 SHA 保護對照夾具。

Socialite 使用共用客戶端顯式 `redactPayload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。受控驗收不冒稱真實 GitHub 登入，後者仍需要 OAuth 應用、憑據、回呼網域和帳戶。
