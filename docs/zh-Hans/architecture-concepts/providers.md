# 服务提供者

## 注册服务提供者

`bootstrap/providers.rs` 列出 HTTP 与 Artisan 共用的应用服务提供者，并登记 `app/providers/app_service_provider.rs` 中的默认 `AppServiceProvider`。应用可在这个 Provider 的 `register` 和 `boot` 中接入服务。

选定的 Kernel 加载配置，执行所有普通 Provider 的 `register` 后，再依次执行 `boot`。详见[请求生命周期](lifecycle.md)。

## 生成服务提供者 {#generating-providers}

在源码目录中生成 Provider，并将它加入应用的编译期 Provider 列表。

```sh
./artisan make:provider Reports/ReportServiceProvider
```

命令创建 `app/providers/reports/report_service_provider.rs`，声明模块并把 Provider 追加到 `bootstrap/providers.rs`。标准 Provider 列表写成单行或多行均可识别。重新构建后即可使用。可在生成的钩子内添加服务绑定和初始化。

使用 `-f` 或 `--force` 替换 Provider 源码。如果自定义 bootstrap 的列表无法定位，Artisan 会提示手动注册位置。可通过 `stub:publish` 发布并修改 `stubs/provider.rs.stub`。

## 编写 Service Provider

实现 `openrust::support::ServiceProvider`，在 `bootstrap/providers.rs` 列出 Provider。覆盖 `register` 绑定服务，覆盖 `boot` 在所有 Provider 注册后初始化服务。两者默认均成功且不执行操作，只需实现所需钩子。`register` 和 `boot` 返回异步 Future，注册依赖和初始化均可等待 I/O。

Provider 按具体 Rust 类型去重，保留首次注册。成功引导具备幂等性。注册或引导失败后不再执行后续阶段，同一个应用不能重试已经部分完成的引导。错误标明 Provider 和阶段，并保留原始错误供报告使用；重试初始化需创建新应用。

Provider 钩子在所属应用的门面作用域内运行。HTTP 与 Artisan 使用同样的 Provider 生命周期，不同应用的执行作用域和容器彼此隔离。参阅[容器绑定](container.md#绑定)和[门面](facades.md#门面的工作原理)。

## 延迟服务提供者

实现 `openrust::contracts::support::DeferrableProvider`，通过 `provides()` 声明精确服务键。在原有 Provider 列表中登记 `DeferredProvider::new(factory)`。编译型工厂代替 PHP 按类名构造；静态清单不会实例化 Provider。去重使用实际 Provider 类型。HTTP 不加载未使用的 Provider；Console Kernel 在注册命令前调用 `load_deferred_providers()`。

使用 `Application::make::<T>().await` 或 `get::<T>(key).await` 等待延迟 Provider。容器对应 `make_async` / `get_async`，门面对应 `get_facade_root_async`。同步解析尚未初始化的延迟服务会返回 `DeferredServiceRequiresAsync`，不会阻塞 Tokio。同步绑定工厂需接收已初始化的依赖，不能在内部等待另一个 Provider。`register` 和 `boot` 都返回 Future，注册钩子可以通过 `app.make().await` 加载另一个 Provider。

使用[异步工厂](container.md#异步工厂)可在构造内部自动解析延迟依赖。

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

已有显式实例会跳过尚未使用的装载器；解析先归一化别名。并发调用共享一次注册，并等待 boot 完成；boot 未就绪时，其他调用方不能取得其声明的服务。boot 自身可以解析已经登记的本 Provider 服务。boot 失败或取消后保留明确失败状态，需要新建应用重试。这是针对 Rust 并发运行的就绪保护，超出 PHP 顺序执行场景；已贡献的容器值保留。

只消费触发键，同一 Provider 的其他键保留到 `load_deferred_providers()` 成功结束。注册失败保留贡献并消费该键；后续尚未消费的键通过编译型工厂构造新实例重试。如果直接登记已构造对象并覆盖 `is_deferred()`、`provides()`，重试使用原对象；需要重新执行构造时使用 `DeferredProvider`。

boot 前由服务键首次触发的 Provider 同时进入 booting 回调和已注册 Provider 列表，因此执行两次 boot。boot 前由事件触发时，Provider 按注册顺序加入普通列表并执行一次 boot。在 boot 钩子中首次触发的 Provider 加入启动列表末尾。应用完成 boot 后，触发入口等待注册和 boot 均完成才返回。

### 按事件加载 Provider {#loading-providers-on-events}

通过 `DeferrableProvider::when()` 声明事件名称。应用在注册应用 Provider 之前安装这些监听器。事件触发注册时保留延迟服务键，事件和服务键并发触发时共用同一注册。分发器将 Provider 的 Rust 类型名称作为监听结果返回，便于 `until` 在此结束。注册失败后可以通过后续事件重试。boot 失败沿用延迟 Provider 的失败状态。

应用创建时从编译进二进制的元数据建立 Provider 目录。

需要让具体类型无需服务工厂即可自动注入时，在 `register` 登记编译构造元数据；它不会把类型标为已绑定，也不会实例化。参阅[自动构造器注入](container.md#自动构造器注入)。

## 检查已加载的 Provider

使用 `app.provider_is_loaded(std::any::type_name::<MyProvider>())` 查询 Provider 的编译类型身份。register 成功后、boot 之前返回 true；注册失败或取消不作标记。后续 boot 失败仍保留成功注册状态。仅声明延迟 Provider 不会加载它，自定义诊断用的 name() 不改变类型身份。每个应用独立保存已加载集合。Rust 的 type_name 对应 PHP 的 MyProvider::class 表达式。
