# 門面

## 門面的運作原理

匯入 `openrust::support::facades::Facade`，定義門面的服務類型和 `get_facade_accessor`。`get_facade_root` 透過目前應用容器解析存取器。使用顯式轉發方法替代 PHP 的 `__callStatic`，由 Rust 在編譯時檢查方法。

```rust
use openrust::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    support::facades::Facade,
};

struct Messages;
impl Facade for Messages {
    type Service = str;
    fn get_facade_accessor() -> ServiceKey { "message".into() }
}
impl Messages {
    pub fn text() -> Result<String, BindingResolutionError> {
        Ok(Self::get_facade_root()?.to_string())
    }
}
```

在 Provider 中綁定 `str` 並設定 `message` 別名，然後在 Provider 鉤子、HTTP 處理器或 Artisan 命令內呼叫 `Messages::text()`。在框架應用作用域之外呼叫會回傳 `FacadeNotSet`。

門面解析結果在目前應用執行期間快取。`clear_resolved_instance(None)` 清除目前門面存取器的快取，`clear_resolved_instance(Some(key))` 清除指定存取器，`clear_resolved_instances()` 清除目前全部快取。`swap(instance)` 同時替換快取根物件和容器綁定。門面可以設定 `const CACHED: bool = false`，使新的解析結果不進入快取。

框架為應用引導、每個 HTTP 請求和每次 Artisan 呼叫設定門面作用域。同一應用中的巢狀執行共用作用域，不同應用彼此隔離。新請求或命令使用新的門面快取；註冊為單例的服務仍屬於該應用的容器。應用程式碼自行建立的 Tokio 任務不會自動繼承門面作用域，應向這些任務傳入已解析服務。

`Route::get` 與 `Artisan::command` 在路由模組的 `register()` 執行期間使用專用註冊作用域。服務門面須在應用程式生命週期內使用，並先註冊對應服務。

未載入的延遲 Provider 或非同步工廠使用 `get_facade_root_async().await`。它直接走一次非同步容器解析，等待就緒並共用目前應用的門面快取；期間出現較新的 `swap` 或其他已快取根物件時保留該值。同步解析未就緒的 Provider 或尚未建構的非同步工廠會回傳型別化的非同步解析錯誤。

暫停的門面 Future 屬於原應用執行情境，移入另一請求、命令或應用繼續 poll 會回傳 `FacadeNotSet`。生命週期結束會清空快取；保留的容器解析不能重新開啟已關閉作用域（`ScopeClosed`）。銷毀工作時還原仍存活的原情境，原情境關閉後遮蔽環境門面。應用明確保留的服務與 Future 仍由應用持有。參閱[非同步工廠](container.md#非同步工廠)與[延遲服務提供者](providers.md#延遲服務提供者)。

`make_with` 進行一次明確建構，不取代單獨快取的門面根物件；需要該結果時直接透過應用或容器解析。參閱[參數覆寫](container.md#參數覆寫)。

## 解析回呼與應用作用域 {#facade-lifecycle}

`resolved(callback)` 觀察實際後端服務。所屬應用已經解析它時立即回呼，未來容器重新解析時也回呼；註冊回呼不會提前建立服務。參數為 `Arc<Service>` 和 `&Container`，可直接保留同一個服務控制代碼，不必再次解析。回呼錯誤向上傳遞，即時回呼失敗時不會繼續註冊。非同步工廠完成解析也會觸發同一個掛鉤。

`App::resolved(callback)` 是公共門面回呼。查詢容器服務是否已解析時用 `App::is_resolved(key)`，底層容器保留 `resolved(key)`。查詢方法使用不同名稱，避免遮蔽回呼入口。

```rust
use openrust::{contracts::container::BindingResolutionError, foundation::Application,
    support::facades::{App, Config, Facade}};

pub async fn inspect(app: &Application) -> Result<(), BindingResolutionError> {
    App::set_facade_application(Some(app), async {
        Config::resolved(|repository, _container| {
            let _name = repository.get("app.name");
            Ok(())
        })?;
        Ok(())
    }).await
}
```

一般 HTTP、Artisan 和 Worker 已有應用作用域。嵌入應用或測試中，`set_facade_application(Some(&app), future).await` 只切換給定操作，結束、取消或 panic 展開後還原外層應用；`None` 暫時隱藏應用。`get_facade_application()` 傳回 `Result<Option<Arc<Application>>, BindingResolutionError>`，作用域外為 `None`。同應用巢狀使用保留門面快取，不同應用使用獨立快取。明確建立的 Tokio 任務也可用此入口開啟自己的作用域。

帶作用域的 setter 是全域應用 setter 的 Rust 配接：用明確的生命週期避免程序級修改影響並行請求，不在應用之間合併快取，也不延長已結束作用域。解析回呼屬於應用容器，應在啟動階段註冊持久回呼，避免捕獲只應存活於目前請求的資料。

`is_fake()` 只檢查已快取的門面根，不觸發解析。Mail、Notification 和 Event 傳回實際 Fake 狀態，Event 暫時替身也生效。清空門面快取後，在重新解析該根之前傳回 false。HTTP、Process 的樁仍屬於一般工廠，傳回 false；記錄呼叫的 spy 本身也不等於 Fake 標記。自訂門面可覆寫型別安全的 `is_fake_root` 掛鉤，識別自身服務的 Fake 狀態。

## 應用程式門面 {#application-facade}

`App` 存取目前應用程式及其容器。`bind`、`singleton`、`scoped` 及對應的 `_if`、`_named` 註冊方法委託給同一容器，不建立第二份註冊表。`make`、`get` 和 `make_with` 使用非同步介面，保留延遲 Provider 與非同步工廠的解析行為。

```rust
use std::sync::Arc;
use openrust::support::facades::App;

struct Greeting(String);

App::singleton::<Greeting, _>(|_| Ok(Arc::new(Greeting("Hello".into()))))?;
let greeting = App::make::<Greeting>().await?;
assert_eq!(greeting.0, "Hello");
```

用 `base_path`、`public_path`、`storage_path`、`lang_path` 和 `join_paths` 取得應用程式路徑。`environment` 回傳目前環境或比對指定模式；`is_local`、`is_production` 和 `running_unit_tests` 查詢相同的環境狀態。`get_bindings` 回傳獨立快照。解析掛鉤、擴充器、別名及實例清除沿用容器既有的生命週期。

請求感知用法見[回應門面](../the-basics/responses.md#response-facade)及[工作階段門面](../the-basics/session.md#session-facade)。
