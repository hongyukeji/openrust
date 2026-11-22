# HTTP 用戶端

## 傳送請求 {#making-requests}

應用程式處理器可使用 `openrust::support::facades` 中的 `Http`。`Http::get(url).await?` 回傳帶有狀態碼的回應。用 `status`、`successful` 或 `failed` 檢查狀態，`throw()` 將 4xx 或 5xx 轉為附帶回應的錯誤。`json::<T>()` 用於型別化解碼，`body()` 回傳緩衝的位元組。

用 `Http::acceptJson()?` 或 `Http::pendingRequest()?` 建立可設定的請求。POST、PUT、PATCH 和 DELETE 資料以借用方式傳入，預設編碼為 JSON。用 `asForm`、`withBody` 或 `attach` 更換編碼。`withQueryParameters` 設定編碼後的查詢參數，省略時保留 URL 內的查詢字串。逾時使用 `Duration`，可用 `maxResponseBytes` 限制回應緩衝。預設連線設定下，同一應用程式共用連線池。

請求和回應回呼使用 Rust 型別。透過請求建構器明確選擇傳輸設定，可用 Rust 函式或擴充 trait 重複使用具名設定。

### Digest 驗證 {#digest-authentication}

使用 `withBasicAuth` 設定 Basic 憑證，使用 `withToken` 設定 Bearer 權杖。使用 `withDigestAuth` 回應原始來源的 Digest 挑戰。Digest 會重播已緩衝的請求，伺服器回報 nonce 過期時可再嘗試一次。只有最終回應進入 `afterResponse`。下例使用本機 `GET /digest`，該端點使用範例憑證並傳回內文 `authenticated`。

```rust
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let response = Http::withDigestAuth("example-user", "example-password")?
    .get(&format!("{origin}/digest"))
    .await?
    .throw()?;
assert_eq!(response.body(), b"authenticated");
```

## 重試與測試 {#retries-and-testing}

`retry` 接收總嘗試次數。HTTP 失敗預設將最後一份回應作為錯誤回傳。`throwOnRetryFailure(false)` 直接回傳該回應，傳輸失敗仍回傳錯誤。一次性請求本文需要為每次獨立傳送重新提供。`Factory::new` 可獨立使用，並隔離 fake 和請求記錄。用 `preventStrayRequests(true)` 讓未比對的測試請求在建立連線前失敗。

```rust
use http::{HeaderMap, StatusCode};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::time::Duration;

let factory = Factory::new()?;
factory.fakeSequence("example.test/catalog")
    .push(Ok(Response::new(StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new(), Vec::new())))
    .push(Response::from_json(StatusCode::OK, &json!({"items": ["rust"]})));
factory.preventStrayRequests(true);

let response = factory.pendingRequest()
    .acceptJson()
    .retry(2, Duration::ZERO)
    .get("https://example.test/catalog")
    .await?
    .throw()?;
assert_eq!(response.json::<Value>()?["items"][0], "rust");
factory.assertSentCount(2);
```

用 `pool` 設定非零並行上限，或用 `NonZeroUsize::MAX` 同時啟動所有請求。結果依完成順序排列。`poolOrdered` 同時啟動所有請求，並依註冊順序傳回每項結果。用 `r#as` 為請求命名，或用 `newRequest` 分配字串形式的下一個數字鍵。

### URI 範本、重試回呼和 URL 放行 {#uri-templates-retry-and-allowance}

用 `withUrlParameters` 在傳送請求前展開 RFC 6570 運算式。JSON 映射可傳入字串、數字、布林值、清單或關聯物件。空值會省略對應變數。查詢物件使用展開修飾符時，巢狀欄位採用方括號鍵。前綴長度依 Unicode 字元計算。重複呼叫時，同名參數採用最後一次設定。傳送含有字面大括號的 URL 時，可以省略範本參數。

呼叫 `retryWhenRequest` 檢查失敗並修改待重播的請求，例如在 401 後替換授權標頭。傳回 `Ok(true)` 繼續重試、`Ok(false)` 停止，或傳回錯誤以中止。修改會保留到後續嘗試。

啟用 `preventStrayRequests(true)` 後，可用 `allowStrayRequests` 替換允許傳送實際請求的 URL 模式。符合條件的 fake 優先，每一跳重新導向都會檢查。

將 `HTTP_CLIENT_EXAMPLE_ORIGIN` 設為本機服務。該服務對範例 URL 傳回 401，收到 `x-replay: ready` 標頭後傳回內文 `retried`。

