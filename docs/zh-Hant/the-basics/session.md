# 工作階段

## 設定

用 `SESSION_DRIVER=database` 將工作階段儲存在設定的資料庫和 `sessions` 資料表中。使用工作階段路由前先執行應用程式遷移。`array` 驅動程式將資料保存在目前的應用程式程序中。用 `SESSION_ENCRYPT=true` 加密儲存內容。

設定工作階段驅動程式後，預設 `web` 群組會在處理器執行前啟動工作階段。自訂群組需要將 `EncryptCookies` 放在 `StartSession` 前面。

## 使用工作階段

在路由中介軟體堆疊中依序設定 `EncryptCookies` 和 `StartSession`，再用 `request.session()` 或 `request.session_mut()` 存取已載入的工作階段。路由回傳後，中介軟體儲存工作階段並附加工作階段 Cookie。匯入 `Session` trait 即可呼叫工作階段方法。

```rust
use openrust::{
    contracts::session::Session,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn remember(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let session = request.session_mut()?;
        session.put("profile.name", "Ada".into())?;
        session.flash("message", "Saved".into())?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

## 快閃資料

`flash` 寫入的值在目前請求和下一次請求可用。用 `keep` 延長指定欄位的可用時間，或用 `reflash` 將所有快閃資料保留到再下一次請求。

## 重新產生工作階段 ID

`regenerate(true)` 刪除儲存中的舊 ID、保留工作階段屬性並輪替 CSRF 權杖。用 `invalidate()` 清除工作階段屬性並輪替 ID。
