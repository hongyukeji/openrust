# 錯誤處理

## 報告例外

在 `bootstrap/app.rs` 的 `withExceptions` 閉包中設定報告。使用 `report::<E>` 比對例外型別或其來源。回呼依註冊順序執行，`ReportDecision::Stop` 停止後續回呼和預設報告。

可報告的例外預設以 `Level::Error` 寫入應用程式設定的日誌管道。HTTP 報告記錄狀態碼，預設訊息省略來源詳情。Artisan 保留 stderr 輸出和結束狀態。用 `level::<E>` 設定層級，用 `context` 附加記錄欄位。公開的 [Context](../digging-deeper/context.md) 欄位作為獨立中繼資料寫入。

### 忽略例外

預設忽略 HTTP 400、403、404 和 405。用 `dontReport::<E>` 忽略相符型別，`dontReportWhen` 依條件忽略，`stopIgnoring::<HttpError>` 恢復這些 HTTP 錯誤的報告。忽略規則只影響報告，回應仍依原流程轉譯。

### 例外實例去重

啟用 `dontReportDuplicates` 後，用 `SharedException` 包裝需要共用身分的例外。複製保留同一身分。可以明確呼叫 `Application::report`，也可將共用例外作為 `HttpError::Internal` 的來源傳回。訊息相同的獨立例外仍可分別報告。

```rust
use openrust::{
    contracts::debug::SharedException,
    foundation::Application,
    log::Level,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withExceptions(|exceptions| {
            exceptions
                .level::<std::io::Error>(Level::Warning)
                .dontReportDuplicates();
        })
        .create()?;
    app.boot().await?;
    let error = SharedException::new(std::io::Error::other("Job failed"));
    app.report(&error)?;
    app.report(&error.clone())?;
    Ok(())
}
```

## 轉譯例外

用 `render::<E>` 根據錯誤和請求傳回 HTTP 回應。傳回 `None` 時繼續嘗試後續回呼，最後回退至預設 JSON 或 HTML 轉譯。用 `respond` 調整最終例外回應，自訂和預設回應都會經過它。這兩個回呼僅用於 HTTP。

## 將例外轉譯為 JSON

應用程式在 `bootstrap/app.rs` 中透過 `withExceptions` 宣告例外策略。路徑符合 `api/*` 或請求預期 JSON 時，框架傳回 JSON 例外回應。其他請求收到 HTML 錯誤頁。

例如，存取不存在的 API 路由會傳回 `404` 和包含 `message` 欄位的 JSON。存取不存在的 Web 路由會傳回 HTML `404`，如果請求的 `Accept` 優先選擇 JSON 媒體類型，則傳回 JSON。

## 路由不存在

應用的 `withExceptions` 策略對符合 `api/*` 的路徑或期望 JSON 的請求回傳 JSON，其他缺失路由回傳 HTML。回應格式改變，404 狀態保持。缺失靜態檔案也使用此策略；HEAD 保留狀態和內容類型，本文為空。

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```
