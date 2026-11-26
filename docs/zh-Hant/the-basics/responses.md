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

## JSON 業務回應 {#business-responses}

`ApiResponse` 提供統一業務 JSON 格式，包含 `success`、`code`、`message`，按需附帶 `data`、`meta` 和 `errors`。它是業務回應擴充；HTTP 狀態碼仍表示請求結果。

### 明確回傳業務回應 {#business-response-configuration}

需要統一業務格式時，由處理器明確回傳 `ApiResponse`。舊的 `API_RESPONSE_ENVELOPE` 設定已移除，可從環境變數中刪除。

框架預設的 JSON 例外包含 `message`，驗證例外也包含欄位 `errors`。一般資源依資源設定包裝資料，保留 `links` 和 `meta` 等中繼資料。JSON:API 資源遵循自己的協定格式。

相關機制見 [API 資源](../eloquent/resources.md)及[例外處理](./errors.md)。

### 成功、失敗與分頁 {#business-response-examples}

在 `routes/api.rs` 註冊時，下列路徑還會帶上應用程式設定的 API 前綴，預設 `/api`。

除另有語言或原文訊息說明外，下方預設訊息範例的目前請求語言為 `en`；切換為 `zh-Hans` 或 `zh-Hant` 時，`Success` 對應 `成功`。函式可以直接用於路由，例如匯入 `Route` 後呼叫 `Route::get("/example-user", user_response)?;`。

```rust
use std::collections::BTreeMap;
use openrust::{
    http::{ApiResponse, PageMeta, StatusCode},
    routing::RoutingError,
    support::facades::Route,
};
use serde_json::json;

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get("/items", || {
        ApiResponse::success(json!([{"id": 1, "name": "Notebook"}]))
            .with_message("Loaded.")
            .with_meta(PageMeta { page: 1, per_page: 10, total: 1, total_pages: 1 })
    })?;
    Route::post("/items", || {
        ApiResponse::success(json!({"id": 2}))
            .with_message("Created.")
            .with_status(StatusCode::CREATED)
    })?;
    Route::get("/example-validation", || {
        ApiResponse::failure(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed", "Invalid input.")
            .with_errors(BTreeMap::from([("email".into(), vec!["Email is required.".into()])]))
    })
}
```

GET `/api/items` 回傳 HTTP 200：

```json
{
  "success": true,
  "code": "ok",
  "message": "Loaded.",
  "data": [
    {
      "id": 1,
      "name": "Notebook"
    }
  ],
  "meta": {
    "page": 1,
    "per_page": 10,
    "total": 1,
    "total_pages": 1
  }
}
```

POST `/api/items` 回傳 HTTP 201，本文為 `{"success":true,"code":"ok","message":"Created.","data":{"id":2}}`。

GET `/api/example-validation` 回傳 HTTP 422：

```json
{
  "success": false,
  "code": "validation_failed",
  "message": "Invalid input.",
  "errors": {
    "email": [
      "Email is required."
    ]
  }
}
```

`success` 預設 HTTP 200；`failure` 應搭配非成功狀態碼。`with_status` 只修改 HTTP 狀態，不修改業務欄位；成功／失敗旗標與一般有本文狀態碼不一致時，`into_response` 回傳錯誤。

`ApiResponse::empty()`、`success(())` 和 `success(Value::Null)` 省略 `data`；空物件、空陣列、`0`、`false` 和空字串仍保留。HTTP 204、205、304 及資訊狀態碼傳送空本文。序列化錯誤交給[例外處理](./errors.md)。

#### 最簡單的成功回應 {#business-response-success}

最簡單的成功呼叫回傳 HTTP **200**：

```rust
use openrust::http::ApiResponse;
use serde_json::json;

fn user_response() -> ApiResponse<serde_json::Value> {
    ApiResponse::success(json!({"id": 1, "name": "Ada"}))
}
```

```json
{
  "success": true,
  "code": "ok",
  "message": "Success",
  "data": {
    "id": 1,
    "name": "Ada"
  }
}
```

