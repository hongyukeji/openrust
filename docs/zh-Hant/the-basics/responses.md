# HTTP 回應

## 建立回應

用 `Response::json` 回傳 JSON 資料與明確的狀態碼。透過 `header` 或 `with_headers` 設定具型別的 HTTP 回應標頭。用 `Response::stream` 從 Rust 串流逐段傳送本文。

輸入讀取及 JSON 回應的完整範例見[控制器](./controllers.md)。

### 字串和陣列 {#strings-and-arrays}

路由或控制器可以直接傳回字串。OpenRust 以 HTTP 200 和 `text/html; charset=utf-8` 傳送原字串作為 HTML 本文。需要轉譯和逸出範本資料時傳回檢視。

```rust
Route::get("/greeting", || "<p>Hello</p>")?;
Route::get("/numbers", || [1, 2, 3])?;
```

陣列、向量、`serde_json::Value`、JSON 對應、支援層集合，以及以 JSON 為基礎的 `Fluent` 屬性集合會轉為 HTTP 200 JSON。有序集合保留鍵，稀疏的數字鍵會輸出為 JSON 物件。需要明確指定狀態碼時使用 `Response::json`。

### 原生 HTTP 回應 {#native-http-responses}

路由閉包與控制器方法可以傳回 `http::Response<Body>`。路由器會將它轉為框架回應，接著執行相同的中介層與 HTTP 回應準備流程。非同步處理器與完成 FormRequest 驗證的方法同樣適用。

在應用程式相依性中加入 `http = "1.0"` 即可直接使用原生回應型別。

```rust
Route::get("/native", || {
    http::Response::new(openrust::http::Body::from("native"))
})?;
```

用 `Response::from(native)` 將原生回應接入框架，用 `into()` 轉回原生型別。轉換會保留狀態碼、重複回應標頭、協定版本、擴充資料與目前的本文串流。HTTP 核心會在送出時處理 HEAD 和無本文狀態碼。

### 依賴請求的回應 {#responsable}

傳回物件需要目前請求來產生回應時，實作 `openrust::contracts::support::Responsable`。路由器會在動作執行後呼叫 `to_response`，此時可讀取路由參數和輸入。結果接著經過一般中介層及 HTTP 回應準備流程。

```rust
use openrust::{
    contracts::support::Responsable,
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Route,
};

struct Report;

#[allow(non_snake_case)]
impl Responsable for Report {
    async fn to_response(self, request: &mut Request) -> Result<Response, HttpError> {
        Response::json(StatusCode::OK, request.route("id"))
    }
}

Route::get("/reports/{id}", || Report)?;
```

閉包可以直接傳回物件，控制器或經過 FormRequest 驗證的動作可用 `Result` 傳回物件。從 `to_response` 傳回 `HttpError` 會沿用應用程式的例外轉譯。

### 提前傳回已建立的回應 {#prepared-response-exceptions}

在回傳 `Result` 的處理常式或中介軟體中，用 `Err(response.throw_response())` 提前傳送已建立的 `Response` 或 `RedirectResponse`。HTTP 例外處理器會呼叫 `respond` 回呼。為 `HttpResponseException` 註冊的自訂呈現器可覆蓋原回應，也可以用 `take_response()` 一次取出原回應。這類例外預設略過回報。

已有框架或原生回應時，可以用 `abort_response(response)`。需要目前請求的回應物件使用 `Err(abort_responsable(value, request).await)`。若 `to_response` 失敗，原例外交由處理器呈現。按狀態碼中止時使用 `abort(status, message, headers)`。

用 `with_exception(error)` 附加診斷資訊，應用程式可透過 `response.exception()` 讀取。附加例外只會記錄回應中繼資料。

### 附加回應標頭 {#attaching-headers-to-responses}

用 `without_header` 移除一個具型別的回應標頭，用 `without_headers` 批次移除。兩者都回傳回應，可繼續鏈式呼叫。

#### 快取控制中介層 {#cache-control-middleware}

在需要快取策略的路由上使用 `cache.headers`。選項名稱使用 `snake_case`，以分號分隔。中介層處理成功的 GET 和 HEAD 回應。指定 `etag` 時沿用既有驗證器，或以與框架`xxh128` 相同的 XXH3-128 摘要計算回應本文。串流回應可用 `etag=value` 明確提供驗證器，無須緩衝整個串流。

