# OpenID Connect（擴充）

## 安裝 {#installation}

可選 `openrust-oidc` 套件提供 OpenID Connect RP，支援授權碼＋PKCE S256、Discovery、JWKS 與 UserInfo。啟用根 Cargo 特性 `oidc` 並重建，套件探索會註冊 `OidcManager` 與 `Oidc` facade。應用程式自行註冊固定 Web 登入與回呼路由，預設不開放端點。Passport 保持 OAuth2 授權伺服器範圍，不自動成為 OIDC Provider。

```sh
cargo +1.94.0 build --locked --features oidc --bin main
```

啟用套件時 `oidc.providers` 預設為空。需要明確選用 `work` 範例時，執行 `./artisan vendor:publish --tag=oidc-config`，在 `config/mod.rs` 的模組宣告與設定映射中分別加入 `mod oidc;` 與 `("oidc".to_owned(), oidc::load(env)?),`，然後重新建置；也可明確執行 `init --extensions=oidc` 發布並登記此範本。發布預設保留既有檔案，傳入 `--force` 才覆寫。

## 供應商設定 {#configuration}

發布後的 `config/oidc.rs` 範本提供 `oidc.providers.work`。設定 `OIDC_ISSUER`、`OIDC_CLIENT_ID`、`OIDC_CLIENT_SECRET` 與 `OIDC_REDIRECT_URI`；`OIDC_REPLAY_STORE` 選擇重播快取。未設定 secret 時使用公共用戶端認證，否則預設 `client_secret_basic`。需要時明確將 `client_auth` 設為 `client_secret_post` 或 `none`。登記回呼 URL 必須精確一致。設定範例：

```json
{
"providers": {
    "work": {
        "issuer": "https://identity.example.com",
        "client_id": "registered-client",
        "client_secret": "read-from-environment",
        "redirect": "https://app.example.com/oidc/callback",
        "scopes": ["openid", "profile", "email"],
        "algorithms": ["RS256"],
        "replay_store": "database"
    }
}
}
```

## 授權與回呼 {#authorization}

登入與固定回呼路由使用相同設定名稱。`redirect()` 與 `user()` 均為非同步方法；provider 持有請求可變借用，每次回呼新建 provider。`prompt("login")` 請求重新認證，`maxAge(60)` 要求足夠近期的 `auth_time`。相同 Session 為同名 provider 新建授權會替換上一次嘗試。

```rust,ignore
use openrust_oidc::facades::Oidc;

// Inside an async handler; convert errors to your application's HTTP errors.
let response = Oidc::provider("work", request)?.redirect().await?;

// Inside the fixed callback handler.
let authentication = Oidc::provider("work", request)?.user().await?;
let identity = &authentication.identity;
```

## 已驗證身分 {#identity}

`Authentication.identity` 是經過簽章及宣告校驗後才能建構的 `VerifiedIdentity`。`issuer()` 與 `subject()` 組成穩定身分對，另提供 `email()`、`emailVerified()`、`claims()` 和 `userInfo()`。UserInfo 的 subject 必須精確等於已驗證 subject。信箱改變時不繼承 ID Token 的驗證標記，須由本次 UserInfo 自己給出布林 `email_verified: true`。`Authentication.tokens` 明確暴露零化的 access／ID／可選 refresh token 與可選到期秒數，不實作 Debug／Serialize。refresh-token grant 不屬於此 RP 用戶端 API。

## 本機帳戶綁定 {#local-binding}

按二進位 `(issuer, subject)` 尋找既有本機帳戶，不按信箱自動合併，不接受回呼傳入本機使用者 ID。範例使用預設無號 BIGINT 使用者主鍵；未知綁定回傳 false，註冊與綁定採用獨立的已認證流程。將以下動作放入應用程式，向 `AuthManager` 註冊既有 Eloquent 使用者模型，在 `user()` 成功後執行 `login_bound_user(request, &authentication.identity).await?`，false 回應 403。登入使用原生 Session Guard，輪換 Session 識別碼和 CSRF token。

```sql
CREATE TABLE oidc_identities (
    issuer VARBINARY(2048) NOT NULL,
    subject VARBINARY(255) NOT NULL,
    user_id BIGINT UNSIGNED NOT NULL,
    UNIQUE (issuer, subject),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
```

