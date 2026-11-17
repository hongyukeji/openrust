# 工作階段

## 設定

用 `SESSION_DRIVER=database` 將工作階段儲存在設定的資料庫和 `sessions` 資料表中。使用工作階段路由前先執行應用程式遷移。`array` 驅動程式將資料保存在目前的應用程式程序中。用 `SESSION_ENCRYPT=true` 加密儲存內容。

設定驗證守衛後，資料庫驅動程式會在請求結束時將目前守衛的使用者 ID 寫入 `user_id`。訪客請求寫入 `NULL`。自訂工作階段資料表的 `user_id` 欄位型別應能儲存應用程式使用的使用者識別碼。

用 `SESSION_DRIVER=file` 將工作階段儲存在 `storage/framework/sessions`。可透過應用程式設定的 `session.files` 指定其他目錄。`native` 是 `file` 的別名。用 `SESSION_DRIVER=null` 在目前請求中使用工作階段，請求結束後捨棄工作階段狀態。

用 `SESSION_DRIVER=redis`、`memcached` 或 `dynamodb` 將工作階段儲存在快取後端。`SESSION_STORE` 選擇具名快取儲存，預設使用驅動程式名稱。Redis 的 `SESSION_CONNECTION` 選擇連線名稱，預設為 `default`。應用程式設定中的 `session.prefix` 可覆寫 Redis 工作階段使用的快取前綴。工作階段使用獨立的 Redis 儲存庫，一般快取保留自己的連線和前綴。後端會在 `SESSION_LIFETIME` 分鐘後使儲存內容失效。

用 `SESSION_DRIVER=cookie` 將工作階段儲存在兩個加密 Cookie 中。設定的工作階段 Cookie 保存 ID，以該 ID 命名的 Cookie 保存內容和有效期限。`SESSION_EXPIRE_ON_CLOSE` 讓瀏覽器在關閉時捨棄 Cookie，內容仍受有效期限約束。加密後的內容 Cookie 回應標頭上限為 4096 位元組。輪替並刪除舊工作階段時，目前的瀏覽器會收到舊內容 Cookie 的到期指令。複製的一對 Cookie 在有效期限內仍可重播。工作階段阻塞需要伺服器端驅動程式。

設定工作階段驅動程式後，預設 `web` 群組會在處理器執行前啟動工作階段。自訂群組需要將 `EncryptCookies` 放在 `StartSession` 前面。 Cookie 驅動程式還需要在兩者之間放置 `AddQueuedCookiesToResponse`。

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

`only` 篩選頂層鍵，`except` 從副本移除指定鍵或點號路徑。`exists` 和 `missing` 區分明確空值與缺少的鍵，`has` 和 `hasAny` 檢查非空值。`remember` 接收傳回 `Result` 的閉包，只在鍵缺少或值為空時運算並儲存。

用 `push` 向陣列或關聯陣列追加值。`increment` 和 `decrement` 接收明確的整數步長，傳回更新後的 JSON 值。輸入無效時傳回錯誤並保留原值。

用 `putMany` 同時寫入多個鍵，支援點號路徑。`replace` 更新指定鍵並保留其他屬性。`hasOldInput(Some(key))` 檢查指定快閃欄位是否非空，`hasOldInput(None)` 檢查舊輸入是否包含任何項目。

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

## 新增自訂工作階段驅動程式

實作 `SessionHandler`，負責讀取、寫入、刪除和回收工作階段記錄。處理器儲存框架提供的位元組。在服務提供者的 `boot` 方法中呼叫 `Session::extend` 註冊工廠。工廠可存取目前容器和請求，為這次請求建立處理器。

```rust
use std::time::Duration;
use openrust::{
    foundation::Application,
    http::HttpFuture,
    session::ArraySessionHandler,
    support::{ProviderResult, ServiceProvider, facades::Session},
};

struct AppSessionProvider;

impl ServiceProvider for AppSessionProvider {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            let storage = ArraySessionHandler::new(Duration::from_secs(120 * 60));
            Session::extend("app_memory", move |_, _| Ok(storage.clone()))?;
            Ok(())
        })
    }
}
```

將這個服務提供者註冊到應用程式並設定 `SESSION_DRIVER=app_memory`。範例將記錄保存在應用程式程序內，跨程序持久化可使用共用後端。