```rust
use openrust::{http::client::Error, support::facades::Http};
use serde_json::json;
use std::time::Duration;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
Http::preventStrayRequests(true)?;
Http::allowStrayRequests([format!("{origin}/retry/*")])?;

let parameters = [
    ("origin".to_owned(), json!(origin)),
    ("item".to_owned(), json!("item/id")),
    ("tag".to_owned(), json!("x y")),
].into_iter().collect();

let response = Http::withUrlParameters(parameters)?
    .retry(2, Duration::ZERO)
    .retryWhenRequest(|error, next| {
        if matches!(error, Error::RequestFailed(response) if response.unauthorized()) {
            next.headers_mut().insert("x-replay", "ready".parse().expect("valid header"));
            return Ok(true);
        }
        Ok(false)
    })
    .get("{+origin}/retry/{item}{?tag}")
    .await?;
assert_eq!(response.body(), b"retried");
assert!(matches!(Http::get(&format!("{origin}/blocked")).await, Err(Error::StrayRequest)));
```

## 批次和事件 {#batches-and-events}

用 `Http::batch` 註冊請求，再呼叫 `send().await`。用 `concurrency` 設定同時執行的請求上限。`before` 在第一個請求前執行，`progress` 接收 2xx 回應，`catch` 接收 4xx、5xx 回應或單項錯誤。3xx 回應只增加已完成計數。沒有失敗時執行 `then`，`finally` 一律依註冊順序接收結果。

在待傳送請求上呼叫 `throw()` 或 `throwIf`，可將符合條件的最終 4xx、5xx 回應轉成單項錯誤。`defer()?` 消費持有自有資料的批次，將它排在目前的 HTTP 請求、Artisan 命令或佇列任務結束時執行。預設在任務成功後執行。若要在任務失敗後也執行，可對延後回呼呼叫 `always()`。

執行下例時，將 `HTTP_CLIENT_EXAMPLE_ORIGIN` 設為本機服務位址，讓 `GET /ok` 傳回 200、`GET /missing` 傳回 404。

```rust
use std::num::NonZeroUsize;
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let ok = format!("{origin}/ok");
let missing = format!("{origin}/missing");
let pool = Http::pool(NonZeroUsize::MIN, |pool| {
    pool.newRequest(|request| request.get(&ok))
        .r#as("missing", |request| request.get(&missing));
}).await?;
assert!(pool["0"].as_ref().is_ok_and(|response| response.ok()));
assert!(pool["missing"].as_ref().is_ok_and(|response| response.notFound()));

let batch = Http::batch(|batch| {
    batch
        .r#as("ok", |request| request.get(&ok))
        .r#as("missing", |request| request.throw().get(&missing))
        .finally(|counts, results| {
            assert_eq!(counts.failed_requests, 1);
            assert_eq!(results.keys().map(String::as_str).collect::<Vec<_>>(), ["ok", "missing"]);
        });
})?;
let results = batch.send().await;
assert!(results["missing"].is_err());
```

### 送出請求事件 {#outgoing-request-events}

從 `openrust::http::client::events` 匯入 `RequestSending`、`ResponseReceived` 和 `ConnectionFailed` 以註冊監聽。應用程式工廠使用所屬應用程式的派發器。獨立工廠可透過 `withDispatcher` 指定派發器。用 `withAttributes` 附加本機中繼資料，監聽器可從請求快照讀取，這些資料留在用戶端。

每次向上游傳送請求前，`RequestSending` 先於單項請求的 `beforeSending` 回呼派發。收到最終回應時，`ResponseReceived` 在 `afterResponse` 之前派發，4xx 和 5xx 也會觸發。`ConnectionFailed` 帶有實際嘗試傳送的請求以及型別化的連線或逾時錯誤。重試會重複這個事件序列。被測試攔截的意外請求只派發 `RequestSending`。

執行下例時，在本機服務加入 `GET /redirect`，使它以 302 重新導向至 `/missing`。將 `HTTP_CLIENT_EXAMPLE_FAILURE` 設為拒絕連線的本機端點 URL。

