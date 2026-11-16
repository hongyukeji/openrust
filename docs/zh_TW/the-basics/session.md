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

## 工作階段快取 {#session-cache}

在已啟動工作階段的路由中呼叫 `request.session()?.cache()?`。它與 `Cache::store(Some("session"))` 取得相同的儲存庫。快取項目隨目前的工作階段保存，支援常用快取操作和到期時間。`flush()` 只清除目前的快取命名空間，保留一般工作階段屬性與其他快取命名空間。重新產生工作階段 ID 會保留快取，使工作階段失效則會清空快取。

```rust
use openrust::{
    cache::Ttl,
    contracts::cache::Repository as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn cache_banner(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let cache = request.session()?.cache()?;
        cache.put("dashboard.banner", &"ready".into(), Some(Ttl::Seconds(300))).await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

`SESSION_CACHE_KEY` 設定命名空間，預設為 `_cache`。工作階段驅動程式會將快取與工作階段資料一併持久化。操作需要有效的 HTTP 工作階段，因此 `./artisan cache:clear session` 無法清除這些項目。同一工作階段的請求需要循序寫入時，可使用支援鎖定的快取儲存庫來啟用工作階段阻塞。

## 工作階段阻塞

預設情況下，使用同一個工作階段的請求可以並行執行。註冊工作階段路由時呼叫 `block(lock_seconds, wait_seconds)`，可以讓使用同一個工作階段 ID 的請求依序執行。

```rust
use openrust::support::facades::Route;

Route::getWith("/profile", remember, |route| {
    route.block(10, 10);
    Ok(())
})?;
```

第一個參數是鎖定的有效期，第二個參數是等待已被占用的鎖定的最長時間。中介軟體在讀取工作階段前取得鎖定，儲存後釋放。等待逾時交由例外處理器處理，預設回傳 HTTP 500。

用 `SESSION_BLOCK=true` 為所有啟動工作階段的路由啟用阻塞。用 `SESSION_BLOCK_LOCK_SECONDS` 和 `SESSION_BLOCK_WAIT_SECONDS` 設定預設值，兩者均預設為 10 秒。路由上的明確設定覆蓋預設值。`withoutBlocking()` 清除路由層級設定。啟用全域阻塞後，這條路由仍使用全域設定。

用 `SESSION_BLOCK_STORE` 選擇支援不可分割鎖定的快取儲存體，例如 `redis`。省略此設定時使用預設快取儲存體。多個應用程式程序需要使用共用儲存體，`array` 快取只協調同一個程序中的請求。