```rust
Route::get_with("/cacheable", cache_response, |route| {
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

### 設定與刪除回應 Cookie {#expiring-cookies-early}

一般回應與重新導向回應皆支援 `with_cookie(&cookie, request)` 與 `with_cookies([&first, &second], request)`。名稱、路徑及網域相同時，新的 Cookie 會覆蓋先前的 Cookie。用 `without_cookie("theme", None, None, request)` 或 `without_cookies(["theme", "contrast"], None, None, request)` 傳送過期 Cookie。原 Cookie 的路徑或網域與應用程式預設值不同時，請明確傳入原值。

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

序列化失敗交給[例外處理](errors.md)。內建例外和健康檢查各自使用原有的回應格式。統一業務回應格式另外提供。

## 重新導向回應

使用 `Redirect::to` 前往 URL，或以 `Redirect::route` 前往命名路由。`RedirectOptions` 預設使用 302，並支援回應標頭。`Redirect::guest` 將存取目標存入目前工作階段，`Redirect::intended` 讀取並清除它。工作階段沒有存取目標時使用備用位址。

用 `Redirect::action` 前往[已註冊的控制器動作](./urls.md#urls-for-controller-actions)。

`Redirect::refresh` 重新導向至移除查詢字串的目前路徑。`Redirect::secure` 從應用程式路徑產生 HTTPS 目的地。`Redirect::away` 將明確的 HTTP(S) URL 或相對參照寫入 `Location`，呼叫時請傳入可信任的目的地。`Redirect::signed_route` 和 `Redirect::temporary_signed_route` 重新導向至絕對簽章 URL。`Redirect::back` 依序使用同源 Referer、上一個工作階段 URL 和備用位址。`Redirect::get_intended_url` 讀取目前的預期位址，`Redirect::set_intended_url` 明確寫入預期位址。兩個方法都接收目前的請求。

路由層級的重新導向可使用 `Router::redirect` 或 `Router::permanent_redirect`，將來源參數代入目的地路徑。目的地的選用區段（例如 `/articles/{slug?}`）若無對應值就會省略。`destination` 和 `status` 是保留的來源參數名稱。

### 重新導向 URL 片段 {#redirect-url-fragments}

呼叫 `with_fragment("details")` 取代目前的片段，或用 `without_fragment()` 移除片段。兩者皆保留 URL 路徑與查詢參數，並同步更新 `Location` 回應標頭與 HTML 跳轉連結。方法回傳 `Result`，以便處理無效的重新導向目標。

### 同源重新導向 {#same-origin-redirects}

呼叫 `redirect.enforce_same_origin(request, "/fallback", true, true)`，將目標與目前的請求比對。主機必須相同，兩個布林參數分別控制是否驗證協定與連接埠。比對採用請求生效的實際來源，包含受信任的代理設定，並將明確指定的預設連接埠與省略連接埠視為相同。相對目標沒有主機，因此會使用備用網址。請傳入受信任的備用網址，例如固定的應用程式路徑。

### 重新導向時暫存工作階段資料 {#redirecting-with-flashed-session-data}

在啟用工作階段的 Web 路由中，使用 `with` 暫存值，`with_input` 保留輸入，`with_errors` 附加命名的 `MessageBag`。Rust 方法明確接收目前請求的可變參考。處理常式可以直接傳回 `RedirectResponse`。狀態碼、回應標頭及工作階段更新沿用一般的 HTTP 回應流程。

```rust
use openrust::{
    http::{HttpError, HttpFuture, RedirectResponse, Request},
    support::facades::Redirect,
};
use serde_json::json;

