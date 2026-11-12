# HTTP 測試

## 分派請求

使用 [TestApp](getting-started.md) 設定應用程式，並在測試程序中經 HTTP 核心分派請求。空 GET 使用 `get`，需要指定方法、請求標頭與內容時，將 HTTP 請求傳給 `call`。

```rust
use openrust::{foundation::configuration::Routing, testing::TestApp};

#[tokio::test]
async fn health_route_responds() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| {
        builder.withRouting(Routing::default().health("/up"))
    })?;
    test.get("/up").await?.assertOk();
    Ok(())
}
```

## 檢查回應

`TestResponse` 提供狀態、回應標頭與內容。使用 `assertStatus`、`assertHeader`、`assertContent` 或 `assertExactJson` 斷言。內容擷取限制為 1 MiB，逾時為五秒。驗證連線中繼資料等傳輸行為時，啟動監聽器並使用 HTTP 用戶端。