```rust
use openrust::http::client::{Error, events::{ConnectionFailed, RequestSending, ResponseReceived}};
use openrust::support::facades::{Event, Http};

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let unavailable = std::env::var("HTTP_CLIENT_EXAMPLE_FAILURE")?;
let sending = r"OpenRust\Http\Client\Events\RequestSending";
let received = r"OpenRust\Http\Client\Events\ResponseReceived";
let failed = r"OpenRust\Http\Client\Events\ConnectionFailed";
let events = Event::fake(&[sending, received, failed])?;

assert!(Http::get(&format!("{origin}/redirect")).await?.notFound());
assert!(matches!(Http::get(&unavailable).await, Err(Error::Connection)));
let outgoing = events.dispatched(sending);
assert_eq!(outgoing.len(), 3);
assert_eq!(outgoing[1].downcast_ref::<RequestSending>().unwrap().request.url.path(), "/missing");
let replies = events.dispatched(received);
assert_eq!(replies.len(), 1);
assert!(replies[0].downcast_ref::<ResponseReceived>().unwrap().response.notFound());
let failures = events.dispatched(failed);
assert_eq!(failures.len(), 1);
assert_eq!(failures[0].downcast_ref::<ConnectionFailed>().unwrap().request.url.as_str(), unavailable);
```

## 全域中介軟體與傳輸耗時 {#global-middleware-and-timing}

在應用程式 Provider 中使用 `Http::globalMiddleware` 包裹每一跳出站請求；獨立用戶端可用 `Factory::globalMiddleware`。回呼接收擁有所有權的請求和續體 `next`，傳回 `Box::pin(async move { ... })`；`next.run(request).await` 傳回回應或包含逾時在內的傳輸錯誤。中介軟體按註冊順序進入、反序傳回，重新導向和重試都會再次經過。註冊作用於隨後建立的 PendingRequest，同一 Factory 的複本共用註冊，應用程式之間隔離。

`globalRequestMiddleware` 透過 `&mut Request` 修改請求並傳回 `Result<()>`；`globalResponseMiddleware` 將 `Response` 映射為 `Result<Response>`。單個 PendingRequest 使用 `withMiddleware`、`withRequestMiddleware`、`withResponseMiddleware`；既有 `beforeSending` 與 `afterResponse` 仍可使用。

```rust
use openrust::http::client::Http;

Http::globalMiddleware(|request, next| Box::pin(async move {
    let response = next.run(request).await?;
    let seconds = response.handlerStats().get("total_time").copied();
    // Use `seconds` with the application's metrics collector.
    Ok(response)
}))?;
```

`Response::handlerStats()` 的 `total_time` 以秒記錄最後一次網路傳輸及緩衝本文讀取的耗時，不包含重試間隔，也不是全部重新導向或嘗試的總時長。`response.transferStats.as_ref().map(|stats| stats.getTransferTime())` 提供相同耗時。fake 回應的傳輸時間為0且沒有網路 handler stats；未呼叫傳輸的中介軟體自建回應不具有傳輸統計。reqwest 不提供 cURL 分階段統計，因此不偽造 DNS、連線和 TLS 耗時。脫敏回應事件保留計時，不額外暴露本文或憑據。

## 請求選項

`replaceHeaders` 僅取代傳入的同名請求標頭，保留其他標頭與多值。`withHeader`、`contentType`、`asMultipart`、`maxRedirects` 沿用原請求建構鏈。`sink(path)` 將成功讀取的回應本文寫入檔案，寫入錯誤回傳 `Error::Sink`；目前仍緩衝本文並受 `body_limit` 限制，不適合無限串流或超大檔案下載。

`withUserAgent("OpenRust/1.0")` 設定 `User-Agent`，取代既有的同名標頭，並移除首尾 ASCII 空格、定位、換行、歸位、NUL 和垂直定位字元；Unicode 空格保留。參數為 Rust 字串，無效標頭值會在送出請求前傳回 `InvalidRequest`。PendingRequest、Factory 和 Http Facade 都提供這個入口。

```rust
let response = Http::withUserAgent("OpenRust/1.0")?.get(url).await?;
let response = Http::withoutVerifying()?.get(test_https_url).await?;
```

TLS 驗證預設開啟。`withoutVerifying()` 僅為目前建構的請求關閉憑證鏈與主機名稱驗證，不變更共享 Factory 或之後的預設請求；也可與 `connectTimeout` 組合。此入口用於明確接受不受信任憑證的受控端點。目前傳輸未整合 NTLM 驗證交握，尚未提供 `withNtlmAuth`；不能以 Basic、Digest 或直接組合 Authorization 標頭取代。

## Multipart 欄位值

`asMultipart().post(data)` 接受由純量欄位組成的物件。布林值按 PHP 方式編碼為 `1` 或空字串，null 編碼為空字串。巢狀物件或陣列傳回 `InvalidRequest`；檔案使用 `attach`。該入口不接收 Guzzle 全部 multipart 選項。
