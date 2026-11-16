# HTTP 用戶端

## 傳送請求 {#making-requests}

應用程式處理器可使用 `openrust::support::facades` 中的 `Http`。`Http::get(url).await?` 回傳帶有狀態碼的回應。用 `status`、`successful` 或 `failed` 檢查狀態，`throw()` 將 4xx 或 5xx 轉為附帶回應的錯誤。`json::<T>()` 用於型別化解碼，`body()` 回傳緩衝的位元組。

用 `Http::acceptJson()?` 或 `Http::pendingRequest()?` 建立可設定的請求。POST、PUT、PATCH 和 DELETE 資料以借用方式傳入，預設編碼為 JSON。用 `asForm`、`withBody` 或 `attach` 更換編碼。`withQueryParameters` 設定編碼後的查詢參數，省略時保留 URL 內的查詢字串。逾時使用 `Duration`，可用 `maxResponseBytes` 限制回應緩衝。預設連線設定下，同一應用程式共用連線池。

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

從 `openrust::http::client::events` 匯入 `RequestSending`、`ResponseReceived` 和 `ConnectionFailed` 註冊監聽。事件名稱使用 `OpenRust\Http\Client\Events\` 前綴。應用 HTTP 用戶端在每次請求嘗試時派發事件，包括重試。
