# HTTP 回應

## 建立回應

用 `Response::json` 回傳 JSON 資料與明確的狀態碼。透過 `header` 或 `withHeaders` 設定具型別的 HTTP 回應標頭。用 `Response::stream` 從 Rust 串流逐段傳送本文。

輸入讀取及 JSON 回應的完整範例見[控制器](./controllers.md)。

## 向回應附加 Cookie

在包含 `AddQueuedCookiesToResponse` 的中介軟體堆疊中，用 `Cookie::queue` 將 Cookie 加入佇列，處理器回傳後由中介軟體寫入回應。也可以用 `response.cookie(&cookie, request)?` 直接附加。把 `EncryptCookies` 放在佇列中介軟體之前，讓它同時保護直接附加和排入佇列的 Cookie。 工廠從 Session 設定繼承路徑、網域、Secure 和 SameSite 預設值。有效期限設為零會建立瀏覽器工作階段 Cookie。

```rust
use openrust::{
    cookie::CookieOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Cookie,
};

fn remember(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let cookie = Cookie::make("theme", "dark", 30, &CookieOptions::default())?;
        Cookie::queue(cookie)?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

## JSON 業務回應

需要統一業務回應格式的路由可以使用 `ApiResponse`。回應包含 `success`、`code` 和 `message`，按需附帶 `data`、分頁 `meta` 或欄位 `errors`。`success(data)` 預設回傳 HTTP 200。使用 `failure` 時傳入狀態碼，也可以透過 `with_status` 設定。

```rust
use openrust::{
    http::{ApiResponse, PageMeta, StatusCode},
    routing::RoutingError,
    support::facades::Route,
};

fn register() -> Result<(), RoutingError> {
    Route::get("/items", || {
        ApiResponse::success([1, 2]).with_meta(PageMeta {
            page: 2,
            per_page: 2,
            total: 4,
            total_pages: 2,
        })
    })?;

    Route::get("/example-conflict", || {
        ApiResponse::failure(StatusCode::CONFLICT, "conflict", "The item already exists.")
    })
}
```

`ApiResponse::empty()` 回傳省略 `data` 的成功回應。HTTP 204 回應的本文為空。請給 `failure` 傳入可安全顯示給用戶端的訊息，需要回傳欄位錯誤時使用 `with_errors`。

序列化失敗交給[例外處理](errors.md)。內建例外和健康檢查各自使用原有的回應格式。統一業務回應是 OpenRust 在 [Laravel JSON 回應機制](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#json-responses)之上的擴充。

## 重新導向回應

使用 `Redirect::to` 前往 URL，或以 `Redirect::route` 前往命名路由。`RedirectOptions` 預設使用 302，並支援回應標頭。`Redirect::guest` 將存取目標存入目前工作階段，`Redirect::intended` 讀取並清除它。工作階段沒有存取目標時使用備用位址。

用 `Redirect::action` 前往[已註冊的控制器動作](./urls.md#urls-for-controller-actions)。

`Redirect::refresh` 重新導向至移除查詢字串的目前路徑。`Redirect::secure` 從應用程式路徑產生 HTTPS 目的地。`Redirect::away` 將明確的 HTTP(S) URL 或相對參照寫入 `Location`，呼叫時請傳入可信任的目的地。`Redirect::signedRoute` 和 `Redirect::temporarySignedRoute` 重新導向至絕對簽章 URL。`Redirect::back` 依序使用同源 Referer、上一個工作階段 URL 和備用位址。`Redirect::getIntendedUrl` 讀取目前的預期位址，`Redirect::setIntendedUrl` 明確寫入預期位址。兩個方法都接收目前的請求。

路由層級的重新導向可使用 `Router::redirect` 或 `Router::permanentRedirect`，將來源參數代入目的地路徑。目的地的選用區段（例如 `/articles/{slug?}`）若無對應值就會省略。`destination` 和 `status` 是保留的來源參數名稱。

### 重新導向時暫存工作階段資料 {#redirecting-with-flashed-session-data}

在啟用工作階段的 Web 路由中，使用 `with` 暫存值，`withInput` 保留輸入，`withErrors` 附加命名的 `MessageBag`。Rust 方法明確接收目前請求的可變參考。處理常式傳回時，將重新導向轉換為 HTTP 回應。

```rust
use openrust::{
    http::{HttpError, Request, Response},
    support::facades::Redirect,
};
use serde_json::json;

async fn save_profile(request: &mut Request) -> Result<Response, HttpError> {
    let redirect = Redirect::to("/profile", Default::default())?
        .with(request, "status", json!("saved"))?
        .onlyInput(request, &["name"])
        .await?;
    Ok(redirect.into())
}
```

向 `withInput` 傳入 `None` 時，讀取請求的表單或 JSON 欄位及查詢參數。上傳檔案會被排除。使用 `onlyInput` 或 `exceptInput` 依點路徑篩選欄位。下一次請求可用 `request.old(Some("name"))` 讀取舊輸入，並從工作階段讀取暫存值。