#### 空資料與無本文回應 {#business-response-empty}

下列三個函式都回傳 HTTP **200**，但只有第一個省略 `data`：

```rust
use openrust::http::ApiResponse;
use serde_json::{Value, json};

fn empty_response() -> ApiResponse<()> {
    ApiResponse::empty()
}

fn empty_object_response() -> ApiResponse<Value> {
    ApiResponse::success(json!({}))
}

fn empty_array_response() -> ApiResponse<Value> {
    ApiResponse::success(json!([]))
}
```

`empty_response`、`empty_object_response`、`empty_array_response` 的完整 JSON 本文依序為：

```json
{
  "success": true,
  "code": "ok",
  "message": "Success"
}
```

```json
{
  "success": true,
  "code": "ok",
  "message": "Success",
  "data": {}
}
```

```json
{
  "success": true,
  "code": "ok",
  "message": "Success",
  "data": []
}
```

需要 HTTP **204** 時，整個本文為空，而不是回傳空的業務封裝：

```rust
use openrust::http::{ApiResponse, HttpError, Response, StatusCode};

fn no_content_response() -> Result<Response, HttpError> {
    ApiResponse::empty()
        .with_status(StatusCode::NO_CONTENT)
        .into_response()
}
```

此回應狀態為 `204 No Content`，**本文為零位元組**，用戶端不要將它解析為 JSON。

### 國際化訊息 {#business-response-localization}

預設成功訊息讀取 `response.ok`，框架例外讀取對應的 `response.*` 鍵，使用目前請求語言和既有回退機制。應用程式可在目前語言目錄的 `response.json` 覆寫這些詞條。請求語言的設定與選擇見[本地化](../digging-deeper/localization.md)；無需在每個回應中解析請求標頭。

`with_message` 和 `failure` 的訊息是原文，不自動翻譯。需要翻譯時使用 `with_message_key`，在最終序列化時解析。下例在應用程式預設語言目錄的 `lang/zh-Hans/messages.json` 定義詞條，並在目前語言為 `zh-Hans` 時回傳：

```json
{
  "created": "已创建 :name。"
}
```

```rust
use std::collections::BTreeMap;
use openrust::http::ApiResponse;

fn localized_response() -> ApiResponse<u64> {
    ApiResponse::success(1).with_message_key(
        "messages.created",
        BTreeMap::from([("name".into(), "Notebook".into())]),
    )
}
```

```json
{
  "success": true,
  "code": "ok",
  "message": "已创建 Notebook。",
  "data": 1
}
```

英文詞條放在 `lang/en/messages.json`：

```json
{
  "created": "Created :name."
}
```

上方本地化範例回傳 HTTP **200**。目前請求語言切換為 `en` 時，完整本文變為：

```json
{
  "success": true,
  "code": "ok",
  "message": "Created Notebook.",
  "data": 1
}
```

### 自訂欄位 {#business-response-fields}

`with_field` 從回應根開始設定點路徑，`without_field` 刪除對應欄位；支援巢狀物件及陣列索引。`additional` 淺合併字面的頂級鍵，可傳 JSON Map 或鍵值對陣列。操作按呼叫順序生效，後者覆蓋前者；JSON 欄位修改不會改變 `with_status` 設定的 HTTP 狀態。

缺失路徑會自動建立，既有 null 可作為空佔位；經過非 null 純量時回傳序列化錯誤，不靜默覆蓋父資料。刪除缺失路徑不報錯。沿用既有 `Arr` 規則：`*` 是字面鍵，連續數字索引保持陣列，跨索引新增或刪除中間元素會保留鍵並呈現為 JSON 物件，不自動重排。含點號的字面鍵使用片段陣列，例如 `["data", "user", "first.name"]`。字串刪除路徑優先刪除完整的頂級字面鍵，與 `Arr::forget` 相同。

欄位操作在預設空值省略及訊息翻譯之後執行，因此可以顯式加入 `data: null`、覆蓋 `code` 或刪除 `message`。應用應自行維持自訂後公開協定的一致性。

