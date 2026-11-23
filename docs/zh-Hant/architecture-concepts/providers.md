# 服務提供者

## 註冊服務提供者

`bootstrap/providers.rs` 列出 HTTP 與 Artisan 共用的應用程式服務提供者，並登記 `app/providers/app_service_provider.rs` 中的預設 `AppServiceProvider`。應用程式可在這個 Provider 的 `register` 和 `boot` 中接入服務。

選定的 Kernel 載入設定，執行所有一般 Provider 的 `register` 後，再依序執行 `boot`。詳見[請求生命週期](lifecycle.md)。

## 產生服務提供者 {#generating-providers}

在原始碼目錄產生 Provider，並將它加入應用程式的編譯期 Provider 清單。

```sh
./artisan make:provider Reports/ReportServiceProvider
```

命令建立 `app/providers/reports/report_service_provider.rs`、宣告模組，並將 Provider 加入 `bootstrap/providers.rs` 的清單末端。標準 Provider 清單寫成單行或多行皆可辨識。重新建置後即可使用。可在產生的掛鉤內加入服務綁定和初始化。

使用 `-f` 或 `--force` 取代 Provider 原始碼。若無法定位自訂 bootstrap 的清單，Artisan 會提示手動註冊位置。可透過 `stub:publish` 發佈並修改 `stubs/provider.rs.stub`。

## 撰寫 Service Provider

實作 `openrust::support::ServiceProvider`，在 `bootstrap/providers.rs` 列出 Provider。覆寫 `register` 綁定服務，覆寫 `boot` 在所有 Provider 註冊後初始化服務。兩者預設均成功且不執行操作，只需實作所需鉤子。`register` 和 `boot` 回傳非同步 Future，註冊相依服務和初始化均可等待 I/O。

Provider 按具體 Rust 類型去重，保留首次註冊。成功引導具備冪等性。註冊或引導失敗後不再執行後續階段，同一個應用不能重試已經部分完成的引導。錯誤標明 Provider 和階段，並保留原始錯誤供報告使用；重試初始化需建立新應用。

Provider 鉤子在所屬應用的門面作用域內執行。HTTP 與 Artisan 使用同樣的 Provider 生命週期，不同應用的執行作用域和容器彼此隔離。參閱[容器綁定](container.md#綁定)和[門面](facades.md#門面的運作原理)。

## 延遲服務提供者

實作 `openrust::contracts::support::DeferrableProvider`，透過 `provides()` 宣告精確服務鍵。在原有 Provider 清單中登記 `DeferredProvider::new(factory)`。編譯型工廠取代 PHP 按類名建構；靜態清單不會實例化 Provider。去重使用實際 Provider 類型。HTTP 不載入未使用的 Provider；Console Kernel 在註冊命令前呼叫 `load_deferred_providers()`。

使用 `Application::make::<T>().await` 或 `get::<T>(key).await` 等待延遲 Provider。容器對應 `make_async` / `get_async`，門面對應 `get_facade_root_async`。同步解析尚未初始化的延遲服務會回傳 `DeferredServiceRequiresAsync`，不會阻塞 Tokio。同步綁定工廠需接收已初始化的相依服務，不能在內部等待另一個 Provider。`register` 和 `boot` 都回傳 Future，註冊鉤子可以透過 `app.make().await` 載入另一個 Provider。

使用[非同步工廠](container.md#非同步工廠)可在建構內自動解析延遲相依服務。

```rust
use std::{error::Error, sync::Arc};
use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, events::Dispatcher as _, foundation::Application as _, support::DeferrableProvider},
    events::{Dispatcher, EventContext, NamedEvent},
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

    fn when() -> Vec<&'static str> {
        vec!["reports.opened"]
    }
}
impl ServiceProvider for ClientProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singleton_async::<Client, _>(|_| Box::pin(async {
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
        .with_providers(vec![Box::new(DeferredProvider::new(|| ClientProvider))])
        .create()?;
    app.boot().await?;
    assert!(app.is_deferred_service(ServiceKey::typed::<Client>()));
    let events = app.make::<Dispatcher>().await?;
    events.dispatch(Arc::new(NamedEvent::new("reports.opened", ())), &mut EventContext::new()).await?;
    let client = app.make::<Client>().await?;
    assert!(Arc::ptr_eq(&client, &app.make::<Client>().await?));
    Ok(())
}
```

已有明確實例會略過尚未使用的載入器；解析先正規化別名。並行呼叫共用一次註冊，並等待 boot 完成；boot 尚未就緒時，其他呼叫者不能取得其宣告的服務。boot 本身可以解析已經登記的本 Provider 服務。boot 失敗或取消後保留明確失敗狀態，需要建立新應用重試。這是針對 Rust 並行執行的就緒保護，超出 PHP 順序執行場景；已貢獻的容器值保留。

只消耗觸發鍵，同一 Provider 的其他鍵保留到 `load_deferred_providers()` 成功結束。註冊失敗保留貢獻並消耗該鍵；後續尚未消耗的鍵透過編譯型工廠建構新實例重試。如果直接登記已建構物件並覆寫 `is_deferred()`、`provides()`，重試使用原物件；需要重新執行建構時使用 `DeferredProvider`。

boot 前由服務鍵首次觸發的 Provider 同時進入 booting 回呼與已註冊 Provider 清單，因此執行兩次 boot。boot 前由事件觸發時，Provider 依註冊順序加入一般清單並執行一次 boot。在 boot 掛鉤中首次觸發的 Provider 加入啟動清單末尾。應用程式完成 boot 後，觸發入口等待註冊與 boot 都完成才回傳。

### 依事件載入 Provider {#loading-providers-on-events}

透過 `DeferrableProvider::when()` 宣告事件名稱。應用程式在註冊應用程式 Provider 之前安裝這些監聽器。事件觸發註冊時保留延遲服務鍵，事件與服務鍵並行觸發時共用同一次註冊。分派器將 Provider 的 Rust 型別名稱當作監聽結果回傳，讓 `until` 可以在此結束。註冊失敗後可以透過後續事件重試。boot 失敗沿用延遲 Provider 的失敗狀態。

應用程式建立時，從編譯進執行檔的中繼資料建立 Provider 目錄。

需要讓具體型別無需服務工廠即可自動注入時，在 `register` 登記編譯建構中繼資料；它不會把型別標為已綁定，也不會實例化。參閱[自動建構式注入](container.md#自動建構式注入)。

## 檢查已載入的 Provider

使用 `app.provider_is_loaded(std::any::type_name::<MyProvider>())` 查詢 Provider 的編譯型別身分。register 成功後、boot 之前回傳 true；註冊失敗或取消不作標記。後續 boot 失敗仍保留成功註冊狀態。僅宣告延遲 Provider 不會載入它，自訂診斷用的 name() 不改變型別身分。每個應用程式獨立保存已載入集合。Rust 的 type_name 對應 PHP 的 MyProvider::class 表達式。
