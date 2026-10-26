# `openrust::core`

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：应用内核当前事实。

## 边界

只提供装配、配置、错误、命令与生命周期；不含路由、数据库或任何 driver。依赖：`axum-core`（`IntoResponse`）、`http`、`figment`、`serde`、`tokio`、`tracing`、`toml`。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `app` | `Application { type Config; fn config_namespace() -> &'static str; fn build(&mut AppBuilder) }`，默认命名空间 `APP_`；`AppBuilder`（`plugin` / `insert` / `get` / `command` / `section` / `extension_mut`，注册方法为 `&mut self` 链式，`get` 为只读）；`App`（`state` / `config` / `figment` / `commands` / `extension` / `run_command`）；`AppState`（`get` / `try_get`，按 `TypeId` 的只读注册表）；`MissingServiceError` |
| `plugin` | `Plugin { name, build(&mut AppBuilder), finish(&App) -> BoxFuture }`；`BoxFuture` |
| `command` | `Command { name, about, accepts_args, run }`；额外参数默认拒绝；内置 `help`、`config show`（密钥遮蔽） |
| `config` | `src/config/` 已有 app/api/auth/database/logger/mailer/server/signing/sms，queue 与 security 随对应 feature 开启；单一分层 `ConfigLoader` 合并默认值 → 根 `config/default.toml` / `<env>.toml` → 非生产 `.env` → 进程环境变量。默认 `APP_`、工作目录下 `config/`；`FrameworkConfig { app, server, api, log, i18n }`，其他能力通过独立 section 读取；数据库、认证、邮件旧模块路径由 re-export 保持；`NoConfig`、`load_dotenv`、`redact_secrets` |
| `error` | `Error` 枚举（`BadRequest` / `Unauthorized` / `Forbidden` / `NotFound` / `Conflict` / `Validation` / `ValidationDetail` / `RateLimited` / `RateLimitedWithRetryAfter` / `Config` / `Custom` / `Internal`），`status()` / `code()` / `to_api_response()`；`validation_with_detail` 用于 JSON 结构错误，`custom_with_status` 承载 405/408/413/415 等稳定码，`IntoResponse` 输出 `application/json`；`ApiResponse<T>`、请求作用域 `ResponseContext`、`ValidationErrors` |
| `rate_limit` | `RateLimiter` 与 `RateLimitStore`；按稳定 key 原子消费固定窗口，服务统一策略校验和 429/Retry-After 语义，store 故障传播；core 不决定 HTTP 身份来源或中间件 |
| `lifecycle` | `bootstrap` / `bootstrap_with(defaults)` / `finalize(builder)` / `init_tracing` / `shutdown_signal` |

分页 `meta` 使用 `Option<PageMeta>`，`with_meta` 直接保留强类型值。

## 约束

- 注册期间新增的 plugin 会被继续处理（循环直到队列为空）。
- `Application::Config` 不能是 `()`，用 `NoConfig`。
- 5xx 错误只输出通用 `message`。

## 后续边界

- `/ready` 依赖检查仍待实现，按 M5 可观测性契约接入。
- 若 plugin 需要声明硬依赖，在 `build` / `finish` 的合适阶段检查并返回配置错误；现有接口没有 `register` 生命周期方法。
- 公共业务流程钩子与事务前后扩展仍待落地；现有 `Plugin::build/finish` 只覆盖应用装配。worker/scheduler 当前由命令持有 supervisor 管理生命周期，不据此宣称已有通用 plugin shutdown。

## 验证

`cargo test -p openrust`：配置分层、APP_ 默认和自定义命名空间、`.env` 引导、前缀优先级、错误键名、密钥遮蔽、错误渲染、注册表与 plugin 级联构建/finish。

限流单测验证 quota 边界、固定窗口重试语义、非法策略和存储故障关闭。
