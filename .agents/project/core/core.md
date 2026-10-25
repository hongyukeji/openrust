# openrust-core

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：应用内核当前事实。

## 边界

只提供装配、配置、错误、命令与生命周期；不含路由、数据库或任何 driver。依赖：`axum-core`（`IntoResponse`）、`http`、`figment`、`serde`、`tokio`、`tracing`、`toml`。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `app` | `Application { type Config; fn build(&mut AppBuilder) }`；`AppBuilder`（`provider` / `provide` / `service` / `command` / `section` / `extension_mut`，全部 `&mut self` 链式）；`App`（`state` / `config` / `figment` / `commands` / `extension` / `run_command`）；`AppState`（`get` / `try_get`，按 `TypeId` 的只读注册表）；`ServiceNotRegistered` |
| `provider` | `ServiceProvider { name, register(&mut AppBuilder), boot(&App) -> BoxFuture }`；`BoxFuture` |
| `command` | `Command { name, about, run }`；内置 `help`、`config:show`（密钥遮蔽） |
| `config` | 分层 `ConfigLoader`（默认值 → `config/app.toml` → `config/<env>.toml` → `APP__*`）；`Environment`（`APP_ENV`）；`FrameworkConfig { app, server, log }`，其中 `ServerConfig` 包含 `compression` 与 `cors` 默认配置；`NoConfig`；`load_dotenv`；`redact_secrets` |
| `error` | `Error` 枚举（`BadRequest` / `Unauthorized` / `Forbidden` / `NotFound` / `Conflict` / `Validation` / `RateLimited` / `Config` / `Custom` / `Internal`），`status()` / `code()` / `to_problem()`，`IntoResponse` 输出 `application/problem+json`；`ValidationErrors`；`ProblemDetails` |
| `lifecycle` | `bootstrap` / `bootstrap_with(defaults)` / `finalize(builder)` / `init_tracing` / `shutdown_signal` |

## 约束

- 注册期间新增的 provider 会被继续处理（循环直到队列为空）。
- `Application::Config` 不能是 `()`，用 `NoConfig`。
- 5xx 错误不输出 `detail`。

## 后续边界

- `/ready` 依赖检查由能力 crate 注册，不在 core。
- 若 provider 需要声明硬依赖，在 `register` 中用 `app.service::<T>()` 检查并返回 `Error::Config`。

## 验证

`cargo test -p openrust-core`：配置分层、键名报错、密钥遮蔽、错误渲染、注册表、provider 级联注册与 boot。
