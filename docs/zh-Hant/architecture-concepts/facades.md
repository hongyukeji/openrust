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

未載入的延遲 Provider 或非同步工廠使用 `getFacadeRootAsync().await`。它直接走一次非同步容器解析，等待就緒並共用目前應用的門面快取；期間出現較新的 `swap` 或其他已快取根物件時保留該值。同步解析未就緒的 Provider 或尚未建構的非同步工廠會回傳型別化的非同步解析錯誤。

暫停的門面 Future 屬於原應用執行情境，移入另一請求、命令或應用繼續 poll 會回傳 `FacadeNotSet`。生命週期結束會清空快取；保留的容器解析不能重新開啟已關閉作用域（`ScopeClosed`）。銷毀工作時還原仍存活的原情境，原情境關閉後遮蔽環境門面。應用明確保留的服務與 Future 仍由應用持有。參閱[非同步工廠](container.md#非同步工廠)與[延遲服務提供者](providers.md#延遲服務提供者)。
