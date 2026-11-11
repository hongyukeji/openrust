# 身分驗證

## Guard 與使用者提供器

`auth.guards` 設定選擇 Guard 及其提供器。`session` Guard 使用目前 Web 請求的 Session。受保護路由使用 `auth` 中介軟體別名，訪客路由使用 `guest`。透過 `Auth::guard(request, None)?.user().await?` 讀取身分。每個請求獨立儲存已解析身分及選定的 Guard。

## 手動驗證使用者

用應用程式選定的查詢欄位與密碼建立 `Credentials`。`attempt` 驗證憑證，依設定升級密碼雜湊，成功後輪換 Session ID 和 CSRF 權杖。登入處理器可使用 `attemptThrottled`，限流鍵由正規化帳號與可信用戶端位址產生。限流器原子記帳，達到上限回傳帶有 `Retry-After` 的 429，驗證通過後清除計數。

```rust
use openrust::{
    auth::{AuthResult, Credentials},
    http::Request,
    support::facades::Auth,
};

async fn sign_in(
    request: &mut Request,
    credentials: &Credentials,
    throttle_key: &str,
    remember: bool,
) -> AuthResult<bool> {
    Auth::sessionGuard(request, None)?
        .attemptThrottled(credentials, remember, throttle_key, 5, 60)
        .await
}
```

## 記住使用者與登出

將 remember 參數設為 `true` 會簽發加密、HTTP-only 的 Cookie。Guard 使用持久化 remember 權杖和目前密碼雜湊的指紋恢復工作階段。`logout` 清除目前 Guard 並輪換權杖。若應用程式還需捨棄整個工作階段並輪換 CSRF 權杖，使用 `logoutAndInvalidate`。

## 自訂 Guard 和提供器

預設應用程式註冊編譯後的 Eloquent `User`。用 `AuthManager::registerModel` 註冊其他模型，或為 `database` 提供器設定資料表與欄位。用 `provider` 註冊提供器，用 `viaRequest` 註冊無狀態請求解析器。內建 `token` Guard 讀取 Bearer 標頭，支援將權杖雜湊後再查詢資料庫。
