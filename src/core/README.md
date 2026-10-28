# 应用核心

OpenRust 框架的应用内核。

## 容器与 Provider 生命周期

`openrust::core::container` 提供 `ContainerBuilder`、`Container`、`Resolver` 与 `Scope`。`Resolver` 使用弱容器句柄，延迟解析时容器已释放则报告 `container_closed`，避免 singleton 保存解析器造成引用环。类型键支持 `dyn Trait`，`bind` 每次创建，`singleton` 同容器共享，`scoped` 同生命周期共享，`instance` 注册已有 `Arc`。同类型后注册覆盖前定义；定义经 `build` 封闭，不使用 PHP 反射构造。

根容器也有 scoped 生命周期，`forget_scoped_instances()` 清除旧代，下次解析重建；子 scope 独立。关闭/取消在途解析后可重新构造，不保留失败占位；同链与并发等待环返回类型链。singleton 捕获 scoped 被拒绝，这是长驻进程安全增强。错误 `Debug` 只输出 code/status，工厂原始错误保留在内部 source，不直接输出 HTTP。

缓存工厂在成功、失败或取消时先结束构造帧并清除已结束的等待边，再发布结果；已完成服务不再创建等待关系。scope关闭和根reset的唤醒、用户值释放都在容器内部锁之外执行，允许Waker同步重入。

生产 `bootstrap` 使用同一 `Container`：`Application::providers` 给出清单，`ProviderBuilder` 先执行所有 `register`，再合并基础配置、顺序执行 `boot`，最后校验配置与贡献并解析必需服务。请求 `Inject<T>` 从请求 `Scope` 异步解析；每次命令使用独立 scope，Provider 使用同一应用容器。`AppState::get<T>` 仅同步读取已就绪的实例，首次工厂构造使用 `resolve<T>().await`。应用、请求 scope、受管任务和连接池通过 `ResourceOwner` 在共同期限内关闭，错误不会静默忽略。

## 入口作用域与资源回收

- HTTP：每个请求创建独立 scope，响应 body 仍持有它；body 结束、出错或丢弃，以及请求处理被取消时关闭。流式响应尚未结束时，scope 仍然有效。
- 命令：每次 `App::run_command` 有独立 scope；长驻的 `queue work` 命令 scope 不代替每次任务尝试的 scope。
- 队列：每次 job attempt 使用新 scope，成功、失败、超时或取消都会关闭；重试不会复用上一次的 scoped 实例。`JobContext::service` 从该 attempt 解析。直接调用没有应用状态的 `Queue::run_next` 时，不提供应用服务上下文。
- 调度：每个到期时段的事务派发使用独立 scope，后续 worker 执行时再创建 job attempt scope。当前全局调度不继承调用者的租户、请求 ID 或其他执行元数据；调度项目前不声明租户。

`Scope::run` 为当前 future 安装 scope，让同一容器的 `AppState::resolve` 使用它。普通 `tokio::spawn` 不继承 task-local scope 或执行上下文；需要共享本次生命周期的后台工作时，用 `Scope::spawn` 登记受管任务，显式传入允许传播的元数据和 deadline。scope 关闭时会请求取消并等待受管任务；任务仍需合作检查取消信号。例如，在已取得 `Arc<Scope>` 的代码中：

```rust,ignore
use openrust::core::ExecutionMetadata;

scope.spawn(ExecutionMetadata::default(), None, |cancel| async move {
    cancel.cancelled().await;
})?;
// 在 scope 的所有者处：
scope.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5)).await?;
```

`Scope::shutdown(deadline).await` 关闭解析并等待资源清理；`Scope::close()` 和 Drop 触发关闭与有界回收，但调用者不能据此认定异步清理已结束。超过期限会返回错误，已逃逸的 `Arc` 仍由持有者持有，不能强制销毁；关闭后通过该 scope 新解析返回 `scope_closed`。应用对活动子 scope 有容量限制，达到上限返回 `scope_capacity_exceeded`；生命周期标识耗尽返回 `container_capacity_exceeded`，调用方应处理这些错误。

自定义队列 driver 若在超时或租约丢失时丢弃 handler future，应在确认或重试前显式等待 `JobAttempt::shutdown`。每次重试都创建新的 context 和 guard；`JobRegistry::execute` 会复用已创建的 attempt scope。下面省略 driver 的租约和持久化逻辑：

