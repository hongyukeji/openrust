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
