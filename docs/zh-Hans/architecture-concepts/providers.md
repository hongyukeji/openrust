# 服务提供者

## 注册服务提供者

`bootstrap/providers.rs` 列出 HTTP 与 Artisan 共用的应用服务提供者，并登记 `app/providers/app_service_provider.rs` 中的默认 `AppServiceProvider`。应用可在这个 Provider 的 `register` 和 `boot` 中接入服务。

选定的 Kernel 加载配置，执行所有普通 Provider 的 `register` 后，再依次执行 `boot`。详见[请求生命周期](lifecycle.md)。

## 编写 Service Provider

实现 `openrust::support::ServiceProvider`，在 `bootstrap/providers.rs` 列出 Provider。覆盖 `register` 绑定服务，覆盖 `boot` 在所有 Provider 注册后初始化服务。两者默认均成功且不执行操作，只需实现所需钩子。`register` 和 `boot` 返回异步 Future，注册依赖和初始化均可等待 I/O。

Provider 按具体 Rust 类型去重，保留首次注册。成功引导具备幂等性。注册或引导失败后不再执行后续阶段，同一个应用不能重试已经部分完成的引导。错误标明 Provider 和阶段，并保留原始错误供报告使用；重试初始化需创建新应用。

Provider 钩子在所属应用的门面作用域内运行。HTTP 与 Artisan 使用同样的 Provider 生命周期，不同应用的执行作用域和容器彼此隔离。参阅[容器绑定](container.md#绑定)和[门面](facades.md#门面的工作原理)。

## 延迟服务提供者

实现 `openrust::contracts::support::DeferrableProvider`，通过 `provides()` 声明精确服务键。在原有 Provider 列表中登记 `DeferredProvider::new(factory)`。编译型工厂代替 PHP 按类名构造；静态清单不会实例化 Provider。去重使用实际 Provider 类型。HTTP 不加载未使用的 Provider；Console Kernel 在注册命令前调用 `loadDeferredProviders()`。

使用 `Application::make::<T>().await` 或 `get::<T>(key).await` 等待延迟 Provider。容器对应 `makeAsync` / `getAsync`，门面对应 `getFacadeRootAsync`。同步解析尚未初始化的延迟服务会返回 `DeferredServiceRequiresAsync`，不会阻塞 Tokio。同步绑定工厂需接收已初始化的依赖，不能在内部等待另一个 Provider。`register` 和 `boot` 都返回 Future，注册钩子可以通过 `app.make().await` 加载另一个 Provider。

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
            app.container().singleton::<Client, _>(|_| Ok(Arc::new(Client)))?;
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

已有显式实例会跳过尚未使用的装载器；解析先归一化别名。并发调用共享一次注册，并等待 boot 完成；boot 未就绪时，其他调用方不能取得其声明的服务。boot 自身可以解析已经登记的本 Provider 服务。boot 失败或取消后保留明确失败状态，需要新建应用重试。这是针对 Rust 并发运行的就绪保护，超出 PHP 顺序执行场景；已贡献的容器值保留。

只消费触发键，同一 Provider 的其他键保留到 `loadDeferredProviders()` 成功结束。注册失败保留贡献并消费该键；后续尚未消费的键通过编译型工厂构造新实例重试。如果直接登记已构造对象并覆盖 `isDeferred()`、`provides()`，重试使用原对象；需要重新执行构造时使用 `DeferredProvider`。

在 boot 阶段开始前首次加载的 Provider，同时进入 booting 回调和已注册 Provider 列表，因此 boot 会执行两次，保留固定 Laravel 基线的行为。在普通 Provider 的 boot 中首次使用时，进入剩余启动列表一次；应用 boot 完成后首次使用，会等待注册和 boot 均完成再返回服务。事件触发加载及其他生命周期回调依后续单元推进。
