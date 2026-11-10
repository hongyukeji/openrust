# 服务提供者

## 注册服务提供者

`bootstrap/providers.rs` 列出 HTTP 与 Artisan 共用的应用服务提供者，并登记 `app/providers/app_service_provider.rs` 中的默认 `AppServiceProvider`。应用可在这个 Provider 的 `register` 和 `boot` 中接入服务。

选定的 Kernel 加载配置，执行所有 Provider 的 `register` 后，再依次执行 `boot`。详见[请求生命周期](lifecycle.md)。

## 编写 Service Provider

实现 `openrust::support::ServiceProvider`，在 `bootstrap/providers.rs` 列出 Provider。覆盖 `register` 绑定服务，覆盖 `boot` 在所有 Provider 注册后初始化服务。两者默认均成功且不执行操作，只需实现所需钩子。`boot` 返回异步 Future，初始化可以等待 I/O。

Provider 按具体 Rust 类型去重，保留首次注册。成功引导具备幂等性。注册或引导失败后不再执行后续阶段，同一个应用不能重试已经部分完成的引导。错误标明 Provider 和阶段，并保留原始错误供报告使用；重试初始化需创建新应用。

Provider 钩子在所属应用的门面作用域内运行。HTTP 与 Artisan 使用同样的 Provider 生命周期，不同应用的执行作用域和容器彼此隔离。参阅[容器绑定](container.md#绑定)和[门面](facades.md#门面的工作原理)。
