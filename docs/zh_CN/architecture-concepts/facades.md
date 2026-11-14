# 门面

## 门面的工作原理

导入 `openrust::support::facades::Facade`，定义门面的服务类型和 `getFacadeAccessor`。`getFacadeRoot` 通过当前应用容器解析访问器。使用显式转发方法替代 PHP 的 `__callStatic`，由 Rust 在编译时检查方法。

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

在 Provider 中绑定 `str` 并设置 `message` 别名，然后在 Provider 钩子、HTTP 处理器或 Artisan 命令内调用 `Messages::text()`。在框架应用作用域之外调用会返回 `FacadeNotSet`。

门面解析结果在当前应用执行期间缓存。`clearResolvedInstance(None)` 清除当前门面访问器的缓存，`clearResolvedInstance(Some(key))` 清除指定访问器，`clearResolvedInstances()` 清除当前全部缓存。`swap(instance)` 同时替换缓存根对象和容器绑定。门面可以设置 `const cached: bool = false`，使新的解析结果不进入缓存。

框架为应用引导、每个 HTTP 请求和每次 Artisan 调用设置门面作用域。同一应用中的嵌套执行共享作用域，不同应用彼此隔离。新请求或命令使用新的门面缓存；注册为单例的服务仍属于该应用的容器。应用代码自行创建的 Tokio 任务不会自动继承门面作用域，应向这些任务传入已解析服务。

`Route::get` 与 `Artisan::command` 在路由模块的 `register()` 执行期间使用专用注册作用域，属于已交付的注册门面。其他门面类型声明存在，不代表对应服务已经实现。Mocking 辅助接口和解析回调由后续单元交付。

未加载的延迟 Provider 或异步工厂使用 `getFacadeRootAsync().await`。它直接走一次异步容器解析，等待就绪并共用当前应用的门面缓存；期间出现较新的 `swap` 或其他已缓存根对象时保留该值。同步解析未就绪的 Provider 或尚未构造的异步工厂返回类型化的异步解析错误。

挂起的门面 Future 属于原应用执行上下文，移入另一请求、命令或应用继续 poll 会返回 `FacadeNotSet`。生命周期结束会清空缓存；保留的容器解析不能重新打开已关闭作用域（`ScopeClosed`）。销毁工作时恢复仍存活的原上下文，原上下文关闭后屏蔽环境门面。应用明确保留的服务和 Future 仍由应用持有。参阅[异步工厂](container.md#异步工厂)和[延迟服务提供者](providers.md#延迟服务提供者)。

`makeWith` 进行一次显式构造，不替换单独缓存的门面根对象；需要该结果时直接通过应用或容器解析。参阅[参数覆盖](container.md#参数覆盖)。
