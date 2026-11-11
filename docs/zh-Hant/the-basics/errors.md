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
    app.report(&error).await?;
    app.report(&error.clone()).await?;
    Ok(())
}
```

## 限流例外報告

在 `withExceptions` 中透過 `throttle::<E>` 註冊策略。第一個符合且回傳策略的回呼生效。`ReportThrottle::Lottery(Lottery::odds(1, 100)?)` 用於取樣，`ReportThrottle::Limit` 用於固定視窗配額，`ReportThrottle::Unlimited` 允許直接報告。

將下方的 `configure` 傳給 `withExceptions`，每分鐘最多報告兩次 I/O 例外。

```rust
use openrust::{
    cache::rate_limiting::Limit,
    foundation::configuration::{Exceptions, ReportThrottle},
};

fn configure(exceptions: &mut Exceptions) {
    exceptions.throttle::<std::io::Error>(|_| {
        Some(ReportThrottle::Limit(Limit::perMinute(2).by("transport")))
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
        .withExceptions(|exceptions| {
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

框架預設依請求對 JSON 的偏好選擇回應。應用程式骨架在 `withExceptions` 中為 `api/*` 和預期 JSON 的請求啟用 JSON 例外回應。可用 `shouldRenderJsonWhen` 自訂策略。

正式環境設為 `APP_DEBUG=false`。通用 500 的預設 JSON 訊息為 `Server Error`，預設 HTML 頁面省略例外來源。偵錯模式下，內建伺服器錯誤回應包含有數量及長度上限的來源訊息，輸出為 HTML 時會逸出。自訂回呼負責控制所傳回的回應內容。

例如，存取不存在的 API 路由會傳回 `404` 和包含 `message` 欄位的 JSON。存取不存在的 Web 路由會傳回 HTML `404`，如果請求的 `Accept` 優先選擇 JSON 媒體類型，則傳回 JSON。

## 路由不存在

應用的 `withExceptions` 策略對符合 `api/*` 的路徑或期望 JSON 的請求回傳 JSON，其他缺失路由回傳 HTML。回應格式改變，404 狀態保持。缺失靜態檔案也使用此策略；HEAD 保留狀態和內容類型，本文為空。

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```

## 自訂 HTTP 錯誤頁

加入依狀態碼命名的視圖，例如 `resources/views/errors/404.blade.html`。Blade 錯誤頁可讀取 `$status` 和 `$message`，也支援一般的 `errors/404.html`。可以用 `errors/4xx.blade.html` 或 `errors/5xx.blade.html` 處理同類錯誤。404、500 和 503 使用對應的專屬視圖或框架備援頁。回應沿用原有狀態碼與 JSON 協商規則。
