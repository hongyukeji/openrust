# OpenID Connect（擴充）

## 安裝 {#installation}

可選 `openrust-oidc` 套件提供 OpenID Connect RP，支援授權碼＋PKCE S256、Discovery、JWKS 與 UserInfo。啟用根 Cargo 特性 `oidc` 並重建，套件探索會註冊 `OidcManager` 與 `Oidc` facade。應用程式自行註冊固定 Web 登入與回呼路由，預設不開放端點。Passport 保持 OAuth2 授權伺服器範圍，不自動成為 OIDC Provider。

```sh
cargo +1.94.0 build --locked --features oidc --bin main
```

## 供應商設定 {#configuration}

`config/oidc.rs` 提供 `oidc.providers.work`。設定 `OIDC_ISSUER`、`OIDC_CLIENT_ID`、`OIDC_CLIENT_SECRET` 與 `OIDC_REDIRECT_URI`；`OIDC_REPLAY_STORE` 選擇重播快取。未設定 secret 時使用公共用戶端認證，否則預設 `client_secret_basic`。需要時明確將 `client_auth` 設為 `client_secret_post` 或 `none`。登記回呼 URL 必須精確一致。設定範例：

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

## 校驗邊界 {#validation}

RP 要求 HTTPS、精確 issuer、用戶端 audience、非空且不超過 255 位元組的 ASCII subject、nonce、`exp` 和 `iat`。多個 audience 要求匹配 `azp`，只要存在 `azp` 就必須匹配。校驗 `nbf`、可選 `at_hash`／`c_hash` 及請求的 `auth_time`。要求非空且唯一的 `kid`，核對 JWK 用途與演算法，僅接受設定的非對稱演算法。預設 RS256，可明確選 RS384/512、PS256/384/512、ES256/384；拒絕 HMAC、無簽章 token、token 攜帶的金鑰或遠端金鑰 URL。

Discovery 端點預設限於 issuer 同源。確需跨源時在 `allowed_origins` 明確列出所需 HTTPS origin，最多額外 16 項。拒絕帶使用者憑證／fragment 的 URL，以及預含保留查詢參數的授權端點。不跟隨 HTTP 重新導向，回應上限 1 MiB。`allow_loopback_http: true` 僅為本機測試放行字面 IPv4／IPv6 回環 HTTP，不放行主機名稱 HTTP。預設授權 TTL 600 秒（1–3600）、單次請求逾時 10 秒（1–60）、ID Token 最大年齡 600 秒（0–86400）、時鐘寬限 0 秒（0–60）、啟用 UserInfo。多程序／副本必須選擇共用原子資料庫或 Redis 快取；array 僅保護單一程序。失敗回呼也會消費嘗試，重試須重新登入。本機 Session 登出不會撤銷 OP token 或結束 OP Session。

## 測試與部署 {#testing}

受控供應商測試使用真實 TCP HTTP、RSA 簽章、同 kid 輪換、Session 中介層和 MySQL 8.4。兩個 RP 副本在載入相同回呼 Session 後，透過共用資料庫快取原子認領 state，只有一個請求呼叫 token 端點。另涵蓋 PKCE／用戶端認證、宣告／Discovery／UserInfo 失敗、到期或替換嘗試、跨應用 Cookie、本機綁定和同信箱不合併。真實第三方授權需要供應商 issuer、已登記測試應用、用戶端憑證、獲准回呼與測試帳戶。瀏覽器 SSO 和聯邦登出由 API-O02 獨立交付。
