# Foundation

`ApplicationDefinition` 是宿主编译期声明，`Application` 是运行对象。二者复用唯一 Container 和 Provider 生命周期。

Provider 的公开扩展契约属于 [`ServiceProvider`](crate::support::ServiceProvider)，具体注册、启动和关闭装配属于 Foundation。命令回调属于 [`Command`](crate::console::Command)，HTTP 路由与 URL 实体属于 Routing。

框架错误与执行上下文仍在 Core。目录和实体归属不表示完整官方 Application、Container 或 Routing 契约已完成；默认行为、公开扩展与尚缺方法按唯一对应计划继续验收。

## 测试应用

`openrust::testing::TestApp<A>` 使用与生产相同的 `ApplicationDefinition::providers`、`ProviderBuilder` 和 Router。测试显式传入 `Figment`，不修改全局环境或工作目录。`remove_provider(ProviderId)` 在注册前调整清单；`configure_after_register` 在所有 Provider 注册后、boot 前通过同一 `Container` 异步覆盖绑定，因此后续 `boot` 能读取替代服务。测试可通过 `request` 发起进程内请求，默认总期限五秒、响应体上限 1 MiB；`request_with_limits` 可调整这两个预算，`raw_response` 保留流式正文并由调用方消费或丢弃。

`TestResponse` 提供状态、响应头、字节和 JSON 读取以及链式断言；断言失败不会回显正文或头值。状态断言遵循应用配置的 `[api] http_status`，默认为 Laravel 式语义状态；仅显式配置 `always_ok` 时才将框架 JSON 错误映射为 HTTP 200。`shutdown(deadline).await` 等待本应用拥有的资源回收；`reset(deadline).await` 先关闭整个旧应用，再用相同蓝图重建，不把根 scoped 缓存清空冒充完整重置。这里的 deadline 只约束旧应用关闭，新应用的启动与路由构建不受同一期限约束。丢弃 `TestApp` 会立即封闭新解析；存活的 `Tokio` runtime 才能继续异步兜底回收。独立 `TestApp` 持有各自容器，捕获的外部共享 `Arc` fixture 由调用方负责重置。实际业务替换与 Provider 移除示例见 [`modules/example`](../../../../../modules/example/README.md)，CLI 生成项目也包含普通 HTTP 与认证测试。进程内请求不替代真实 TCP 验收。

需要从空数据库测试迁移命令时，使用 `TestApp::<A>::builder(sources).run_command("migrate", args).await`：该入口以相同命令名引导并执行一次命令，然后限时回收应用；成功后再用普通 `build()` 验证迁移后的启动与请求。命令失败仍回收资源，命令和回收同时失败时保留命令错误并记录回收错误码；取消执行 future 时沿夹具 Drop 做异步兜底回收。它不返回带命令上下文的应用。