```rust,ignore
use openrust::core::{Cancellation, Error};
use openrust::queue::JobContext;
use std::time::Duration;

let attempt = JobContext::new(job_id, attempt_number, max_attempts, Cancellation::new())?
    .with_app_state(app_state.clone())
    .begin_attempt()?;
let result = {
    let execution = registry.execute(job_name, payload, attempt.context().clone());
    tokio::pin!(execution);
    tokio::select! {
        result = &mut execution => result,
        () = tokio::time::sleep(Duration::from_secs(30)) => {
            Err(Error::custom("job_timeout", "job attempt timed out"))
        }
    }
}; // 先丢弃执行 future
let cleanup = attempt.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)).await;
match (result, cleanup) {
    (Err(error), Err(cleanup_error)) => {
        // 安全记录 cleanup_error 的错误码，然后保留首个错误。
        let _ = cleanup_error.code();
        return Err(error);
    }
    (Err(error), _) | (Ok(()), Err(error)) => return Err(error),
    (Ok(()), Ok(())) => {}
}
```

`JobAttempt` 的 Drop 是取消兜底，不等待回收。已开始的 context 不能重绑定到另一个应用：`scope`、`service` 和 `JobRegistry::execute` 会以 `queue_attempt_container_mismatch` 拒绝跨容器使用。将 context 留在原应用内使用；若 handler 与清理均失败，调用方应保留首个业务错误并安全记录清理失败。

## 当前应用启动资产

其他 OpenRust package 所依赖的核心契约包括：

- [`Application`]：项目实现一次该 trait，用于描述应用的组装方式。
- [`ProviderBuilder`] / [`ServiceProvider`] / [`App`] / [`AppState`]：框架、应用模块和第三方 package 使用同一扩展点与容器。`register` 声明绑定和配置区段，`boot` 贡献路由、命令及 middleware。依赖可在所有注册完成后解析；配置覆盖在最终校验前仍可提交。`App::run_command` 允许同一应用依次执行命令；顶层 CLI 在命令结束后调用 `App::shutdown`，HTTP `serve` 在退出时合作取消、限时回收受管资源。
- [`Command`]：应用二进制对外提供的运行时命令（`serve`、`migrate`、`queue work` 等）。命令默认拒绝额外参数；接受参数的命令须实现 `accepts_args`。
- [`Error`] / [`Result`] / [`ApiResponse<T>`] / [`PageMeta`]：统一错误类型、JSON 响应封装和类型化分页元数据。
- [`ExecutionContext`] / [`ExecutionMetadata`] / [`Cancellation`]：单次 HTTP、命令或 Job 尝试的 task-local 关联信息、时间预算和合作取消；task-local 不自动传播至 `tokio::spawn`。
- [`RateLimiter`] / [`RateLimitStore`]：固定窗口配额服务契约。存储故障向上传播；超限请求返回 429 和 `Retry-After`；每种实现都需说明共享范围与原子性边界。
- [`Readiness`]：进程 listener 生命周期状态，并允许 Provider 注册有界异步依赖探针；HTTP `/ready` 聚合探针结果，`is_ready()` 本身仍只表示 listener 状态。
- `config`：类型化配置按领域组织在 `openrust::config::{app,api,auth,database,logger,mailer,server}`，并由统一加载器合并：crate 默认值 → 根 `config/*.toml` 按文件名排序读取 → 应用工作目录 `.env` → 进程环境变量。领域文件如 `app.toml`、`server.toml`、`cache.toml`；环境差异与密钥放 `.env` 或部署环境变量，不再按环境拆 TOML。`APP_NAME`、`APP_URL` 可直接配置应用名与地址。若进程尚未设置 `APP_ENV`，启动引导阶段先从 `.env` 选择环境。进程环境变量优先；未显式选择目录且默认 `config/` 不存在时可使用内置默认；显式目录缺失、为空、非目录或不可读均失败，已发现的 TOML 必须可读且格式有效。错误、`Debug` 和引导日志不回显目录原值。`APP_CONFIG_DIR` 用于指定运行时 TOML 目录；`src/config/` 仅存放 Rust 配置代码。`Application::config_namespace()` 可选用其他有效前缀，如 `SHOP_`；只读取选定的命名空间。旧配置按领域重新导出以保持兼容。
- [`lifecycle`]：启动顺序、优雅停机信号与应用后台刷新任务回收。

应用内核位于根 `openrust` package 的 `src/core/`；`config`、`http`、`db`、`auth` 和 `mail` 是同一 package 中的兄弟 Rust 模块。应用依赖公开的 `openrust` package。CLI、proc-macro 和可复用测试工具继续作为独立 workspace package，维持工具与构建边界。

`ServerConfig` 包含 `[server].trusted_proxy_ranges`（默认空）、`[server.compression]`（默认启用）和 `[server.cors]`（默认禁用）。可信代理项接受 IP 或 CIDR，由 HTTP `ClientIp` extractor 使用；只有直接 socket peer 可信时才处理转发头，否则忽略任意转发头。CORS 默认允许来源列表为空，方法为 GET/POST/PUT/PATCH/DELETE/OPTIONS，请求头为 `content-type`/`authorization`/`x-request-id`，暴露 `x-request-id`，不启用 credentials，预检缓存 600 秒。环境变量使用 `APP_SERVER_CORS_ENABLED=true`；列表采用 Figment 数组语法，例如 `APP_SERVER_CORS_ALLOWED_ORIGINS='["https://example.com", "https://other.example"]'`。

