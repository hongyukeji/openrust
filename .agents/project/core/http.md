# openrust-http

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：HTTP 层当前事实。

## 边界

基于 `axum` 0.8 + `tower-http`。只提供路由约定、提取器、默认中间件、`/health` 与 `serve`；认证、OpenAPI、限流、CORS、压缩在后续里程碑加入。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `provider` | `HttpProvider`（注册 `serve` 与 `/health`）；`AppBuilderHttpExt::routes(Fn(Routes) -> Routes)`；`RouteRegistry` 扩展；`build_router(&App)` |
| `routes` | `Routes`：`route` / `get` / `post` / `put` / `patch` / `delete` / `group(prefix, f)` / `merge` / `layer` / `into_router`；`group("")` 或 `"/"` 走 merge |
| `extract` | `Inject<T>`（从 `AppState` 取服务，缺失 → 500 Problem）；`Validated<Json<T>>` / `Validated<Query<T>>`（反序列化失败 → 400，规则失败 → 422，嵌套路径 `a.b[0].c`） |
| `middleware` | `apply(router, &ServerConfig)`：请求 ID → 传播 → `TraceLayer` span → 超时 408 → `CatchPanic` → 500 Problem |
| `health` | `GET /health` → `{"status":"ok"}` |
| `serve` | `serve(&App)` 绑定 `[server] host:port`，优雅停机；`ServeCommand` |

`pub use axum` 供门面 re-export。

## 后续边界

- `Routes::resource` 与 `Paginated<T>` / `Created<T>` 响应类型（M1）。
- CORS、压缩、限流、OpenAPI `/docs`（M1/M3）。
- `/ready` 聚合各能力 crate 的检查（M5）。

## 验证

`cargo test -p openrust-http`：`tests/router.rs` 用 `oneshot` 覆盖 health 与请求 ID、`Inject` 命中与缺失、`Validated` 的 422 / 400 / 200、panic → 500。