下列回應回傳 HTTP 200：

```rust
use openrust::http::ApiResponse;
use serde_json::json;

fn custom_response() -> ApiResponse<serde_json::Value> {
    ApiResponse::success(json!({"id": 1}))
        .with_message("Saved.")
        .additional(json!({"trace_id": "first", "code": "saved"}).as_object().unwrap().clone())
        .without_field("trace_id")
        .with_field("trace_id", "request-123")
}
```

```json
{
  "success": true,
  "code": "saved",
  "message": "Saved.",
  "data": {
    "id": 1
  },
  "trace_id": "request-123"
}
```

以下完整範例回傳 HTTP **200**，同時展示頂級欄位新增與刪除、巢狀欄位新增／修改／刪除、陣列元素修改與附加，以及含點號的字面鍵：

```rust
use openrust::http::ApiResponse;
use serde_json::json;

fn customized() -> ApiResponse<serde_json::Value> {
    ApiResponse::success(json!({
        "user": {"name": "Before", "password": "private"},
        "items": [{"id": 1}]
    }))
    .additional([
        ("request_id", json!("req-123")),
        ("temporary", json!(true)),
    ])
    .with_field("data.user.name", "Ada")
    .with_field("data.user.profile.city", "Shanghai")
    .with_field("data.items.0.selected", true)
    .with_field("data.items.1", json!({"id": 2}))
    .without_field("data.user.password")
    .without_field("temporary")
    .with_field(["data", "user", "first.name"], "Ada")
}
```

```json
{
  "success": true,
  "code": "ok",
  "message": "Success",
  "request_id": "req-123",
  "data": {
    "user": {
      "name": "Ada",
      "profile": {
        "city": "Shanghai"
      },
      "first.name": "Ada"
    },
    "items": [
      {
        "id": 1,
        "selected": true
      },
      {
        "id": 2
      }
    ]
  }
}
```

#### 陣列刪除與跳過索引 {#business-response-array-boundaries}

下列兩個函式均回傳 HTTP **200**。刪除索引 `1`，或在缺少索引 `1` 時直接新增索引 `2`，都會保留原來的數字鍵。兩者的完整回應相同：

```rust
use openrust::http::ApiResponse;
use serde_json::{Value, json};

fn removed_item_response() -> ApiResponse<Value> {
    ApiResponse::success(json!({
        "items": [{"id": 1}, {"id": 2}, {"id": 3}]
    }))
        .without_field("data.items.1")
}

fn sparse_item_response() -> ApiResponse<Value> {
    ApiResponse::success(json!({
        "items": [{"id": 1}]
    }))
        .with_field("data.items.2", json!({"id": 3}))
}
```

```json
{
  "success": true,
  "code": "ok",
  "message": "Success",
  "data": {
    "items": {
      "0": {
        "id": 1
      },
      "2": {
        "id": 3
      }
    }
  }
}
```

若公共協定要求 JSON 陣列，請使用連續索引。刪除不存在的路徑不影響回應；如果 `data.user` 已是非 null 純量，再設定 `data.user.name` 會產生序列化錯誤，框架按服務例外處理，不會直接覆寫原純量。

### 應用資訊介面 {#api-application-info}

根應用透過 `ApiController::index` 提供公開的 `GET /api`。名稱及版本讀取 `app.name`、`app.version`，可透過 `APP_NAME`、`APP_VERSION` 設定；位址遵循 `API_PREFIX`，例如 `api/admin` 對應 `/api/admin`。無需認證擴充或資料庫。此端點將 `ApiResponse` 傳給 `Response::json`，`name` 和 `version` 位於 `data` 中。

`ApiResponse` 的預設訊息使用請求語言，英文為 `Success`，簡體與繁體中文為 `成功`。框架隨附既有 18 種預設語言的回應詞條，`lang:publish` 會發佈 `response.json` 供應用覆蓋。自訂鍵缺失時遵循既有語言回退；翻譯來源格式錯誤時回傳安全的服務例外，不把翻譯診斷偽裝為成功訊息。

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
