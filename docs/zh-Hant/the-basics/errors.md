# 錯誤處理

可選業務 JSON 封裝、設定開關和錯誤範例見[業務回應](./responses.md#business-responses)。

## 傳播領域錯誤

對無須額外上下文的固定錯誤轉換，使用 `?`。例如在回傳 `HttpError` 的控制器或中介軟體中，可直接傳播組態讀取錯誤：

```rust
use openrust::{http::HttpError, support::facades::Config};

fn locale_form_field() -> Result<String, HttpError> {
    Ok(Config::string("app.locale_form_field")?)
}
```

組態、容器、翻譯、視圖、資料庫、日誌、Redis、Hash、JSON 序列化、I/O 與非同步工作排程錯誤進入 HTTP 邊界後保留原始來源，預設歸入 `Internal`。回應狀態為 500，由現有例外處理流程負責報告、偵錯顯示與正式環境脫敏。資料庫錯誤的預設轉換不會將 `ModelNotFound` 改為 404；回傳模型結果的路由仍按自己的既有規則處理。

圖片領域錯誤以及 `openrust-horizon`、`openrust-pulse`、`openrust-pennant` 的錯誤同樣預設轉換為 `Internal`，保留領域來源，不自動展開內嵌錯誤改變狀態碼。

底層模組繼續回傳自身領域錯誤。容器解析失敗可直接傳播到組態、翻譯、視圖、資料庫、快取、佇列、事件、日誌等對應的領域結果；認證中的資料庫、快取、Hash、Session、組態和容器錯誤保留為 `AuthError::Backend`。不要為了使用 `?` 讓通用元件依賴 HTTP。

`?` 只轉換目前回傳邊界的錯誤，不會自動逐層呼叫多個 `From`。回傳運算式需要時使用 `Ok(operation()?)`，或在回傳型別明確時使用 `map_err(Into::into)`。閉包有多個可能的錯誤型別時，明確宣告其 `Result` 型別。

保留需要路徑、操作名稱、請求或階段上下文的 `map_err`，以及狀態碼分類、復原、重試和脫敏邏輯。檔案系統、驗證、集合偵錯與認證已有專用 HTTP 轉換；強制包裝為 `Internal` 的呼叫不能在未核對語義時直接改成 `?`。

## 報告例外

在 `bootstrap/app.rs` 的 `with_exceptions` 閉包中設定報告。使用 `report::<E>` 比對例外型別或其來源。回呼依註冊順序執行，`ReportDecision::Stop` 停止後續回呼和預設報告。

透過相同的 `with_exceptions` 回呼設定[佇列工作依例外停止重試](../digging-deeper/queues.md#stopping-retries-by-exception)。

可報告的例外預設以 `Level::Error` 寫入應用程式設定的日誌管道。HTTP 報告記錄狀態碼，預設訊息省略來源詳情。Artisan 保留 stderr 輸出和結束狀態。用 `level::<E>` 設定層級，用 `context` 附加記錄欄位。公開的 [Context](../digging-deeper/context.md) 欄位作為獨立中繼資料寫入。

### 忽略例外

預設忽略 HTTP 400、403、404 和 405。用 `dont_report::<E>` 忽略相符型別，`dont_report_when` 依條件忽略，`stop_ignoring::<HttpError>` 恢復這些 HTTP 錯誤的報告。忽略規則只影響報告，回應仍依原流程轉譯。

### 例外實例去重

啟用 `dont_report_duplicates` 後，用 `SharedException` 包裝需要共用身分的例外。複製保留同一身分。可以明確呼叫 `Application::report`，也可將共用例外作為 `HttpError::Internal` 的來源傳回。訊息相同的獨立例外仍可分別報告。

```rust
use openrust::{
    contracts::debug::SharedException,
    foundation::Application,
    log::Level,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .with_exceptions(|exceptions| {
            exceptions
                .level::<std::io::Error>(Level::Warning)
                .dont_report_duplicates();
        })
        .create()?;
    app.boot().await?;
    let error = SharedException::new(std::io::Error::other("Job failed"));
    app.report(&error).await?;
    app.report(&error.clone()).await?;
    Ok(())
}
```

## 限流例外報告

在 `with_exceptions` 中透過 `throttle::<E>` 註冊策略。第一個符合且回傳策略的回呼生效。`ReportThrottle::Lottery(Lottery::odds(1, 100)?)` 用於取樣，`ReportThrottle::Limit` 用於固定視窗配額，`ReportThrottle::Unlimited` 允許直接報告。

將下方的 `configure` 傳給 `with_exceptions`，每分鐘最多報告兩次 I/O 例外。

```rust
use openrust::{
    cache::rate_limiting::Limit,
    foundation::configuration::{Exceptions, ReportThrottle},
};

fn configure(exceptions: &mut Exceptions) {
    exceptions.throttle::<std::io::Error>(|_| {
        Some(ReportThrottle::Limit(Limit::per_minute(2).by("transport")))
    });
}
```

省略 `by` 時依註冊的 Rust 例外類型計數。使用相同明確金鑰的策略在設定的限流快取中共用配額。快取操作失敗時繼續報告。限流在報告回呼和寫入記錄前執行，HTTP 狀態與本文、Artisan 的標準錯誤及結束碼仍依原有流程處理。

`Application::report` 是非同步方法，明確報告與 HTTP、Artisan 共用以快取為基礎的限流策略。

## 轉譯例外

用 `render::<E>` 根據錯誤和請求傳回 HTTP 回應。傳回 `None` 時繼續嘗試後續回呼，最後回退至預設 JSON 或 HTML 轉譯。用 `respond` 調整最終例外回應，自訂和預設回應都會經過它。這兩個回呼僅用於 HTTP。

### 自訂例外回應

可為預設 HTML 回應加入 UTF-8 頁面，例如 `resources/views/errors/404.html` 或 `resources/views/errors/500.html`。框架依設定的檢視路徑尋找。其他錯誤狀態可先回退到 `4xx.html` 或 `5xx.html`，再使用內建頁面。JSON 回應有自己的轉譯流程。

```rust
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, http::Kernel},
    http::{Body, HttpError, Request, Response, StatusCode},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Application::configure(std::env::current_dir()?)
        .with_exceptions(|exceptions| {
            exceptions
                .render::<HttpError>(|error, request| {
                    if matches!(error, HttpError::NotFound) && request.uri().path() == "/example" {
                        Some(Response::new(StatusCode::NOT_FOUND, Body::from("Example not found")))
                    } else {
                        None
                    }
                })
                .respond(|mut response, _, _| {
                    response.parts_mut().headers.insert("x-error", "handled".parse().unwrap());
                    response
                });
        })
        .create()?;

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await?;
    let (parts, ()) = http::Request::get("/example").body(())?.into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["x-error"], "handled");
    Ok(())
}
```

## 將例外轉譯為 JSON

框架預設依請求對 JSON 的偏好選擇回應。應用程式骨架在 `with_exceptions` 中為 最終生效的 API 路徑 和預期 JSON 的請求啟用 JSON 例外回應。可用 `should_render_json_when` 自訂策略。

正式環境設為 `APP_DEBUG=false`。通用 500 的預設 JSON 訊息為 `Server Error`，預設 HTML 頁面省略例外來源。偵錯模式下，內建伺服器錯誤回應包含有數量及長度上限的來源訊息，輸出為 HTML 時會逸出。自訂回呼負責控制所傳回的回應內容。

例如，存取不存在的 API 路由會傳回 `404` 和包含 `message` 欄位的 JSON。存取不存在的 Web 路由會傳回 HTML `404`，如果請求的 `Accept` 優先選擇 JSON 媒體類型，則傳回 JSON。

## 路由不存在

應用的 `with_exceptions` 策略對最終生效的 API 路徑或期望 JSON 的請求回傳 JSON，其他缺失路由回傳 HTML。回應格式改變，404 狀態保持。缺失靜態檔案也使用此策略；HEAD 保留狀態和內容類型，本文為空。

路徑規則比對生效前綴下的路徑，例如前綴 `/v1/internal` 下的 `/v1/internal/users`。前綴為空時，應用程式骨架依請求對 JSON 的偏好選擇例外回應。

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```

## HTTP 例外 {#http-exceptions}

在路由處理器或中介層傳回 `Err(abort(status, message, headers))`，即可中止處理並呈現 HTTP 例外。例外處理器保留狀態碼和回應標頭，並依請求選擇 HTML 或 JSON。訊息應適合直接傳回用戶端。非預期的內部錯誤仍遵循正式環境的去識別策略。

```rust
use openrust::http::{HeaderMap, HttpError, Response, StatusCode, abort};

async fn maintenance() -> Result<Response, HttpError> {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", "60".parse().unwrap());
    Err(abort(StatusCode::SERVICE_UNAVAILABLE, "Maintenance", headers))
}
```

## 自訂 HTTP 錯誤頁

加入依狀態碼命名的視圖，例如 `resources/views/errors/404.blade.html`。Blade 錯誤頁可讀取 `$status` 和 `$message`，也支援一般的 `errors/404.html`。可以用 `errors/4xx.blade.html` 或 `errors/5xx.blade.html` 處理同類錯誤。404、500 和 503 使用對應的專屬視圖或框架備援頁。回應沿用原有狀態碼與 JSON 協商規則。

## Exceptions 門面 {#exceptions-facade}

`Exceptions` 解析目前應用的 `dyn ExceptionHandler` 綁定。HTTP 中介軟體錯誤邊界、應用報告與 Artisan 失敗都解析同一契約；請求執行期間替換處理器後再拋錯，也使用新處理器。Worker 報告原本已解析此契約。綁定缺失時回傳錯誤，不會悄悄重用最初的具體處理器。

```rust
use openrust::{support::facades::Exceptions, log::LogResult};

async fn report_failure() -> LogResult<()> {
    Exceptions::report(std::io::Error::other("报告失败")).await
}
```

`report` 接收擁有所有權的 Rust 錯誤；`should_report`、`should_stop_retries`、`render` 與 `render_for_console` 轉發既有契約，`handler` 傳回原處理器 Arc，例外替身會被解包。`should_report` 檢查重複／忽略規則，並執行採樣或節流准入；重複查詢也可能消耗配額。每次實際報告只執行一次准入，請勿將它當作無副作用的狀態探針。啟動設定繼續寫在 `with_exceptions`。執行期現已提供 `reportable`、`renderable`、`dont_report`、`ignore`、`stop_ignoring`、`dont_report_when`、`dont_retry`、`dont_retry_when`、`dont_flash`、`level`、`build_context_using`、`respond_using`、`should_render_json_when`、`dont_report_duplicates` 與帶型別的 `throttle_using`。`is_reporting` 查詢同一例外實例是否正在被報告，不是報告資格查詢，也不消耗報告節流配額。自訂處理器須明確支援執行期設定及此唯讀查詢，否則回傳錯誤。例外替身與斷言見下文。

### 可報告回呼控制代碼 {#reportable-handlers}

`reportable` 回傳已註冊的 `ReportableHandler`。回呼回傳 `ReportDecision::Stop` 時，本次報告不再執行後續回呼或預設日誌。呼叫 `.stop()` 也會設定此行為，註冊完成後修改控制代碼仍生效；控制代碼及其複本共用停止旗標。需要比對所有錯誤時使用 `reportable_all`，無須逐型別註冊。

```rust
use openrust::{contracts::container::BindingResolutionError, foundation::configuration::ReportDecision, support::facades::Exceptions};

fn configure_reports() -> Result<(), BindingResolutionError> {
    Exceptions::reportable::<std::io::Error>(|_| ReportDecision::Continue)?.stop();
    Exceptions::reportable_all(|_| ReportDecision::Stop)?;
    Ok(())
}
```

`handles` 用於檢查型別回呼是否比對某個 `Exception`；`invoke` 遇到不相符型別會回傳錯誤。停止旗標不阻止目前回呼本身執行，而是在它回傳後阻止後續報告。既有啟動設定 `exceptions.report(...)` 保留原來的鏈式設定回傳值，需要控制代碼時使用啟動設定 `exceptions.reportable(...)`。

### 例外測試 {#exception-testing}

在目前應用門面作用域中呼叫 `Exceptions::fake()`，或使用 `TestApp::fake_exceptions(&[])` 保留傳回的句柄，跨 HTTP、Artisan 呼叫進行斷言。空選擇列表記錄所有可報告例外。`ExceptionType::of::<E>()` 精確選擇報告的根型別；`including_sources::<E>()` 才明確檢查錯誤來源鏈。型別斷言仍匹配根型別，不將巢狀來源錯誤算作另一筆報告。

```rust
use openrust::{http::HttpError, testing::TestApp};

async fn check_reports(app: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let exceptions = app.fake_exceptions(&[]).await?;
    let _response = app.get("/failure").await?;
    exceptions.assert_reported::<HttpError>();
    exceptions.assert_reported_where::<HttpError>(|error| error.to_string().contains("failure"));
    exceptions.assert_reported_count(1);
    Ok(())
}
```

句柄與門面都提供 `assert_reported`、`assert_not_reported`、對應的 `_where` 條件斷言、`assert_reported_count` 與 `assert_nothing_reported`。`reported()` 傳回共享的原始錯誤；執行條件回呼時不持有記錄列表鎖。HTTP 渲染、驗證重新導向與主控台渲染仍使用原處理器，選中並記錄的報告不再重複寫入其日誌，未選中的錯誤正常委派。`handler()` 傳回替身背後的處理器；重複安裝替身使用新的記錄列表，不巢狀包裝舊替身。`set_handler` 更換委派處理器並保留既有記錄。

`Exceptions::report(error)` 與 `Application::report_owned(error)` 接收錯誤所有權，HTTP、Artisan 與 Worker 邊界也保留共享錯誤。自訂呼叫端若只有借用的非透明錯誤，須先持有 `SharedException` 再借用；替身無法安全保存任意短期借用，因此明確傳回錯誤，不降級為字串快照。不同應用與並行任務不會共用替身記錄。

`throw_on_report()` 會在記錄後中止受保護的測試呼叫。HTTP 呼叫透過自身的 `Result` 傳回原始錯誤；`TestApp::try_artisan(args)` 傳回 `Result<TestCommand, SharedException>`，也包括 `queue:work` 報告的任務錯誤。相容的 `artisan(args)` 仍傳回 `TestCommand`；明確啟用重拋模式時，它以攜帶原始 `SharedException` 的 panic 重拋。需要可失敗呼叫的 Rust 測試優先使用 `try_artisan`。

其他非同步入口可用 `Exceptions::catch_reported(future).await` 包裹。每次捕獲使用詞法任務區域通道，報告後傳回原始錯誤並取消本次呼叫，後續程式碼不再執行；巢狀捕獲恢復外層作用域，並行請求與取消不會遺留通道。捕獲作用域外直接 `report(...).await` 會傳回含原始錯誤的 `Err`，不會掛起任務。`throw_first_reported()` 傳回首筆報告的錯誤，空列表則成功。此測試控制不會將正式環境日誌失敗改為 panic。

### 例外上下文 {#exception-context}

`Exceptions::register()` 轉發實際處理器的註冊鉤子，標準處理器沒有額外註冊工作。自訂處理器可以覆寫，例外替身會繼續轉發給原處理器。

`exception_context::<E>(callback)` 為具體 Rust 錯誤註冊欄位。錯誤本身有 `context()` 方法時，明確傳入 `|error| error.context()`；`std::error::Error` 未定義此可選方法。`build_context_using(callback)` 保留單參數便捷用法；`build_context_using_with_context(callback)` 同時接收例外和此前累積的欄位，按註冊順序執行，後傳回的同名欄位覆蓋前值。

`build_context_for_exception(&exception)` 傳回例外專屬欄位。`context_for_exception(&exception).await?` 傳回 `ExceptionContext`：`fields` 保存合併欄位，`exception` 保留原始例外借用，不將錯誤物件偽裝成 JSON 值。預設上下文在例外欄位後合併目前預設 Guard 已解析的使用者 ID，因此其 `userId` 優先；假值識別碼會被省略。HTTP 錯誤邊界會在記錄前惰性解析預設 Guard。認證失敗保留原例外報告；認證內部再次報告不會遞迴解析同一應用的 Guard。巢狀應用與取消後的呼叫各自恢復上下文。

實際日誌處理器使用同一個上下文建構器。`SharedException` 傳輸包裝不會在 JSON 日誌中多出一層原因，也不會讓原錯誤的上下文回呼執行兩次；真實來源錯誤仍保留在例外鏈中。其他應用或請求讀取、報告上下文時不會重用此前已認證身分。

路由已持有 `&mut Request` 時，使用 `context_for_exception_with_request(request, &exception).await?` 或 `report_for_request(request, error).await`；也可透過 `request.with_container(...)` 借出請求後使用環境門面。如此明確處理可變借用，惰性認證不依賴不安全的全域請求參考。無請求參數的上下文入口會嘗試解析可存取的環境請求；HTTP 之外不附帶請求身分。

自訂處理器若未實作 `register`、`build_context_for_exception` 或 `context_for_exception`，會明確傳回不支援此能力的錯誤，經過替身也相同；只有已實作操作且確實沒有欄位時才傳回空上下文。

### 例外映射 {#exception-mapping}

`Exceptions::map::<來源型別, 目標型別>(callback)` 註冊型別轉換，啟動設定對應 `exceptions.map::<來源型別, 目標型別>(callback)`。僅套用首個匹配映射；同一來源型別重新註冊會覆蓋回呼並保留原順序，結果不會遞迴映射。Rust 型別與建構回呼取代動態類別名稱字串，來源／目標型別無效時由編譯器拒絕。

對 Rust 列舉，可用 `map_optional::<E, 目標型別>(callback)` 選擇變體：匹配時傳回 `Some(mapped)`，不匹配時傳回 `None`，保留原錯誤並繼續檢查後續映射。如此僅修改某種狀態時，不必轉換所有 `HttpError` 變體。

```rust
use openrust::{contracts::container::BindingResolutionError, support::facades::Exceptions};

fn configure_mapping() -> Result<(), BindingResolutionError> {
    Exceptions::map::<std::num::ParseIntError, _>(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, error.clone())
    })?;
    Ok(())
}
```

實際報告在忽略規則、報告回呼與日誌之前套用映射。渲染單獨套用映射，因此一次 HTTP 失敗可能在報告、渲染階段各呼叫一次。`should_report` 查詢傳入的例外，不先執行映射；選中的替身記錄原始例外，渲染仍委派原處理器套用映射。

映射後的根型別決定報告、渲染與忽略規則；保留的來源錯誤僅作為診斷鏈，不再誤觸發舊型別的規則。映射為 `HttpError` 時保留其狀態與回應處理。`should_render_json_when` 現在接收真實 `Exception`，包括映射後的業務例外；需要按型別判斷時使用 `downcast_ref::<E>()`，不會用合成 HTTP 錯誤取代業務錯誤。

包裝錯誤具有可選內層例外時，可用 `inner_exception::<包裝型別>(callback)` 傳回 `Option<SharedException>`，明確接入 Rust `Error` 未定義的內層錯誤方法。存在內層錯誤時優先於普通映射，直接傳回且不再次映射；`None` 繼續匹配。複製共享內層句柄可保留原始實例。

### 關閉例外處理 {#without-exception-handling}

使用 `without_exception_handling(&[])` 讓 HTTP 測試呼叫傳回未處理錯誤，`with_exception_handling()` 恢復最初的原處理器。非 Clone 錯誤仍保留真實的擁有所有權的實例。路由不存在時補充方法和不含查詢參數的 URL，原始 `HttpError::NotFound` 留在來源鏈中。

```rust
use openrust::{contracts::debug::Exception, http::HttpError, testing::TestApp};

