# `openrust::http`

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：HTTP 层当前事实。

## 边界

基于 `axum` 0.8 + `tower-http`。提供路由约定、提取器、默认中间件、`/health` 与 `serve`；core 已提供 `RateLimiter`，DatabasePlugin 提供双库共享 store，AuthFlows 与显式 `RateLimitLayer` 已复用它。OpenAPI schema 与 `/docs` 尚未实现。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `plugin` | `HttpPlugin`（注册 `serve`、`/health` 与 `/ready`）；`AppBuilderHttpExt::routes(Fn(Routes) -> Routes)`；`RouteRegistry` 扩展；`build_router(&App) -> Result<Router>` 在任何 route layer 前向请求扩展注入最终 `AppState`，供跨能力中间件读取；经应用路由组合发现的 Axum 冲突转换为 `http_route_registration_failed` 启动错误，直接调用底层 Axum API 不在转换范围 |
| `routes` | `Routes`：`route` / `get` / `post` / `put` / `patch` / `delete` / `resource` / `resource_with` / `nest(prefix, f)` / `merge` / `layer` / `into_router`；`nest("")` 或 `"/"` 走 merge |
| `resource` | `Resource` 显式登记动作；`#[openrust::resource]` 从 controller inherent impl 的动作关联函数生成实现；`only` 连续调用取交集、`except` 裁剪；路径去尾斜杠后注册集合与成员，`update` 同时映射 PUT / PATCH |
| `response` | `Created<T>` → semantic 201 / 默认 200，`with_location` 返回 `Result<Located<T>>`（非法值 → 500 包装）；`NoContent` → 200 成功包装并省略 `data`；无载荷的错误包装也省略 `data`，已有载荷保持原样；直接提取 `PageParams` 默认 1/20，并校验页数、每页 1..=100 与 offset 不超过 `i64::MAX`；`Paginated<T>` 输出 `data` 与 `meta` |
| `extract` | `Inject<T>`（从 `AppState` 取服务，缺失 → 500 包装）；OpenRust `Json<T>` / `Path<T>` / `Query<T>`（解析失败 → 400/415/422/413 包装），`Validated<Json<T>>` / `Validated<Query<T>>`（先解析，规则失败 → 422，嵌套路径 `a.b[0].c`） |
| `middleware` | `apply(router, &ServerConfig) -> Result<Router>`：请求 ID → 传播 → `TraceLayer` span → 可选 CORS → 超时 408 包装 → 可选 gzip/br 压缩 → `CatchPanic` → 500 包装；404 fallback、405 fallback（保留 `Allow`）、413 包装；启用 CORS 时在组层前逐项校验配置；响应上下文同时建立 request ID/locale/deadline/cancellation 的 `ExecutionContext`，非法 request ID 移除后由 middleware 生成新值 |
| `rate_limit` | `RateLimitLayer` 显式包裹路由并调用 AppState 中注册的 `RateLimiter`；key 支持可信客户端 IP、（启用 auth 时）已验证用户/匿名回退及自定义策略；store 错误关闭，429 沿用统一错误与 `Retry-After`。未包裹路由、health/fallback 不自动限流 |
| `sort` | `SortParams::parse` 按 ADR-008 `sort=-created_at,title` 语法解析，限制长度/字段数，拒绝重复、未知或格式错误字段；调用方必须提供字段 allowlist，再显式映射到 SeaORM 列。过滤器应使用 `deny_unknown_fields` 的输入 DTO 并构造显式 `Condition`，输出用响应 DTO 做字段 allowlist |
| `health` | `GET /health` → success `ApiResponse<Health>`，`data.status = "ok"`；`GET /ready` 在 listener 可接流量且依赖探针成功时为 200，否则 503 |
| `serve` | `serve(&App)` 绑定 `[server] host:port`，收到 SIGINT/SIGTERM 后停止接入并按 `shutdown_timeout_secs` drain；到期取消请求并强制 join server task，返回 `http_shutdown_timeout`；`ServeCommand` |

`pub use axum` 供门面 re-export。

CORS 具体 origin 仅接受 `http://` 或 `https://` 加主机及可选端口，不接受路径、查询或尾部 `/`；错误包含配置键和转义后的原值。方法、请求头与暴露头的 `"*"` 不能与具体值混用，启用 credentials 时三者均不得使用 `"*"`。CORS 位于超时层外，408 响应保留允许来源头。

CORS 预检（`OPTIONS` 且带 `Access-Control-Request-Method`）保持 tower-http 的空正文，普通跨域错误仍为包装响应并携带允许来源头。分页 `meta` 使用 `PageMeta`。

