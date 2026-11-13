# 路由

在 `routes/web.rs` 宣告 Web 路由。應用程式啟動時透過 `openrust::routing::Router` 註冊這些路由及 `/up` 健康檢查。需要讀取請求的處理器接收可變的 `Request`，並傳回非同步 HTTP 結果。框架呼叫 Web 路由模組的 `register()` 時繫結目前應用程式的 Router。匯入 `openrust::support::facades::Route` 後，可以用 `Route::get("/", || view("welcome"))` 宣告具名檢視。

在 `routes/api.rs` 宣告 API 路由。應用程式骨架預設掛載到 `/api` 並套用 `api` 中介軟體群組。設定 `ROUTING_API_PREFIX=/v1/internal` 可修改 `routing.api_prefix`，在 Builder 中呼叫 `Routing::apiPrefix("v2")` 則優先於設定。首尾斜線可以省略。設定 `ROUTING_API_PREFIX=` 或傳入空字串可省略前綴，`api` 中介軟體群組仍會套用。

在 `routes/api.rs` 的無參數 `register() -> Result<(), RoutingError>` 中使用 `Route` Facade 宣告 API 端點。路由路徑相對於 API 前綴。GET 路由需要命名、模型繫結或路由層級中介軟體時，可使用 `Route::getWith`。

`{id}` 表示必填參數。在末尾參數加上 `?` 即可省略，例如 `/profiles/{id?}/{tab?}` 或 `/report/{name}.{format?}`。使用 `request.route("id")` 讀取符合的值。省略參數時傳回 `None`，也可以用路由的 `defaults` 提供字串預設值。參數後面還有必填參數或固定文字時，仍須提供該參數。

用 `Route::r#where` 指定正規表示式，也可使用 `where_number`、`where_alpha`、`where_alpha_numeric`、`where_uuid`、`where_ulid` 或 `where_in`。在註冊路由前呼叫 `Router::pattern`，可替後續路由的同名參數設定限制。單一路由上的限制優先。`where_in` 將值接成正規表示式的替代分支，特殊字元保留原義。OpenRust 使用 Rust 的 `regex` 語法，PCRE 的環視與反向參照會在註冊時被拒絕。

比對時以 UTF-8 對路徑執行百分號解碼，並忽略查詢字串。解碼結果無效時傳回 HTTP 400。比對路由與檢查可用方法時都會套用限制。例如 `/profiles/{id?}` 是限制數字 ID 的 GET 路由時，`POST /profiles/7` 傳回 405 和 `Allow`。如果沒有其他路由符合該路徑，`POST /profiles/alice` 傳回 404。

`Router::get` 同時註冊 GET 和 HEAD。HEAD 保留回應狀態和標頭，傳送空的回應主體。未知路徑傳回 404。路徑存在但方法不受支援時傳回 405，並附帶 `Allow` 標頭。路徑存在且未明確註冊 OPTIONS 時，路由器傳回 200 並列出允許的方法。同一方法有多條路由符合時，使用最先註冊的符合項目。

## 掛載靜態目錄

