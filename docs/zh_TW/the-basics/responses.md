# HTTP 回應

## 建立回應

用 `Response::json` 回傳 JSON 資料與明確的狀態碼。透過 `header` 或 `withHeaders` 設定具型別的 HTTP 回應標頭。用 `Response::stream` 從 Rust 串流逐段傳送本文。

輸入讀取及 JSON 回應的完整範例見[控制器](./controllers.md)。

### 附加回應標頭 {#attaching-headers-to-responses}

用 `withoutHeader` 移除一個具型別的回應標頭，用 `withoutHeaders` 批次移除。兩者都回傳回應，可繼續鏈式呼叫。

#### 快取控制中介層 {#cache-control-middleware}

在需要快取策略的路由上使用 `cache.headers`。選項名稱使用 `snake_case`，以分號分隔。中介層處理成功的 GET 和 HEAD 回應。指定 `etag` 時沿用既有驗證器，或以與 Laravel `xxh128` 相同的 XXH3-128 摘要計算回應本文。串流回應可用 `etag=value` 明確提供驗證器，無須緩衝整個串流。

```rust
Route::getWith("/cacheable", cache_response, |route| {
    route.middleware_name("cache.headers:public;max_age=2628000;etag");
    Ok(())
})?;
```

`last_modified` 接受 Unix 時間戳或日期運算式。`If-None-Match` 或 `If-Modified-Since` 相符時可回傳無本文的 `304 Not Modified`。當請求同時提供兩個驗證條件且回應有 ETag 時，優先比較 ETag。

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

## 其他回應類型 {#other-response-types}

### 視圖回應 {#view-responses}

用 `Response::view` 渲染已準備好的視圖，並指定狀態碼與回應標頭。可傳入 `view("name")`，也能用 `View::first` 門面選取第一個可用的範本。渲染保留目前請求的上下文。

### JSONP 回應 {#jsonp-responses}

用 `Response::jsonp(status, Some(callback), data)` 將 JSON 資料傳給 JavaScript 回呼。回呼名稱支援點分名稱與方括號字面值鍵。無效的回呼名稱會回傳 HTTP 400。傳入 `None` 會回傳一般 JSON 回應。

### Markdown 回應 {#markdown-responses}

用 `Response::markdown(status, content)` 傳送 Markdown 原始文字，媒體類型為 `text/markdown; charset=utf-8`。由用戶端決定如何渲染。

### 空回應 {#empty-responses}

用 `Response::noContent()` 回傳無本文的 HTTP 204。用 `Response::empty(status)` 指定其他狀態碼，用 `withHeaders` 附加回應標頭。

### 檔案下載 {#file-downloads}

用 `Response::download(path, name, headers, "attachment").await` 下載本機檔案。傳入 `Some(name)` 指定下載名稱，傳入 `None` 沿用原檔名。使用 `"inline"` 可指定行內處置。

### 檔案回應 {#file-responses}

用 `Response::file(path, headers).await` 回傳本機檔案、媒體類型與長度。兩種方法都將應用程式選定的一般檔案分段傳送，預設採用公開快取策略。

HTTP Kernel 在回應中介層後處理單一位元組區間。`If-Range` 可比對強 ETag 或檔案的 `Last-Modified`。HEAD 回傳完整檔案的中繼資料，本文為空。需要快取重新驗證及 `304 Not Modified` 時，為路由設定 `cache.headers`。格式無效或含多個區間的請求會回傳完整檔案。

## 串流回應 {#streamed-responses}

使用 `Response::stream` 傳送原始資料區塊時，回應會帶上 `X-Accel-Buffering: no`。

### 在瀏覽器中讀取串流回應 {#consuming-streamed-responses}

使用 Fetch 的本文讀取器可以逐段讀取內容。對於串流 JSON 文件，可累積文字，在串流結束後解析。

SSE 可以用 `EventSource` 依事件名稱監聽，收到預設結束標記後關閉連線。

```javascript
const source = new EventSource('/events');
source.addEventListener('snapshot', (event) => {
    renderSnapshot(JSON.parse(event.data));
});
source.addEventListener('update', (event) => {
    if (event.data === '</stream>') source.close();
});
```

### 串流 JSON 回應 {#streamed-json-responses}

透過 `Response::streamJson` 和 `StreamedJson::object` 將資料列串流放進 JSON 物件。`StreamedJson::items` 在每列到達時序列化。固定值使用 `StreamedJson::value`，巢狀陣列使用 `StreamedJson::array`。使用 `StreamedJson::entries` 可將鍵值對逐項寫入 JSON 物件。

串流 JSON 與 JSON 事件資料沿用 Laravel 的預設字串跳脫。SSE JSON 保留無須跳脫的非 ASCII 文字。

```rust
use futures_util::stream;
use openrust::http::{HttpError, Response, StatusCode, StreamedJson};

let users = stream::iter([Ok::<_, HttpError>(1), Ok(2)]);
let data = StreamedJson::object([("users", StreamedJson::items(users))]);
let response = Response::streamJson(StatusCode::OK, data);
```

### 伺服器傳送事件（SSE） {#event-streams}

`Response::eventStream` 傳送 `text/event-stream` 並關閉快取與代理緩衝。`StreamedEvent::message` 使用 `update` 事件。具名文字事件使用 `StreamedEvent::new`，JSON 資料使用 `StreamedEvent::json`。預設結束事件帶有 `</stream>`。`Response::eventStreamWithEnd` 接收自訂結束事件，傳入 `None` 可省略結束事件。

回應標頭送出後，JSON 或下載資料來源失敗會以傳輸錯誤結束本文。應用程式中的 SSE 資料來源失敗時，會交由目前應用程式的例外處理器回報，連線關閉且不傳送結束事件。

### 串流下載 {#streamed-downloads}

`Response::streamDownload` 分段傳送下載內容。提供檔名時，框架依指定的處置方式產生 `Content-Disposition`，並視需要附加 UTF-8 檔名。檔名為 `None` 時，由呼叫端提供的回應標頭決定處置方式。

## 回應巨集 {#response-macros}

在 Service Provider 的 `boot` 中註冊非同步回應巨集。巨集接收 JSON 值參數並回傳 HTTP 回應。

```rust
use openrust::{
    http::{HttpError, Response as HttpResponse, StatusCode},
    support::facades::Response,
};

Response::r#macro("caps", |arguments| async move {
    let [serde_json::Value::String(value)] = arguments.as_slice() else {
        return Err(HttpError::InvalidRequest);
    };
    HttpResponse::json(StatusCode::OK, value.to_uppercase())
})?;
```

在路由或控制器中按名稱呼叫。

```rust
Response::call("caps", vec!["hello".into()]).await
```

`hasMacro` 檢查目前的工廠。`mixin` 接收明確列出的 `ResponseMacro` 與替換開關。`flushMacros` 清空目前應用程式的登記表。每個應用程式使用自己的回應工廠。