async fn check_unhandled(app: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.without_exception_handling(&[]).await?;
    let error = app.get("/missing").await.expect_err("expected an unhandled error");
    assert!(Exception::Error(error.as_ref()).downcast_chain_ref::<HttpError>().is_some());
    app.with_exception_handling().await?;
    Ok(())
}
```

`handle_exceptions(&types)` 保留所選型別的正常渲染，`handle_validation_exceptions()` 保留正常驗證回應。`ExceptionType::matching::<HttpError>(predicate)` 可選擇 Rust 列舉的具體變體。重複切換不會巢狀包裝舊的關閉處理器。關閉前或關閉後安裝的 Fake 都保留記錄列表；關閉時記錄原本被忽略的例外，恢復後重新遵守原報告規則。主控台失敗仍正常渲染，其他應用保持各自模式。

正式 `Exceptions::render` 傳回可失敗的裝箱錯誤結果，關閉處理時保留原始共享錯誤。借用的不透明錯誤無法長期保存：請傳入 `SharedException`、使用擁有錯誤所有權的 HTTP 測試入口，或處理獨立的 `ExceptionOwnershipError`。`SharedException::from_exception` 複製既有共享擁有者；再次將共享錯誤傳給 `new`／`from_arc` 時保留原身分，包括映射重複傳回同一目標時，確保重複報告判斷正確。

`Exception::downcast_chain_ref::<E>()` 明確讀取診斷來源，包括映射前例外；`including_sources` 選擇器也使用它。映射後的規則仍僅匹配新根。來源搜尋最多檢查 32 個節點，循環 `source()` 不會掛起測試或報告。

`TestApp` 自動捕獲未處理錯誤。直接呼叫底層 Kernel 的 Future 時，用 `Exceptions::catch_reported(future).await` 包裹；僅傳回 `Response` 的舊處理器方法在明確關閉模式下以 `SharedException` 為載荷 panic。需要 `Result` 時使用可失敗的門面或測試入口。

### 所有例外的限流策略 {#global-exception-throttle}

透過 `Exceptions::throttle_using_all` 註冊全域策略，或在啟動設定中使用 `configuration.throttle_all`。它與 `throttle_using::<E>` 共用有序回呼集合。回傳 `None` 繼續尋找；回傳 `Unlimited` 明確放行並停止尋找。

```rust
Exceptions::throttle_using_all(|exception| {
    if exception.downcast_ref::<TransientFailure>().is_some() {
        Some(ReportThrottle::Limit(Limit::per_minute(5)))
    } else {
        None
    }
})?;
```

未指定 `.by(...)` 時依實際 Rust 錯誤型別分組，不依訊息分組。擁有錯誤的 `Exceptions::report(error)` 自動保留型別；借用查詢使用 `Exception::typed(&error)`，具體型別的 `Arc<E>` 使用 `SharedException::from_typed_arc(error)`。映射後的報告依目標型別計費；明確的 `.by("supplier-timeout")` 可讓不同型別共用配額。

已抹除型別的 `Arc<dyn Error + Send + Sync>` 或 `Exception::Error(&dyn_error)` 無法還原 Rust 具體型別名稱。請明確指定 `.by(...)`，或使用上述建構器保留型別。全域無鍵策略遇到缺少型別資訊時沿用限流失敗放行規則，保留原例外報告，不會把所有未知型別悄悄混入同一配額。原有具型別策略仍可為舊借用呼叫推斷符合的型別。
