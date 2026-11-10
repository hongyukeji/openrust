# 服务提供者

## 注册服务提供者

`bootstrap/providers.rs` 列出 HTTP 与 Artisan 共用的应用服务提供者，并登记 `app/providers/app_service_provider.rs` 中的默认 `AppServiceProvider`。应用可在这个 Provider 的 `register` 和 `boot` 中接入服务。

选定的 Kernel 加载配置，执行所有 Provider 的 `register` 后，再依次执行 `boot`。详见[请求生命周期](lifecycle.md)。