```rust
use openrust::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::DB,
    http::{HttpError, Request},
};
use openrust_oidc::VerifiedIdentity;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Application recipe: existing bindings only; issuer and subject come only from the verified identity.
pub async fn login_bound_user(
    request: &mut Request,
    remote: &VerifiedIdentity,
) -> Result<bool, HttpError> {
    let Some(binding) = DB::table("oidc_identities")
        .map_err(internal)?
        .whereExactText("issuer", remote.issuer())
        .whereExactText("subject", remote.subject())
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

## 校驗邊界 {#validation}

RP 要求 HTTPS、精確 issuer、用戶端 audience、非空且不超過 255 位元組的 ASCII subject、nonce、`exp` 和 `iat`。多個 audience 要求匹配 `azp`，只要存在 `azp` 就必須匹配。校驗 `nbf`、可選 `at_hash`／`c_hash` 及請求的 `auth_time`。要求非空且唯一的 `kid`，核對 JWK 用途與演算法，僅接受設定的非對稱演算法。預設 RS256，可明確選 RS384/512、PS256/384/512、ES256/384；拒絕 HMAC、無簽章 token、token 攜帶的金鑰或遠端金鑰 URL。

Discovery 端點預設限於 issuer 同源。確需跨源時在 `allowed_origins` 明確列出所需 HTTPS origin，最多額外 16 項。拒絕帶使用者憑證／fragment 的 URL，以及預含保留查詢參數的授權端點。不跟隨 HTTP 重新導向，回應上限 1 MiB。`allow_loopback_http: true` 僅為本機測試放行字面 IPv4／IPv6 回環 HTTP，不放行主機名稱 HTTP。預設授權 TTL 600 秒（1–3600）、單次請求逾時 10 秒（1–60）、ID Token 最大年齡 600 秒（0–86400）、時鐘寬限 0 秒（0–60）、啟用 UserInfo。多程序／副本必須選擇共用原子資料庫或 Redis 快取；array 僅保護單一程序。失敗回呼也會消費嘗試，重試須重新登入。本機 Session 登出不會撤銷 OP token 或結束 OP Session。

## 瀏覽器 SSO 與登出 {#sso-logout}

為每個 RP 分別設定用戶端 ID、應用金鑰、資料庫和本地使用者綁定。首次在 OP 登入後，第二個 RP 可藉助既有 OP 瀏覽器 Session 授權；每個 RP 仍擁有獨立本地 Session，並分別解析 `(issuer, subject)`。`prompt("login")` 請求 OP 重新認證，`maxAge` 另外檢查傳回的 `auth_time`。

RP 發起登出遵循 OpenID Connect RP-Initiated Logout 1.0（2022-09-12）。將 `OIDC_POST_LOGOUT_REDIRECT_URI`（`post_logout_redirect`）設為 OP 已登記的固定回呼。在 `user()` 及本地帳戶綁定成功後，呼叫 `Oidc::provider("work", request)?.rememberLogout(&authentication)?`，明確將已驗證的 ID Token 儲存到伺服器端 Session。`LogoutHint` 是不透明型別，修改公開 token 欄位無法替換擷取的已驗證提示。Session 儲存和送出的登出 URL 均視為敏感資料。

將以下動作放入應用，並註冊為原生 web 群組中受 CSRF 保護的 POST 路由。傳入 `false` 僅本地登出，`true` 嘗試 OP 登出：

```rust
use openrust::{
    auth::Auth,
    http::{HttpError, Request, Response, StatusCode},
};
use openrust_oidc::facades::Oidc;
use serde_json::json;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Register on a POST route in the native web group with CSRF protection.
pub async fn logout(request: &mut Request, federated: bool) -> Result<Response, HttpError> {
    let hint = Oidc::provider("work", request)
        .map_err(internal)?
        .takeLogoutHint()
        .ok();
    Auth::session_guard(request, Some("web"))
        .map_err(internal)?
        .logout_and_invalidate()
        .await
        .map_err(internal)?;
    if federated
        && let Some(hint) = hint
        && let Ok(url) = Oidc::provider("work", request)
            .map_err(internal)?
            .logoutUrl(&hint)
            .await
    {
        return Ok(
            openrust::support::facades::Redirect::away(&url, Default::default())
                .map_err(internal)?
                .into(),
        );
    }
    Response::json(StatusCode::OK, json!({"local":true,"federated":false}))
}
```

`takeLogoutHint()` 先消耗提示，原生 `logout_and_invalidate()` 再清空本地 Session 並更新 CSRF。`logoutUrl(&hint)` 探索並校驗獲准的 `end_session_endpoint`，附加 ID Token 提示、用戶端 ID、固定傳回位址和一次性 state，將狀態儲存到新的匿名 Session。此動作在 OP 不支援、Discovery 無法使用或中繼資料無效時仍完成本地登出。不得從請求參數讀取提示或傳回目標。

固定傳回處理器呼叫 `Oidc::provider("work", request)?.completeLogout().await?`，再傳回本地已登出頁面。完成時校驗 Session 綁定、設定、目標、期限與共用原子重放宣告；失敗同樣消耗本次嘗試，且不會登入任何使用者。此結果確認傳回狀態，不證明全部遠端憑證均已撤銷。

OP 瀏覽器 Session 結束後，另一個 RP 的既有本地 Session 仍有效。既有 access／refresh token 遵循 OP 獨立的撤銷策略。本實作不提供 front-channel／back-channel 登出通知及全域單點登出。


## 測試與部署 {#testing}

在身分提供方註冊應用程式和回呼，設定 issuer 與用戶端憑證，並使用獲該應用程式授權的帳戶。多個 RP 副本須共用 Session 儲存及支援原子操作的資料庫或 Redis 快取，防止回呼重播。
