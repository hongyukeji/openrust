# Sanctum

## 權杖資料表遷移 {#token-table-migration}

使用 Cargo 的 `sanctum` 功能編譯應用程式，註冊 `personal_access_tokens` 遷移。遷移和復原均使用含有此功能的執行檔。

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

遷移建立多型歸屬欄位、唯一的 64 字元權杖雜湊欄位、可為空值的能力和時間欄位，以及到期時間索引。此功能同時註冊 Sanctum 守衛與 CSRF Cookie 路由。`install:api` 的 API 骨架另有交付批次。

## 簽發個人存取權杖 {#issuing-api-tokens}

在應用程式資料庫上使用 `PersonalAccessTokens` 建立權杖，傳入歸屬型別、歸屬 ID、名稱、能力和選用的到期時間。簽發時將傳回的 `plain_text_token` 交給用戶端。資料庫保存 SHA-256 摘要。`findToken` 查找憑證，`revokeToken` 依歸屬刪除權杖。`can` 支援精確能力及萬用字元 `*`。HTTP 守衛在驗證請求前檢查到期時間。

```rust
use openrust::{auth::AuthResult, database::Connection, sanctum::{NewAccessToken, PersonalAccessTokens}};

async fn issue(connection: Connection, user_id: u64) -> AuthResult<NewAccessToken> {
    PersonalAccessTokens::new(connection, "")
        .createToken("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

## 保護路由與檢查能力 {#protecting-routes}

在受保護的路由加上 `auth:sanctum`，以 `Authorization: Bearer <token>` 傳送憑證。憑證無效、到期或已撤銷時回傳 HTTP 401。`abilities:reports:read,reports:write` 要求具備兩項能力，`ability:reports:read,reports:write` 要求具備任一項。權杖已通過驗證但缺少能力時回傳 HTTP 403。

```rust
use openrust::{http::ApiResponse, routing::RoutingError, support::facades::Route};

fn register() -> Result<(), RoutingError> {
    Route::getWith("/reports", || ApiResponse::success(["quarterly"]), |route| {
        route.middleware_name("auth:sanctum").middleware_name("abilities:reports:read");
        Ok(())
    })
}
```

簽發時以已註冊的 Provider 模型名稱作為權杖歸屬型別。預設應用程式使用 `app::models::User`。可在 `auth.guards` 為自訂 Sanctum 守衛設定 `provider`，省略時使用 Web 守衛的 Provider。以 `AccessToken::currentAccessToken(request)` 檢視目前請求的憑證。`sanctum.expiration` 以分鐘限制權杖最長有效期，與個別權杖的到期時間同時生效。Bearer 驗證成功後派發 `TokenAuthenticated` 事件並更新 `last_used_at`，可設定 `sanctum.last_used_at=false` 關閉。

## SPA 驗證 {#spa-authentication}

在 `bootstrap/app.rs` 現有的 `withMiddleware` 閉包呼叫 `middleware.statefulApi();`。以 `SANCTUM_STATEFUL_DOMAINS` 設定以逗號分隔的第一方前端主機，非預設連接埠須一併填寫。使用預設路由設定時，先請求 `GET /sanctum/csrf-cookie`，再經由應用程式的 Web 工作階段登入入口登入。後續 API 請求攜帶 Cookie。需要 CSRF 檢查時，將 URL 解碼後的 `XSRF-TOKEN` Cookie 值放入 `X-XSRF-TOKEN` 請求標頭。

跨來源第一方前端須為指定來源啟用攜帶憑證的 CORS。符合設定的 Referer 或 Origin 會啟用 Session、Cookie 和 CSRF 中介軟體。Sanctum 優先檢查已驗證的工作階段，再檢查 Bearer 權杖。第一方工作階段會通過權杖能力檢查，使用者操作權限由應用程式授權策略決定。
