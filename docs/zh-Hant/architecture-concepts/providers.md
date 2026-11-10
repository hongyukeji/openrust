# 服務提供者

## 註冊服務提供者

`bootstrap/providers.rs` 列出 HTTP 與 Artisan 共用的應用程式服務提供者，並登記 `app/providers/app_service_provider.rs` 中的預設 `AppServiceProvider`。應用程式可在這個 Provider 的 `register` 和 `boot` 中接入服務。

選定的 Kernel 載入設定，執行所有一般 Provider 的 `register` 後，再依序執行 `boot`。詳見[請求生命週期](lifecycle.md)。

## 撰寫 Service Provider

實作 `openrust::support::ServiceProvider`，在 `bootstrap/providers.rs` 列出 Provider。覆寫 `register` 綁定服務，覆寫 `boot` 在所有 Provider 註冊後初始化服務。兩者預設均成功且不執行操作，只需實作所需鉤子。`register` 和 `boot` 回傳非同步 Future，註冊相依服務和初始化均可等待 I/O。

Provider 按具體 Rust 類型去重，保留首次註冊。成功引導具備冪等性。註冊或引導失敗後不再執行後續階段，同一個應用不能重試已經部分完成的引導。錯誤標明 Provider 和階段，並保留原始錯誤供報告使用；重試初始化需建立新應用。

Provider 鉤子在所屬應用的門面作用域內執行。HTTP 與 Artisan 使用同樣的 Provider 生命週期，不同應用的執行作用域和容器彼此隔離。參閱[容器綁定](container.md#綁定)和[門面](facades.md#門面的運作原理)。

## 延遲服務提供者

實作 `openrust::contracts::support::DeferrableProvider`，透過 `provides()` 宣告精確服務鍵。在原有 Provider 清單中登記 `DeferredProvider::new(factory)`。編譯型工廠取代 PHP 按類名建構；靜態清單不會實例化 Provider。去重使用實際 Provider 類型。HTTP 不載入未使用的 Provider；Console Kernel 在註冊命令前呼叫 `loadDeferredProviders()`。

使用 `Application::make::<T>().await` 或 `get::<T>(key).await` 等待延遲 Provider。容器對應 `makeAsync` / `getAsync`，門面對應 `getFacadeRootAsync`。同步解析尚未初始化的延遲服務會回傳 `DeferredServiceRequiresAsync`，不會阻塞 Tokio。同步綁定工廠需接收已初始化的相依服務，不能在內部等待另一個 Provider。`register` 和 `boot` 都回傳 Future，註冊鉤子可以透過 `app.make().await` 載入另一個 Provider。

使用[非同步工廠](container.md#非同步工廠)可在建構內自動解析延遲相依服務。

```rust
use std::{error::Error, sync::Arc};
use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _, support::DeferrableProvider},
    foundation::Application,
    http::HttpFuture,
    support::{DeferredProvider, ProviderResult, ServiceProvider},
};

struct Client;
struct ClientProvider;
impl DeferrableProvider for ClientProvider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Client>()]
    }
}
impl ServiceProvider for ClientProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singletonAsync::<Client, _>(|_| Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Client))
            })).await?;
            Ok(())
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withProviders(vec![Box::new(DeferredProvider::new(|| ClientProvider))])
        .create()?;
    app.boot().await?;
    assert!(app.isDeferredService(ServiceKey::typed::<Client>()));
    let client = app.make::<Client>().await?;
    assert!(Arc::ptr_eq(&client, &app.make::<Client>().await?));
    Ok(())
}
```

已有明確實例會略過尚未使用的載入器；解析先正規化別名。並行呼叫共用一次註冊，並等待 boot 完成；boot 尚未就緒時，其他呼叫者不能取得其宣告的服務。boot 本身可以解析已經登記的本 Provider 服務。boot 失敗或取消後保留明確失敗狀態，需要建立新應用重試。這是針對 Rust 並行執行的就緒保護，超出 PHP 順序執行場景；已貢獻的容器值保留。

只消耗觸發鍵，同一 Provider 的其他鍵保留到 `loadDeferredProviders()` 成功結束。註冊失敗保留貢獻並消耗該鍵；後續尚未消耗的鍵透過編譯型工廠建構新實例重試。如果直接登記已建構物件並覆寫 `isDeferred()`、`provides()`，重試使用原物件；需要重新執行建構時使用 `DeferredProvider`。

在 boot 階段開始前首次載入的 Provider，同時進入 booting 回呼和已註冊 Provider 清單，因此 boot 會執行兩次，保留固定 Laravel 基線的行為。在一般 Provider 的 boot 中首次使用時，進入剩餘啟動清單一次；應用 boot 完成後首次使用，會等待註冊和 boot 均完成再回傳服務。事件觸發載入及其他生命週期回呼依後續單元推進。

需要讓具體型別無需服務工廠即可自動注入時，在 `register` 登記編譯建構中繼資料；它不會把型別標為已綁定，也不會實例化。參閱[自動建構式注入](container.md#自動建構式注入)。
