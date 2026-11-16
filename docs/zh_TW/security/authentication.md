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

## HTTP Basic 驗證 {#http-basic-authentication}

在 Web 路由上使用 `auth.basic`，透過工作階段 Guard 驗證。預設以 `email` 尋找使用者。可用 `auth.basic:web,username` 指定 Guard 和查找欄位。憑證缺少或無效時，回應為 HTTP 401，並附上 `WWW-Authenticate: Basic`。已有效的工作階段也可通過中介軟體。

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_basic_routes() -> Result<(), RoutingError> {
    Route::getWith("/profile", || view("profile"), |route| {
        route.middleware_name("auth.basic");
        Ok(())
    })?;
    Route::getWith("/profile/by-name", || view("profile"), |route| {
        route.middleware_name("auth.basic:web,username");
        Ok(())
    })
}
```

在 API 處理常式中呼叫 `Auth::onceBasic(request, "email", &[]).await?` 可以只驗證目前請求。請透過 HTTPS 傳送 Basic 憑證。

## 驗證事件 {#authentication-events}

憑證驗證開始時，應用程式派發 `Attempting`，提供者驗證通過後派發 `Validated`。工作階段登入依序派發 `Login` 和 `Authenticated`。讀取現有工作階段派發 `Authenticated`，透過 remember Cookie 恢復工作階段時派發帶有 `remember = true` 的 `Login`。嘗試失敗時派發 `Failed`，其中可能包含提供者找到的使用者。`Logout` 和 `CurrentDeviceLogout` 分別表示一般登出和目前裝置登出。

`Attempting` 和 `Failed` 的同步監聽器可透過 `credentials(context)` 借用憑證。請避免將密碼寫入日誌。`Validated` 監聽器可透過 `user::<T>(context)` 借用具型別的使用者。

登入觸發限流時，`Lockout` 會帶有 Guard 名稱和重試等待時間。同步監聽器可透過 `request(context)` 借用請求中繼資料。

```rust
use std::sync::Arc;

use openrust::{
    auth::events::{Lockout, Login},
    contracts::events::{Dispatcher as _, Event},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
};

fn record_auth_event<'a>(
    event: &'a dyn Event,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        if let Some(login) = event.downcast_ref::<Login>() {
            eprintln!("login guard={} remember={}", login.guard, login.remember);
        } else if let Some(lockout) = event.downcast_ref::<Lockout>() {
            eprintln!("lockout guard={} retry_after={}", lockout.guard, lockout.retry_after);
        }
        Ok(None)
    })
}

fn register_auth_events(dispatcher: &Dispatcher) -> EventResult<()> {
    dispatcher.listen("Login", Arc::new(record_auth_event), Delivery::Sync)?;
    dispatcher.listen("Lockout", Arc::new(record_auth_event), Delivery::Sync)
}
```

## 使其他裝置的工作階段失效 {#invalidating-sessions-on-other-devices}

在需要執行工作階段失效的路由中，將 `auth.session` 放在 `auth` 之後。在已驗證的 Web 處理常式中驗證請求，再呼叫 `Auth::logoutOtherDevices(request, password).await?`。Guard 檢查目前密碼、更新已儲存的雜湊，視需要更新目前裝置的 remember Cookie，並派發 `OtherDeviceLogout`。仍帶有舊指紋的工作階段下次造訪受保護的路由時會被拒絕。

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_account_routes() -> Result<(), RoutingError> {
    Route::getWith("/account", || view("account"), |route| {
        route.middleware_name("auth").middleware_name("auth.session");
        Ok(())
    })
}
```

## 密碼確認 {#password-confirmation}

在敏感的 Web 路由上，將 `password.confirm` 放在 `auth` 之後。中介軟體使用 `auth.password_timeout`，單位為秒，預設 10,800。可用 `password.confirm:password.confirm,300` 覆寫確認路由與期限。需要確認時，瀏覽器會重新導向命名路由，並將原始目的地儲存在工作階段。JSON 請求會收到 HTTP 423。

密碼確認表單透過受 CSRF 保護的 POST 送出。`Auth::confirmPassword` 透過使用者提供者驗證目前帳號密碼，並記錄成功確認。自行驗證密碼後，也可呼叫 `request.session_mut()?.passwordConfirmed()?` 記錄確認。

```rust
use openrust::{
    auth::Auth,
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode, abort},
    routing::{RouteGroup, RoutingError},
    support::facades::{Redirect, Route},
    view::view,
};

fn confirm(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let password = request.input(Some("password")).await?;
        let password = password.as_ref().and_then(serde_json::Value::as_str)
            .ok_or(HttpError::InvalidRequest)?;
        if !Auth::confirmPassword(request, password).await? {
            return Err(abort(StatusCode::UNPROCESSABLE_ENTITY, "Invalid password.", Default::default()));
        }
        Ok(Redirect::intended(request, "/settings", Default::default())?.into())
    })
}

fn register_password_routes() -> Result<(), RoutingError> {
    Route::getWith("/confirm-password", || view("auth.confirm-password"), |route| {
        route.name("password.confirm").middleware_name("auth");
        Ok(())
    })?;
    Route::group(
        RouteGroup::new().middleware_name("auth").middleware_name("throttle:6,1"),
        |router| {
            router.r#match(&[Method::POST], "/confirm-password", confirm)?;
            Ok(())
        },
    )?;
    Route::getWith("/settings", || view("settings"), |route| {
        route.middleware_name("auth").middleware_name("password.confirm");
        Ok(())
    })
}
```

## 自訂 Guard 和提供器

預設應用程式註冊編譯後的 Eloquent `User`。用 `AuthManager::registerModel` 註冊其他模型，或為 `database` 提供器設定資料表與欄位。用 `provider` 註冊提供器，用 `viaRequest` 註冊無狀態請求解析器。內建 `token` Guard 讀取 Bearer 標頭，支援將權杖雜湊後再查詢資料庫。
