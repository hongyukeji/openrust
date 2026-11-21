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
## 回應斷言

`assertJsonPath` 檢查點路徑，`assertJsonStructure` 檢查巢狀或萬用字元結構，`assertJsonValidationErrors` 檢查 JSON `errors` 物件中的欄位。`assertRedirect`、`assertSessionHas`、`assertDownload` 檢查實際回應及工作階段狀態。`assertCookie` 在傳入預期值時解密並驗證框架加密的字串 Cookie；未加密 Cookie 使用 `assertPlainCookie`。不提供 PHP Cookie 反序列化及 PHP 斷言的全部可選參數。斷言失敗會透過 panic 顯示不符的值或缺少的鍵。
