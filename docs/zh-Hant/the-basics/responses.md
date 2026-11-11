# HTTP 回應

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
