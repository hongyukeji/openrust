# 门面

## 门面的工作原理

导入 `openrust::support::facades::Facade`，定义门面的服务类型和 `get_facade_accessor`。`get_facade_root` 通过当前应用容器解析访问器。使用显式转发方法替代 PHP 的 `__callStatic`，由 Rust 在编译时检查方法。

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

在 Provider 中绑定 `str` 并设置 `message` 别名，然后在 Provider 钩子、HTTP 处理器或 Artisan 命令内调用 `Messages::text()`。在框架应用作用域之外调用会返回 `FacadeNotSet`。

门面解析结果在当前应用执行期间缓存。`clear_resolved_instance(None)` 清除当前门面访问器的缓存，`clear_resolved_instance(Some(key))` 清除指定访问器，`clear_resolved_instances()` 清除当前全部缓存。`swap(instance)` 同时替换缓存根对象和容器绑定。门面可以设置 `const CACHED: bool = false`，使新的解析结果不进入缓存。

框架为应用引导、每个 HTTP 请求和每次 Artisan 调用设置门面作用域。同一应用中的嵌套执行共享作用域，不同应用彼此隔离。新请求或命令使用新的门面缓存；注册为单例的服务仍属于该应用的容器。应用代码自行创建的 Tokio 任务不会自动继承门面作用域，应向这些任务传入已解析服务。

`Route::get` 与 `Artisan::command` 在路由模块的 `register()` 执行期间使用专用注册作用域。服务门面须在应用生命周期内使用，并先注册对应服务。

未加载的延迟 Provider 或异步工厂使用 `get_facade_root_async().await`。它直接走一次异步容器解析，等待就绪并共用当前应用的门面缓存；期间出现较新的 `swap` 或其他已缓存根对象时保留该值。同步解析未就绪的 Provider 或尚未构造的异步工厂返回类型化的异步解析错误。

挂起的门面 Future 属于原应用执行上下文，移入另一请求、命令或应用继续 poll 会返回 `FacadeNotSet`。生命周期结束会清空缓存；保留的容器解析不能重新打开已关闭作用域（`ScopeClosed`）。销毁工作时恢复仍存活的原上下文，原上下文关闭后屏蔽环境门面。应用明确保留的服务和 Future 仍由应用持有。参阅[异步工厂](container.md#异步工厂)和[延迟服务提供者](providers.md#延迟服务提供者)。

`make_with` 进行一次显式构造，不替换单独缓存的门面根对象；需要该结果时直接通过应用或容器解析。参阅[参数覆盖](container.md#参数覆盖)。

## 解析回调与应用作用域 {#facade-lifecycle}

`resolved(callback)` 观察实际后端服务。所属应用已经解析它时立即回调，未来容器重新解析时也回调；注册回调不会提前创建服务。参数为 `Arc<Service>` 和 `&Container`，可直接保留同一个服务句柄，不必再次解析。回调错误向上传递，即时回调失败时不会继续注册。异步工厂完成解析也会触发同一个钩子。

`App::resolved(callback)` 是公共门面回调。查询容器服务是否已解析时用 `App::is_resolved(key)`，底层容器保留 `resolved(key)`。查询方法使用不同名称，避免遮蔽回调入口。

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

普通 HTTP、Artisan 和 Worker 已有应用作用域。嵌入应用或测试中，`set_facade_application(Some(&app), future).await` 只切换给定操作，结束、取消或 panic 展开后恢复外层应用；`None` 临时隐藏应用。`get_facade_application()` 返回 `Result<Option<Arc<Application>>, BindingResolutionError>`，作用域外为 `None`。同应用嵌套保留门面缓存，不同应用使用独立缓存。显式创建的 Tokio 任务也可用此入口打开自己的作用域。

带作用域的 setter 是全局应用 setter 的 Rust 适配：用明确的生命周期避免进程级修改影响并发请求，不在应用之间合并缓存，也不延长已结束作用域。解析回调属于应用容器，应在启动阶段注册持久回调，避免捕获只应存活于当前请求的数据。

`is_fake()` 只检查已缓存的门面根，不触发解析。Mail、Notification 和 Event 返回实际 Fake 状态，Event 临时替身也生效。清空门面缓存后，在重新解析该根之前返回 false。HTTP、Process 的桩仍属于普通工厂，返回 false；记录调用的 spy 本身也不等于 Fake 标记。自定义门面可覆盖类型安全的 `is_fake_root` 钩子，识别自身服务的 Fake 状态。

## 应用门面 {#application-facade}

`App` 访问当前应用及其容器。`bind`、`singleton`、`scoped` 及对应的 `_if`、`_named` 注册方法委托给同一容器，不建立第二份注册表。`make`、`get` 和 `make_with` 使用异步接口，保留延迟 Provider 和异步工厂的解析行为。

```rust
use std::sync::Arc;
use openrust::support::facades::App;

struct Greeting(String);

App::singleton::<Greeting, _>(|_| Ok(Arc::new(Greeting("Hello".into()))))?;
let greeting = App::make::<Greeting>().await?;
assert_eq!(greeting.0, "Hello");
```

用 `base_path`、`public_path`、`storage_path`、`lang_path` 和 `join_paths` 获取应用路径。`environment` 返回当前环境或匹配指定模式；`is_local`、`is_production` 和 `running_unit_tests` 查询相同的环境状态。`get_bindings` 返回独立快照。解析钩子、扩展器、别名及实例清除沿用容器已有的生命周期。

请求感知用法见[响应门面](../the-basics/responses.md#response-facade)和[会话门面](../the-basics/session.md#session-facade)。
