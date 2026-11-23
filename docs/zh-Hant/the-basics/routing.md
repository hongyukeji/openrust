# 路由

在 `routes/web.rs` 宣告 Web 路由。應用程式啟動時透過 `openrust::routing::Router` 註冊這些路由及 `/up` 健康檢查。需要讀取請求的處理器接收可變的 `Request`，並傳回非同步 HTTP 結果。框架呼叫 Web 路由模組的 `register()` 時繫結目前應用程式的 Router。匯入 `openrust::support::facades::Route` 後，可以用 `Route::get("/", || view("welcome"))` 宣告具名檢視。

執行 `./artisan install:api` 按需產生並註冊 `routes/api.rs` 和 `config/api.rs`。在 `routes/api.rs` 中定義 API 路由。應用程式從 `config/api.rs` 載入 `api.prefix`，並套用 `api` 中介軟體群組。預設前綴為 `/api`。在 `.env` 中設定 `API_PREFIX=api/admin` 可掛載至 `/api/admin`，設定 `API_PREFIX=` 可省略前綴。在 `withRouting` 中明確呼叫 `Routing::apiPrefix(...)` 時，以該呼叫為準。 重複安裝時保留既有的 `config/api.rs`，包括使用 `--force` 時。

在 `routes/api.rs` 的無參數 `register() -> Result<(), RoutingError>` 中使用 `Route` Facade 宣告 API 端點。路由路徑相對於 API 前綴。GET 路由需要命名、模型繫結或路由層級中介軟體時，可使用 `Route::getWith`。

`{id}` 表示必填參數。在末尾參數加上 `?` 即可省略，例如 `/profiles/{id?}/{tab?}` 或 `/report/{name}.{format?}`。使用 `request.route("id")` 讀取符合的值。省略參數時傳回 `None`，也可以用路由的 `defaults` 提供字串預設值。參數後面還有必填參數或固定文字時，仍須提供該參數。

用 `Route::r#where` 指定正規表示式，也可使用 `where_number`、`where_alpha`、`where_alpha_numeric`、`where_uuid`、`where_ulid` 或 `where_in`。在註冊路由前呼叫 `Router::pattern`，可替後續路由的同名參數設定限制。單一路由上的限制優先。`where_in` 將值接成正規表示式的替代分支，特殊字元保留原義。沒有明確限制的路由使用 Rust 的線性正規表示式引擎。明確限制使用 PCRE2，支援環視、具名反向參照和 `(*MARK:...)`。例如，`r"(?!admin\z)[a-z]+"` 排除 `admin`。比對使用 UTF-8 和 Unicode 字元屬性。無效表示式會使註冊失敗。執行時達到 PCRE2 的比對、深度或堆積記憶體上限時，視為未比對。在 `/paired/{id}/{tab?}` 中，將 `tab` 的限制設為 `r"\k<id>"`，即可要求傳入的 `tab` 與 `id` 相同。反向參照可指向同一路徑或同一網域名稱中的其他擷取群組。群組和全域限制的參照會在註冊路由時解析。

比對時以 UTF-8 對路徑執行百分號解碼，並忽略查詢字串。解碼結果無效時傳回 HTTP 400。比對路由與檢查可用方法時都會套用限制。例如 `/profiles/{id?}` 是限制數字 ID 的 GET 路由時，`POST /profiles/7` 傳回 405 和 `Allow`。如果沒有其他路由符合該路徑，`POST /profiles/alice` 傳回 404。

`Router::get` 同時註冊 GET 和 HEAD。HEAD 保留回應狀態和標頭，傳送空的回應主體。未知路徑傳回 404。路徑存在但方法不受支援時傳回 405，並附帶 `Allow` 標頭。路徑存在且未明確註冊 OPTIONS 時，路由器傳回 200 並列出允許的方法。同一方法有多條路由符合時，使用最先註冊的符合項目。

## HTTP 方法

單一方法使用 `get`、`post`、`put`、`patch`、`delete`、`options` 或 `query`。`Route` 和 `Router` 均提供這些入口；`get` 同時接受 HEAD。`Router` 回傳可變路由物件，可繼續設定名稱、中介軟體、約束和預設值；Facade 宣告回傳 `Result<(), RoutingError>`。

