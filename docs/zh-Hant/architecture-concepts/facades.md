# 門面

## 門面的運作原理

匯入 `openrust::support::facades::Facade`，定義門面的服務類型和 `getFacadeAccessor`。`getFacadeRoot` 透過目前應用容器解析存取器。使用顯式轉發方法替代 PHP 的 `__callStatic`，由 Rust 在編譯時檢查方法。

```rust
use openrust::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    support::facades::Facade,
};

struct Messages;
impl Facade for Messages {
    type Service = str;
    fn getFacadeAccessor() -> ServiceKey { "message".into() }
}
impl Messages {
    pub fn text() -> Result<String, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.to_string())
    }
}
```

在 Provider 中綁定 `str` 並設定 `message` 別名，然後在 Provider 鉤子、HTTP 處理器或 Artisan 命令內呼叫 `Messages::text()`。在框架應用作用域之外呼叫會回傳 `FacadeNotSet`。

門面解析結果在目前應用執行期間快取。`clearResolvedInstance(None)` 清除目前門面存取器的快取，`clearResolvedInstance(Some(key))` 清除指定存取器，`clearResolvedInstances()` 清除目前全部快取。`swap(instance)` 同時替換快取根物件和容器綁定。門面可以設定 `const cached: bool = false`，使新的解析結果不進入快取。

框架為應用引導、每個 HTTP 請求和每次 Artisan 呼叫設定門面作用域。同一應用中的巢狀執行共用作用域，不同應用彼此隔離。新請求或命令使用新的門面快取；註冊為單例的服務仍屬於該應用的容器。應用程式碼自行建立的 Tokio 任務不會自動繼承門面作用域，應向這些任務傳入已解析服務。

`Route::get` 與 `Artisan::command` 在路由模組的 `register()` 執行期間使用專用註冊作用域，屬於已交付的註冊門面。其他門面類型宣告存在，不代表對應服務已經實作。Mocking 輔助介面和解析回呼由後續單元交付。

根服務屬於尚未載入的延遲 Provider 時，使用 `getFacadeRootAsync().await`。它等待服務就緒，並使用同一應用的門面快取；Provider 尚未就緒時同步解析會回傳需要非同步解析的錯誤。參閱[延遲服務提供者](providers.md#延遲服務提供者)。