fn save_profile(request: &mut Request) -> HttpFuture<'_, Result<RedirectResponse, HttpError>> {
    Box::pin(async move {
        Redirect::to("/profile", Default::default())?
            .with(request, "status", json!("saved"))?
            .only_input(request, &["name"])
            .await
    })
}
```

向 `with_input` 傳入 `None` 時，讀取請求的表單或 JSON 欄位及查詢參數。上傳檔案會被排除。使用 `only_input` 或 `except_input` 依點路徑篩選欄位。下一次請求可用 `request.old(Some("name"))` 讀取舊輸入，並從工作階段讀取暫存值。

## 其他回應類型 {#other-response-types}

### 視圖回應 {#view-responses}

用 `Response::view` 渲染已準備好的視圖，並指定狀態碼與回應標頭。可傳入 `view("name")`，也能用 `View::first` 門面選取第一個可用的範本。渲染保留目前請求的上下文。

### JSON 回應 {#json-responses}

用 `Response::json(StatusCode::CREATED, data)?` 指定狀態碼，並序列化實作了 `serde::Serialize` 的值。回應會設定 `Content-Type: application/json`。用 `header` 或 `with_headers` 附加回應標頭。路由直接回傳 JSON 值時使用 HTTP 200。

### JSONP 回應 {#jsonp-responses}

用 `Response::jsonp(status, Some(callback), data)` 將 JSON 資料傳給 JavaScript 回呼。回呼名稱支援點分名稱與方括號字面值鍵。無效的回呼名稱會回傳 HTTP 400。傳入 `None` 會回傳一般 JSON 回應。

### Markdown 回應 {#markdown-responses}

用 `Response::markdown(status, content)` 傳送 Markdown 原始文字，媒體類型為 `text/markdown; charset=utf-8`。由用戶端決定如何渲染。

### 空回應 {#empty-responses}

用 `Response::no_content()` 回傳無本文的 HTTP 204。用 `Response::empty(status)` 指定其他狀態碼，用 `with_headers` 附加回應標頭。

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

透過 `Response::stream_json` 和 `StreamedJson::object` 將資料列串流放進 JSON 物件。`StreamedJson::items` 在每列到達時序列化。固定值使用 `StreamedJson::value`，巢狀陣列使用 `StreamedJson::array`。使用 `StreamedJson::entries` 可將鍵值對逐項寫入 JSON 物件。

串流 JSON 與 JSON 事件資料沿用 預設字串跳脫。SSE JSON 保留無須跳脫的非 ASCII 文字。

```rust
use futures_util::stream;
use openrust::http::{HttpError, Response, StatusCode, StreamedJson};

let users = stream::iter([Ok::<_, HttpError>(1), Ok(2)]);
let data = StreamedJson::object([("users", StreamedJson::items(users))]);
let response = Response::stream_json(StatusCode::OK, data);
```

### 伺服器傳送事件（SSE） {#event-streams}

`Response::event_stream` 傳送 `text/event-stream` 並關閉快取與代理緩衝。`StreamedEvent::message` 使用 `update` 事件。具名文字事件使用 `StreamedEvent::new`，JSON 資料使用 `StreamedEvent::json`。預設結束事件帶有 `</stream>`。`Response::event_stream_with_end` 接收自訂結束事件，傳入 `None` 可省略結束事件。

回應標頭送出後，JSON 或下載資料來源失敗會以傳輸錯誤結束本文。應用程式中的 SSE 資料來源失敗時，會交由目前應用程式的例外處理器回報，連線關閉且不傳送結束事件。

### 串流下載 {#streamed-downloads}

`Response::stream_download` 分段傳送下載內容。提供檔名時，框架依指定的處置方式產生 `Content-Disposition`，並視需要附加 UTF-8 檔名。檔名為 `None` 時，由呼叫端提供的回應標頭決定處置方式。

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

`has_macro` 檢查目前的工廠。`mixin` 接收明確列出的 `ResponseMacro` 與替換開關。`flush_macros` 清空目前應用程式的登記表。每個應用程式使用自己的回應工廠。

## 回應門面 {#response-facade}

在應用程式作用域中匯入 `support::facades::Response`。它委託給同一應用程式的 `http::ResponseFactory`，也可以透過容器的 `response` 名稱解析該工廠。回應值型別仍是 `http::Response`。

```rust
use openrust::{
    http::{HeaderMap, StatusCode},
    support::facades::{Response, Route},
};

Route::get("/status", || async {
    Response::json(
        serde_json::json!({"status": "ready"}),
        StatusCode::OK,
        HeaderMap::new(),
    )
})?;
```

`make` 接受可轉換為回應的字串或結構化資料；`no_content`、`json`、`jsonp`、`markdown` 接受明確狀態與回應標頭。`stream` 使用 Rust 非同步串流，`stream_json` 使用 `StreamedJson`，`event_stream` 預設附加 `</stream>`；用 `event_stream_with_end(..., None)` 關閉結束事件。檔案與串流下載使用 `file`、`download` 和 `stream_download`。

`view` 渲染目前請求上的視圖，`view_first` 選擇第一個存在的視圖。持有 `&mut Request` 的處理器用 `view_for_request`，或先用 `request.with_container(async { ... }).await` 借出請求。`redirect_to`、`redirect_to_route` 和 `redirect_to_action` 重用應用程式路由與 URL 產生器；`redirect_guest` 和 `redirect_to_intended` 修改目前請求的 Session。
