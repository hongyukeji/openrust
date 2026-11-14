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

用 `pool` 設定非零並行上限，透過 `Pool::r#as` 註冊具名請求。結果保留註冊順序及各個請求的成功或錯誤。
