# openrust-core

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：应用内核当前事实。

## 边界

只提供装配、配置、错误、命令与生命周期；不含路由、数据库或任何 driver。依赖：`axum-core`（`IntoResponse`）、`http`、`figment`、`serde`、`tokio`、`tracing`、`toml`。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `app` | `Application { type Config; fn build(&mut AppBuilder) }`；`AppBuilder`（`plugin` / `insert` / `get` / `command` / `section` / `extension_mut`，注册方法为 `&mut self` 链式，`get` 为只读）；`App`（`state` / `config` / `figment` / `commands` / `extension` / `run_command`）；`AppState`（`get` / `try_get`，按 `TypeId` 的只读注册表）；`MissingServiceError` |
| `plugin` | `Plugin { name, build(&mut AppBuilder), finish(&App) -> BoxFuture }`；`BoxFuture` |
| `command` | `Command { name, about, accepts_args, run }`；额外参数默认拒绝；内置 `help`、`config show`（密钥遮蔽） |
| `config` | 分层 `ConfigLoader`（默认值 → `config/default.toml` → `config/<env>.toml` → `OPENRUST_*`）；`Environment`（`OPENRUST_ENV`）；生产进程跳过 `.env`，未设置进程环境时允许 `.env` 选择环境；`FrameworkConfig { app, server, log }`，其中 `ServerConfig` 包含 `compression` 与 `cors` 默认配置；`NoConfig`；`load_dotenv`；`redact_secrets` |
| `error` | `Error` 枚举（`BadRequest` / `Unauthorized` / `Forbidden` / `NotFound` / `Conflict` / `Validation` / `RateLimited` / `Config` / `Custom` / `Internal`），`status()` / `code()` / `to_problem()`，`IntoResponse` 输出 `application/problem+json`；`ValidationErrors`；`ProblemDetails` |
| `lifecycle` | `bootstrap` / `bootstrap_with(defaults)` / `finalize(builder)` / `init_tracing` / `shutdown_signal` |

## 约束

- 注册期间新增的 plugin 会被继续处理（循环直到队列为空）。
- `Application::Config` 不能是 `()`，用 `NoConfig`。
- 5xx 错误不输出 `detail`。

## 后续边界

- `/ready` 依赖检查由能力 crate 注册，不在 core。
- 若 plugin 需要声明硬依赖，在 `register` 中用 `app.get::<T>()` 检查并返回 `Error::Config`。

## 验证

`cargo test -p openrust-core`：配置分层、键名报错、密钥遮蔽、错误渲染、注册表、plugin 级联构建与 finish。