```rust
use openrust::{http::Method, support::facades::Route};

Route::get("/users", UserController::index)?;
Route::post("/users", UserController::store)?;
Route::put("/users/{id}", UserController::replace)?;
Route::patch("/users/{id}", UserController::update)?;
Route::delete("/users/{id}", UserController::destroy)?;
Route::options("/users", UserController::options)?;
Route::query("/users/search", UserController::search)?;
Route::r#match(&[Method::PUT, Method::PATCH], "/profile", ProfileController::update)?;
Route::any("/callback", CallbackController::handle)?;
```

多個指定方法共用處理器時使用 `r#match`，集合包含 GET 時自動補充 HEAD。空集合回傳 `RoutingError::MissingMethods`。只有端點確實需要接受全部支援的方法時才使用 `any`，集合依序為 GET、HEAD、POST、PUT、PATCH、DELETE、OPTIONS、QUERY，不包含 TRACE 或 CONNECT。

對於相同方法、網域和註冊路徑，後宣告覆蓋先宣告的處理器；`any` 和 `r#match` 也遵循此規則，沒有獨立優先順序。不同但重疊的路徑模式按宣告順序首次匹配，因此應將具體路徑放在較寬泛的模式之前。

## 檢視路由

GET/HEAD 路由只傳回檢視時，使用 `Route::view`。前兩個參數是 URI 和檢視名稱。Rust 要求明確傳入第三個選項參數；`Default::default()` 表示空資料、狀態 200 和不附加標頭。

```rust
use openrust::{routing::ViewRouteOptions, support::facades::Route, view::ViewData};

Route::view("/welcome", "welcome", Default::default())?;
Route::view("/hello/{name}", "welcome", ViewRouteOptions {
    data: ViewData::from_iter([("name".into(), "Taylor".into())]),
    ..Default::default()
})?;
```

範本放在 `resources/views`，例如 `welcome.blade.html`。路徑與網域擷取參數覆蓋 `data` 的同名值，隨後執行檢視建立器與組合器。用 `ViewRouteOptions` 的 `status` 和 `headers` 設定自訂回應。PHP 中在狀態參數位置傳標頭陣列的寫法，改用明確的 `headers` 欄位。參數名稱不要使用 `view`、`data`、`status` 或 `headers`。目前路由預設值只承載字串，不支援透過 `Route::defaults` 任意修改這四項型別化檢視設定；繫結模型物件也不會自動轉換為檢視資料。

`Router::view` 傳回可變路由。Facade 宣告使用 `Route::viewWith(uri, view, options, |route| { route.name("welcome"); Ok(()) })`，在註冊借用範圍內設定路由。繪製使用目前應用程式的檢視工廠與中介軟體。範本不存在或建立器／組合器失敗時，沿用一般 HTTP 例外管線。

## 後備路由

在 `routes/web.rs` 中宣告後備路由，處理沒有一般路由符合的 GET/HEAD 請求。後備路由繼承目前群組的前綴、網域與中介軟體。已符合處理器傳回錯誤或 404 回應時，不會再次嘗試後備路由。

```rust
use openrust::support::facades::Route;

Route::fallback(NotFoundController::show)?;
```

一般路由始終優先，即使後備路由先宣告。多個後備路由可符合時按註冊順序選擇；同一方法、網域與註冊路徑的後宣告覆蓋先宣告。未符合的 POST 在後備路由符合其路徑時傳回 405 和 `Allow: GET, HEAD`；自動 OPTIONS 傳回 200。後備路由也是已註冊路由，優先於下文的靜態檔案掛載。

Facade 需要設定時使用 `Route::fallbackWith(handler, |route| { route.name("missing"); Ok(()) })`。`Router::fallback` 傳回可變路由；`route.fallback()` 可標記既有路由，`route.setFallback(false)` 取消標記。路由快取保持該優先順序。


處理器可以明確傳回另一條命名路由的回應。`respondWithRoute` 使用同一個請求 URI 重新綁定目標路由、執行其路由中介軟體，並取代請求的目前路由參數與名稱；不會產生重新導向，也不會將請求 URI 改成目標路徑。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Route,
};