應用程式骨架先掛載網站公開目錄，再掛載公開儲存磁碟。目錄及 URL 前綴請見[公開檔案設定](../getting-started/configuration.md#public-files)。

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

相對目錄從應用程式根目錄解析。例如，`public/css/app.css` 對應 `/css/app.css`。如需關閉公開磁碟掛載，移除 `routes/web.rs` 中的 `Route::staticDisk("public")?` 呼叫。網站目錄預設掛載到 `/`，公開磁碟預設掛載到 `/storage`。兩個掛載都能回應同一個 URL 時，框架依宣告順序傳回第一個符合的檔案。依骨架的順序，請求 `/storage/avatar.png` 時，`public/storage/avatar.png` 優先於 `storage/app/public/avatar.png`。公開磁碟無需建立 `public/storage` 符號連結。應用程式路由優先，只有未比對到任何應用程式路由的 GET 和 HEAD 請求才會查找檔案。檔案不存在或請求指向目錄時傳回 404，符號連結無法存取掛載目錄外的檔案。

靜態檔案有可用的修改時間時，回應會帶上 `Last-Modified`。GET 或 HEAD 請求可將該值放入 `If-Modified-Since` 重新驗證檔案。檔案未修改時傳回 `304 Not Modified`，回應本文為空。

## 健康檢查

`bootstrap/app.rs` 的 `health("/up")` 註冊健康端點。健康時回傳 200。HTML 頁面的標題顯示應用程式名稱，本文顯示「Application up」和從收到請求到算繪的時間。期望 JSON 的請求回傳 `{"status":"up"}`。HEAD 不傳送本文。

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

端點會分派 `openrust::foundation::events::DiagnosingHealth`。可以註冊同步監聽器檢查資料庫或快取等相依服務。監聽器失敗時，框架回報錯誤並回傳 500，回應為「Application experiencing problems」頁面或 `{"status":"down"}`。除錯模式下由一般例外處理器算繪錯誤。健康頁面內建樣式。

## 查看路由清單

使用 `route:list` 查看生效的路由，以 `--json` 輸出結構化結果。可依路徑、方法、名稱、網域、處理器或中介層篩選。`--except-path` 接收以逗號分隔的子字串。`--sort=definition` 保留註冊順序，`--reverse` 反轉結果。

```sh
./artisan route:list --json --method=HEAD
```

## 路由參數

必要參數寫成 `{id}`，尾端的選用參數寫成 `{id?}`，預設值使用 `defaults` 設定。參數未符合限制時，繼續比對其他路由，均未比對成功則回傳 404。

## 路由群組

群組共用 URI 前綴、名稱前綴、網域、中介層和限制。限定網域的路由應先於相同路徑的不限網域路由註冊。網域與路徑的預留位置都可透過 `request.route` 讀取。

群組閉包接收一個可變的 Rust 路由器，可用它設定已註冊的路由。GET 路由同時接受 HEAD，自動 OPTIONS 依比對的路徑和網域回傳允許的方法。

```rust
use openrust::{routing::{RouteGroup, RoutingError}, support::facades::Route, view::view};

fn register() -> Result<(), RoutingError> {
    Route::group(
        RouteGroup::new()
            .domain("{tenant}.example.test")
            .prefix("admin")
            .name("admin.")
            .r#where("id", "[0-9]+")?,
        |router| {
            router
                .get("/reports/{id?}", || view("welcome"))?
                .name("reports.show")
                .defaults("id", "1");
            Ok(())
        },
    )
}
```

## 跨來源資源共享（CORS）

在 `config/cors.rs` 中設定路徑、來源、方法和請求標頭。全域 `HandleCors` 中介軟體對相符的瀏覽器預檢回傳空本文 HTTP 204，也會為實際回應套用策略，包括 404。在動態策略中，被拒絕的來源不會收到來源許可。只設定一個來源時可傳送固定值，瀏覽器僅允許該來源。

未設定 `CORS_PATHS` 時，`cors.paths` 沿用 Laravel 的預設路徑 `api/*` 和 `sanctum/csrf-cookie`。API 路由前綴與 CORS 路徑各自設定。若 API 掛載到 `/v1`，可設定 `CORS_PATHS=v1/*,sanctum/csrf-cookie` 為這些路徑啟用 CORS。設定 `CORS_PATHS=*` 可涵蓋所有路徑，設定 `CORS_PATHS=` 可停用 CORS。跨來源憑證預設關閉。路徑映射可按主機覆寫路徑，用數字字串鍵表示共用路徑。`allowed_origins_patterns` 使用 Rust 正規表示式引擎支援的模式。HTTP 核心在啟動時載入並驗證策略。

```bash
curl -i -X OPTIONS http://127.0.0.1:8000/api/example \
  -H 'Origin: https://example.com' \
  -H 'Access-Control-Request-Method: GET'
```

## 定義限流器

在服務提供者的 boot 階段註冊策略，再用 `middleware_name("throttle:uploads")` 附加到路由。用 `Limit::by` 劃分計數鍵，`response` 自訂拒絕回應，或用 `after` 選擇哪些回應消耗額度。拒絕回傳 HTTP 429，並附帶 `Retry-After`、`X-RateLimit-Reset`、`X-RateLimit-Limit` 和 `X-RateLimit-Remaining`。

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Limit},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::{RateLimiter, Route},
};

fn register_limits() -> CacheResult<()> {
    RateLimiter::r#for("uploads", |_| Limit::perMinute(5))
}

fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new(), |router| {
        router.get("/uploads", upload)?.middleware_name("throttle:uploads");
        Ok(())
    })
}

fn upload(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}
```

### 使用 Redis 節流

在 `withMiddleware` 中呼叫 `middleware.throttleWithRedis()`，讓命名節流別名使用預設 Redis 連線。使用相同 Redis 連線設定和鍵前綴的程序共用配額。Redis 原子檢查並消耗各項額度。使用 `after` 的規則在回應回傳後計數。

## 路由模型繫結

使用 `Route::model::<Post>("post")?` 在應用程式 Router 註冊顯式繫結。也可以透過 Router 回傳的路由呼叫 `.model::<Post>("post")?`，為該路由宣告模型參數。使用 `request.route_model::<Post>("post")` 讀取已解析的模型。模型缺失回傳 404，可透過 `missing` 自訂回應。

模型預設按主鍵查詢。可覆寫 `getRouteKeyName` 使用 slug，也可為單條路由指定 `{post:slug}`。巢狀的自訂鍵透過前一個已繫結的模型限定查詢範圍。使用 `scopeBindings` 為預設鍵啟用作用域，或透過 `withoutScopedBindings` 關閉。子模型透過 `resolveChildRouteBinding` 選擇型別化關聯。`withTrashed` 可包含軟刪除記錄。預設 `web` 和 `api` 群組在處理器前執行繫結。

## 重新導向路由

使用 `Route::redirect` 註冊重新導向並指定 HTTP 狀態。`Route::permanentRedirect` 傳回 301。`/articles/{id}` 這類目的地可以沿用符合的路由參數。