## 后续边界

- 路由限流仅覆盖显式包裹的路由；默认不覆盖 health、fallback 或其他能力层。认证限流需确保 `AuthLayer` 在请求方向先执行。
- `SortParams` 仅负责安全解析，不提供 ORM 字段反射、过滤 DSL 或自动授权；应用仍须按资源授权并将排序/过滤白名单映射到查询。DTO/查询与关联加载示例见 [HTTP README](../../../src/http/README.md)。
- OpenAPI schema 与 `/docs`（后续里程碑）；`utoipa` / `utoipa-axum` 尚未引入，统一响应需与生成文档一致。
- `Readiness::is_ready()` 仍只表示 listener 生命周期；`/ready` 聚合 DatabasePlugin 的全部数据库池、CachePlugin 的全部 store 和应用显式注册的依赖探针。探针最多注册 64 个、每轮最多并发 16 个、总时限 2 秒，失败只向客户端返回通用 503，并只在日志记录探针名与稳定错误码。此端点不覆盖 gRPC health、WebSocket/SSE、跨节点实时投递或任意应用原生依赖；metrics/OTel 与完整生产就绪验收仍待完成。
- request ID 只作为队列 request metadata 与 worker span 的日志关联，不表示 W3C trace context/OpenTelemetry span link；gRPC G2 仍须按 ADR-040 复验 deadline、cancel、locale 与 trace 传播。

## 验证

`cargo test -p openrust --test http_router --test http_resource`：`tests/http/router.rs` 用 `oneshot` 覆盖 health 统一包装及请求 ID、`Inject` 命中与缺失、`Validated` 的 422 / 400 / 200、panic → 500、压缩协商、CORS 响应与配置错误；`tests/http/resource.rs` 覆盖路由动作、裁剪、嵌套、响应状态与分页错误/结构。

## 出站 HTTP 客户端

`http-client` 是可选 feature，公开 `openrust::http::HttpClient`、`HttpClientConfig`、`HttpTargetPolicy`、`HttpRequest`、`HttpResponse`、`HttpTransport`、`HttpTransportRequest`、`HttpResponseBody`、`TransportResponse`、`RetryPolicy`、`DependencyClient`、`DependencyPolicy` 与熔断策略。reqwest 0.13 Rustls client 的 clone 共享连接池。默认 connect timeout 5s、total timeout 30s、response read 上限 1 MiB、自动 redirect 关闭、系统 proxy 环境变量不继承（proxy 必须显式配置，Debug 脱敏）。`HttpClient::restricted` 是独立 opt-in：默认拒绝全部主机，登记后仅允许 HTTP/HTTPS、显式端口、无 URL 用户信息、不接受自定义 `Host` 头、精确 host allowlist、全部 DNS 解析结果和公网地址策略在请求/实际解析处分别校验；主机 + 单个 CIDR + 端口集合可以显式允许可信内网，内网登记主机只接受登记端口，代理与 custom transport 不可用。域名解析结果会由同一个 reqwest resolver 传给 connector，禁止二次查询产生的 TOCTOU；IP 字面量在发送前检查。仅此入口受约束，不覆盖原始 SeaORM、存储或任意第三方网络调用。响应状态不自动转为错误；错误文本不暴露 URL/头/正文。明确超限的 `Content-Length` 在返回响应前拒绝，未知长度的 body 通过逐块限额 reader 消费；取消只停止本地 future 等待，不承诺撤销已送达的远端操作。

调用韧性为显式 opt-in：`HttpClient::dependency(name, policy)` 按名称共享有界并发/等待队列和可选熔断；同名配置不一致返回错误。排队、发送和显式重试共用客户端总时限，取消释放队列名额/并发许可；响应正文消费完成或响应对象释放后归还许可。熔断仅进程内共享，transport/timeout 错误及 5xx 计为失败，4xx 与成功响应清除失败计数；本地策略、配置和正文限额错误不计作依赖故障。默认关闭熔断。重试必须通过 `idempotent()` 启用，最多五次，使用全抖动指数退避；只对 429、502、503、504 读取受 `max_delay`、`retry_after_cap` 限制的 `Retry-After`。一次性流 body 不可重放。gRPC 尚未实现，不声称已复用或验证该策略。`HttpClient::with_transport` 支持不受限的测试/应用 transport 注入，自定义 transport 同样受总超时和 body 限额保护。验证命令见 `memory/testing.md` / `memory/verification.md`。