fn show_missing(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respondWithRoute("missing", request))
}
```

先按上文用 `fallbackWith` 登記 `missing`，再將 `show_missing` 登記為處理器。Rust 明確借用可變請求；HTTP 核心透過應用容器提供共用的執行期路由器，每個請求保留獨立路由狀態。名稱不存在時傳回錯誤，渲染為 HTTP 500。直接使用 `Router` 時也提供同名方法。執行期路由器在 HTTP 啟動後可解析；路由註冊仍使用註冊作用域。

## 掛載靜態目錄

應用程式骨架先掛載網站公開目錄，再掛載公開儲存磁碟。目錄及 URL 前綴請見[公開檔案設定](../getting-started/configuration.md#public-files)。

```rust
Route::staticDisk("web")?;
Route::staticDisk("public")?;
```

`web` 和 `public` 磁碟設定在 `config/filesystems.rs`，預設根目錄分別是 `public` 和 `storage/app/public`，預設 URL 分別是 `app.url` 和 `app.url` 加 `/storage`。掛載路徑從 URL 取得，包含應用程式 URL 自帶的路徑前綴。應用程式路由優先。磁碟掛載依宣告順序查找，第一個存在的公開檔案生效。兩個掛載都拒絕符號連結，因此會跳過 `public/storage` 連結，由 `public` 磁碟直接提供檔案。只有未符合應用程式路由的 GET 和 HEAD 請求會讀取檔案，缺失檔案和目錄請求回傳 404。

靜態檔案有可用的修改時間時，回應會帶上 `Last-Modified`。GET 或 HEAD 請求可將該值放入 `If-Modified-Since` 重新驗證檔案。檔案未修改時傳回 `304 Not Modified`，回應本文為空。

## 健康檢查

`bootstrap/app.rs` 的 `health("/up")` 註冊健康端點。健康時回傳 200。HTML 頁面的標題顯示應用程式名稱，本文顯示「Application up」和從收到請求到算繪的時間。期望 JSON 的請求回傳 `{"status":"up"}`。HEAD 不傳送本文。

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

端點會分派 `openrust::foundation::events::DiagnosingHealth`。可以註冊同步監聽器檢查資料庫或快取等相依服務。監聽器失敗時，框架回報錯誤並回傳 500，回應為「Application experiencing problems」頁面或 `{"status":"down"}`。除錯模式下由一般例外處理器算繪錯誤。健康頁面內建樣式。維護模式下仍可存取設定的健康路徑。

## 查看路由清單

使用 `route:list` 查看生效的路由，以 `--json` 輸出結構化結果。可依路徑、方法、名稱、網域、處理器或中介層篩選。`--except-path` 接收以逗號分隔的子字串。`--sort=definition` 保留註冊順序，`--reverse` 反轉結果。

```sh
./artisan route:list --json --method=HEAD
```

用 `-v` 查看各路由宣告的中介層。用 `-vv` 展開中介層群組，依請求執行順序顯示型別名稱，並套用排除規則及保留別名參數。輸出範圍為路由中介層。使用 `--json` 時，`middleware` 維持陣列，`--middleware` 依目前詳細等級的內容篩選。

```sh
./artisan route:list -vv --json --middleware=SubstituteBindings
```

路由清單保留 `{post:slug}` 等繫結欄位、選用標記和 `{tenant}.example.test` 等網域樣板。文字和 JSON 輸出皆包含透過 `setAction` 登記的控制器動作。相同方法、網域和路徑的後一次註冊會取代前一次，即使繫結欄位不同。其他 HTTP 方法仍使用原路由。

```sh
./artisan route:list --domain=example.test --action=PostController --json
```

## 路由快取 {#route-caching}

使用 `route:cache` 為部署後的應用程式產生比對索引。

```sh
./artisan route:cache
./artisan route:clear
```

快取依 HTTP 方法和路徑首段儲存候選項目。Rust 仍在啟動時註冊處理器、模型繫結和中介層。比對索引保留原註冊順序。索引遺失、無法讀取或與目前路由結構不符時，使用一般比對。路由變更後重新執行 `route:cache`，用 `route:clear` 移除索引。

預設路徑為 `storage/framework/cache/routes.json`。用 `APP_ROUTES_CACHE` 指定絕對路徑或相對於應用程式根目錄的路徑。使用設定快取時，將這個值設為服務的程序環境變數，因為啟動時可能略過 `.env`。

啟動後，`getCachedRoutesPath()` 回傳解析後的路徑，`routesAreCached()` 表示這次啟動是否載入了有效索引。

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

未設定 `CORS_PATHS` 時，`cors.paths` 沿用 預設路徑 `api/*` 和 `sanctum/csrf-cookie`。API 路由前綴與 CORS 路徑各自設定。若 API 掛載到 `/v1`，可設定 `CORS_PATHS=v1/*,sanctum/csrf-cookie` 為這些路徑啟用 CORS。設定 `CORS_PATHS=*` 可涵蓋所有路徑，設定 `CORS_PATHS=` 可停用 CORS。跨來源憑證預設關閉。路徑映射可按主機覆寫路徑，用數字字串鍵表示共用路徑。`allowed_origins_patterns` 使用 Rust 正規表示式引擎支援的模式。HTTP 核心在啟動時載入並驗證策略。

在 `withMiddleware` 中呼叫 `middleware.skipCorsWhen(|request| request.is("api/internal/*"));`，可在目前的應用程式中依請求略過 CORS。任一回呼傳回 `true` 時，`HandleCors` 將請求交給後續中介軟體並保留既有回應標頭。預檢 `OPTIONS` 同樣適用。

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

用 `middleware_name("throttle:30,0.5,uploads:")` 將額度設為每 30 秒 30 次。只寫 `throttle` 時預設每分鐘 60 次。`throttle:5|20,1,uploads:` 分別給訪客和已驗證使用者每分鐘 5 次和 20 次。訪客依路由宣告的網域和連線對端 IP 共用額度，已驗證請求依驗證識別碼共用額度。為不同規則設定不同的前綴即可分別計數。

使用 `throttle:5|upload_limit,1,uploads:` 可以從已驗證的使用者讀取 `upload_limit`，訪客仍使用固定配額。資料庫使用者提供器可讀取欄位。應用程式模型可透過 `Authenticatable::getAuthAttribute` 提供屬性，並委由 `Model::getAttributeValue` 套用存取器與型別轉換。配額接受有號 64 位元範圍內的非負數和數值字串。屬性缺失或非數字屬於設定錯誤。

### 使用 Redis 節流

在 `withMiddleware` 中呼叫 `middleware.throttleWithRedis()`，讓命名和數字節流別名使用預設 Redis 連線。使用相同 Redis 連線設定和鍵前綴的程序共用配額。Redis 原子檢查並消耗各項額度。使用 `after` 的規則在回應回傳後計數。

使用[預設 Redis 叢集](../database/redis.md#clusters)時，同一命名策略的計數器位於同一雜湊槽，Redis 可以一起檢查和計數。不同命名策略可以使用不同的槽。從舊叢集鍵格式升級後，命名策略開啟新的計數視窗，舊計數器依原到期時間清除。

## 路由模型繫結

使用 `Route::model::<Post>("post")?` 在應用程式 Router 註冊顯式繫結。也可以透過 Router 回傳的路由呼叫 `.model::<Post>("post")?`，為該路由宣告模型參數。使用 `request.route_model::<Post>("post")` 讀取已解析的模型。模型缺失回傳 404，可透過 `missing` 自訂回應。

模型預設按主鍵查詢。可覆寫 `getRouteKeyName` 使用 slug，也可為單條路由指定 `{post:slug}`。巢狀的自訂鍵透過前一個已繫結的模型限定查詢範圍。使用 `scopeBindings` 為預設鍵啟用作用域，或透過 `withoutScopedBindings` 關閉。子模型透過 `resolveChildRouteBinding` 選擇型別化關聯。`withTrashed` 可包含軟刪除記錄。預設 `web` 和 `api` 群組在處理器前執行繫結。

## 重新導向路由

使用 `Route::redirect` 註冊重新導向並指定 HTTP 狀態。`Route::permanentRedirect` 傳回 301。`/articles/{id}` 這類目的地可以沿用符合的路由參數。
