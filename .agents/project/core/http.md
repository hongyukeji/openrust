# openrust-http

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：HTTP 层当前事实。

## 边界

基于 `axum` 0.8 + `tower-http`。提供路由约定、提取器、默认中间件、`/health` 与 `serve`；认证、OpenAPI、限流在后续里程碑加入。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `plugin` | `HttpPlugin`（注册 `serve` 与 `/health`）；`AppBuilderHttpExt::routes(Fn(Routes) -> Routes)`；`RouteRegistry` 扩展；`build_router(&App) -> Result<Router>` |
| `routes` | `Routes`：`route` / `get` / `post` / `put` / `patch` / `delete` / `resource` / `resource_with` / `nest(prefix, f)` / `merge` / `layer` / `into_router`；`nest("")` 或 `"/"` 走 merge |
| `resource` | `Resource` 显式登记动作；`only` 连续调用取交集、`except` 裁剪；路径去尾斜杠后注册集合与成员，`update` 同时映射 PUT / PATCH |
| `response` | `Created<T>` → semantic 201 / 默认 200，`with_location` 返回 `Result<Located<T>>`（非法值 → 500 包装）；`NoContent` → 200 包装，`data: null`；直接提取 `PageParams` 默认 1/20，并校验页数、每页 1..=100 与 offset 不超过 `i64::MAX`；`Paginated<T>` 输出 `data` 与 `meta` |
| `extract` | `Inject<T>`（从 `AppState` 取服务，缺失 → 500 包装）；OpenRust `Json<T>` / `Path<T>` / `Query<T>`（解析失败 → 400/415/422/413 包装），`Validated<Json<T>>` / `Validated<Query<T>>`（先解析，规则失败 → 422，嵌套路径 `a.b[0].c`） |
| `middleware` | `apply(router, &ServerConfig) -> Result<Router>`：请求 ID → 传播 → `TraceLayer` span → 可选 CORS → 超时 408 包装 → 可选 gzip/br 压缩 → `CatchPanic` → 500 包装；404 fallback、405 fallback（保留 `Allow`）、413 包装；启用 CORS 时在组层前逐项校验配置 |
| `health` | `GET /health` → success `ApiResponse<Health>`，`data.status = "ok"` |
| `serve` | `serve(&App)` 绑定 `[server] host:port`，优雅停机；`ServeCommand` |

`pub use axum` 供门面 re-export。

CORS 具体 origin 仅接受 `http://` 或 `https://` 加主机及可选端口，不接受路径、查询或尾部 `/`；错误包含配置键和转义后的原值。方法、请求头与暴露头的 `"*"` 不能与具体值混用，启用 credentials 时三者均不得使用 `"*"`。CORS 位于超时层外，408 响应保留允许来源头。

CORS 预检（`OPTIONS` 且带 `Access-Control-Request-Method`）保持 tower-http 的空正文，普通跨域错误仍为包装响应并携带允许来源头。分页 `meta` 使用 `PageMeta`。

## 后续边界

- `#[openrust::resource]` 控制器模块宏（M1）。
- 限流（M3）；OpenAPI `/docs`（后续里程碑）。
- `/ready` 聚合各能力 crate 的检查（M5）。

## 验证

`cargo test -p openrust-http`：`tests/router.rs` 用 `oneshot` 覆盖 health 统一包装及请求 ID、`Inject` 命中与缺失、`Validated` 的 422 / 400 / 200、panic → 500、压缩协商、CORS 响应与配置错误；`tests/resource.rs` 覆盖路由动作、裁剪、嵌套、响应状态与分页错误/结构。