HTTP 响应使用 `ApiResponse<T>`，包含 `success`、稳定的 `code`、安全的 `message`、有业务载荷时出现的 `data`、可选的类型化 `PageMeta` / `errors`；错误 JSON 额外包含 `request_id` 和 RFC 3339 UTC 字符串 `timestamp`（毫秒精度）。每个 HTTP 响应都通过 `x-request-id` 响应头提供请求关联 ID，成功 JSON 正文不重复携带诊断字段。无载荷时省略 `data`，空集合仍返回 `[]`。错误可使用 `Error::custom(code, message)`（semantic 模式默认状态码为 400）或 `Error::custom_with_status(status, code, message)`。`Error::rate_limited(seconds)` 会生成 `Retry-After`；`Error::validation_with_detail` 提供 422 字段错误。5xx 错误详情会隐藏。HTTP 并发上限触发的 `http_overloaded` 属于预期拒绝，不逐请求写 ERROR 日志；启用指标时由固定标签的过载计数器统计。HTTP middleware 为每个请求设置不可变的 `request_id` 和 `[api] http_status`。默认 `always_ok` 将响应状态设为 HTTP 200；`semantic` 使用 [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md) 中的状态码表。设置 `APP_API_HTTP_STATUS=semantic` 可覆盖配置。

`RateLimiter::check(key, limit, window)` 会在已注册的 `RateLimitStore` 中计入一次请求，并返回剩余配额；超限时返回带 `Retry-After` 的 429 错误。核心契约不决定 HTTP 身份提取方式或 middleware 策略。调用方应按策略命名 key，并对同一个 key 使用一致的时间窗口；存储故障不会被当成未超限。数据库 store 保存 SHA-256 摘要而非原始 key，但无法阻止对低熵值的猜测。

`FrameworkConfig.i18n` 定义 locale 默认值。`I18nServiceProvider` 注册共享 `I18n` 服务；HTTP middleware 解析 `Accept-Language`，并为当前 task 设置请求 locale。框架响应和内置 validator 规则消息使用该 locale，同时保留错误码与字段路径。应用消息可从 JSON 资源合并，并通过 `RequestLocale` 翻译；普通 `{name}` 文本保持兼容，也可用有限 `plural` / `select` 分支。`I18n::format_number` 接受精确十进制字符串；`format_date` 接受已按业务时区确定的日期，`format_datetime` 按传入 Chrono 值的本地墙上时间格式化。显式 task scope 使用 `I18n::with_locale`。

`openrust::validation::validate(value)` 可在命令与 typed Job 中复用 HTTP `Validated<T>` 的 validator 错误转换。`ExecutionContext::current()` 读取单次调用的 metadata、剩余 budget 与 cancellation；子调用 deadline 不超过父级，子 timeout 不会取消父级。队列可传播经校验的 request ID、traceparent、locale 和可信租户 ID；不序列化用户身份或请求凭据；见 [ADR-044](../../.agents/project/decisions/adr-044-cross-protocol-execution-context.md)。

```json
{
  "items": {
    "plural": {
      "argument": "count",
      "forms": { "one": "{count} item", "other": "{count} items" }
    }
  },
  "member": {
    "select": {
      "argument": "role",
      "cases": { "admin": "管理员", "other": "成员" }
    }
  }
}
```

`plural` 使用 ICU/CLDR 类别键（`zero`、`one`、`two`、`few`、`many`、`other`），`select` 按参数精确匹配有限字符串键；两者都必须提供 `other`。缺少参数或分支时走 `other`，模板中缺少的 `{name}` 仍保留原占位符。地区格式使用 ICU4X 编译数据，无效十进制或不可格式化日期返回错误，不会改写 API 数值字段。

邮件支持 `LocalizedEmail`，可使用显式选择的 locale 从共享 i18n catalog 渲染主题和正文；其他格式仍可通过自定义 `Email` 实现。SMS 使用 `openrust::sms::{SmsMessage, SmsSender, SmsServiceProvider}` 和公开的 `SmsDriver` 契约。默认 log driver 仅用于本地；`sms-twilio` 会启用 Twilio Messages adapter 和有界共享 HTTP client。生产环境拒绝 log driver。成功 receipt 仅表示供应商接受请求；异步队列重试和最终送达跟踪是独立能力。

`ConfigLoader::figment()` 现在返回 `Result<Figment, ConfigError>`，直接调用者使用 `?` 传播来源错误；`load`/`load_framework` 使用同一失败语义。此行为不修改领域默认值、环境变量优先级或具名driver配置。
