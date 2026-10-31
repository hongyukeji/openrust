# 应用核心

OpenRust 框架的应用内核。

## 容器与 Provider 生命周期

`openrust::core::container` 提供 `ContainerBuilder`、`Container`、`Resolver` 与 `Scope`。`Resolver` 使用弱容器句柄，延迟解析时容器已释放则报告 `container_closed`，避免 singleton 保存解析器造成引用环。类型键支持 `dyn Trait`，`bind` 每次创建，`singleton` 同容器共享，`scoped` 同生命周期共享，`instance` 注册已有 `Arc`。同类型后注册覆盖前定义；`build` 建立唯一容器后，Provider 注册仍可在同一容器动态重绑，不使用 PHP 反射构造。重绑使各 scope 的旧缓存失效；已交给调用方的旧 `Arc` 和在途引用仍有效，新解析按新定义构造。绑定版本与缓存槽准入同锁核验，旧构造不能回填新槽。

`RegisterContext::extend::<T, _>(...).await?` 保留原 factory，再把异步装饰器依登记顺序作用于其结果；可在 `bind` 前登记，也可包装外部 `Arc<dyn Trait>`。装饰器接收旧 `Arc<T>` 与 `Resolver`，返回 `BoxFuture<'static, Result<Arc<T>>>`。同类型 factory 覆盖仍保留未解析服务的持久装饰链；`forget_extenders::<T>().await?` 清除未来构造的持久链，不撤销已缓存服务。`rebinding::<T, _>(...).await?` 登记监听者；服务已绑定时立即解析并返回当前 `Arc`，后续缓存更新按顺序通知。回调错误在当前 `register` 位置短路，不延迟到 `register` 返回后。已缓存 singleton/instance 可通过 `Container::extend(...).await` 或 `BootContext::extend(...).await` 即时变换；变换失败保留旧缓存，通知失败则新值已发布。`ServiceProvider::register` 返回 `BoxFuture`，绑定、实例、装饰与通知均在原调用位置 `await?`；已完成的贡献即使后续操作失败也不回滚。直接使用 `ContainerBuilder` 时，显式 `instance` 的待执行装饰仍在首次异步 `resolve` 执行；待执行时同步 `get_ready` 不返回旧值。

```rust,ignore
app.extend::<dyn Driver, _>(|original, resolver| {
    Box::pin(async move {
        let dependency = resolver.resolve::<Dependency>().await?;
        Ok(Arc::new(ObservedDriver::new(original, dependency)) as Arc<dyn Driver>)
    })
}).await?;
```

运行期 `Container::extend` 处理 singleton/instance 与根 scope 当前值，不主动替换其他活跃请求 scope 的缓存；请求内用 `Scope::extend` 只装饰该 scope 的 scoped/bind 服务，singleton 仍归应用。已返回调用方的旧 `Arc` 不会自动变成新值。此异步时点和多活跃 scope 是 Rust 适配；alias/contextual 等 Container 后继契约仍开放。唯一业务示例在 `modules/example/providers/example.rs` 装饰配置驱动的 `ExampleMessage`，`/api/example/container-extension` 展示原 greeting、装饰标记及 boot/HTTP 同一服务实例。CLI 新建应用只有空模块清单，不自动复制该业务模块；应用可按 Provider 模式注册自己的装饰器。

根容器也有 scoped 生命周期，`forget_scoped_instances()` 清除旧代，下次解析重建；子 scope 独立。关闭/取消在途解析后可重新构造，不保留失败占位；同链与并发等待环返回类型链。singleton 捕获 scoped 被拒绝，这是长驻进程安全增强。错误 `Debug` 只输出 code/status，工厂原始错误保留在内部 source，不直接输出 HTTP。

缓存工厂在成功、失败或取消时先结束构造帧并清除已结束的等待边，再发布结果；已完成服务不再创建等待关系。scope关闭和根reset的唤醒、用户值释放都在容器内部锁之外执行，允许Waker同步重入。

生产 `bootstrap` 使用同一 `Container`：`Application::providers` 给出清单，`ProviderBuilder` 先建立唯一容器，依清单顺序执行异步 `register`，经基础配置校验后依 active 顺序 `boot`，最后校验配置与贡献并解析必需服务。普通重复具体 Provider 类型只保留首次实例，显式 `force_provider` 则重新执行 `register`，成功后在原首次 boot 槽位替换实例。`App::register_provider` 与 `force_register_provider` 在启动后使用相同容器；新实例注册成功便立即 boot。注册失败不更新 Provider 索引，但此前贡献可见；boot 失败保留新索引和贡献，普通重复不会重试 boot。请求 `Inject<T>` 从请求 `Scope` 异步解析；每次命令使用独立 scope，Provider 使用同一应用容器。`AppState::get<T>` 仅同步读取已就绪的实例，首次工厂构造使用 `resolve<T>().await`。启动阶段及运行期 Provider 登记的资源都归同一应用 `ResourceOwner`；force 不提前关闭旧资源，失败 boot 已登记的资源仍在统一 shutdown deadline 内逆序回收。启动 register/校验失败或启动 future 取消时，已提前解析的 root scoped 资源与 Provider 资源共用同一回收期限；关闭开始后拒绝新 Provider 登记。请求 scope、受管任务和连接池也遵循该共同期限，错误不会静默忽略。配置区段及包默认值在各次注册后即对后续解析可见，已有来源值始终高于 Provider 默认值；不同 Provider 可以按登记顺序对同一包区段贡献默认值，schema/校验器只允许原 owner 显式 force 替换。最终仍经过 `validate_base` 校验，包操作仅应用一次；`App::config()`/`figment()` 保持启动快照，`config_repository()`/`section()` 读取运行期仓储，不自动重建既有服务。运行期路由贡献可被随后 `build_router` 读取；已运行的 Axum Router 是快照，其实时可见性仍待固定上游真实 HTTP 对照。

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

### 测试应用

`openrust::testing::TestApp<A>` 使用与生产相同的 `Application::providers`、`ProviderBuilder` 和 Router。测试显式传入 `Figment`，不修改全局环境或工作目录。`remove_provider(ProviderId)` 在注册前调整清单；`configure_after_register` 在所有 Provider 注册后、boot 前通过同一 `Container` 异步覆盖绑定，因此后续 `boot` 能读取替代服务。测试可通过 `request` 发起进程内请求，默认总期限五秒、响应体上限 1 MiB；`request_with_limits` 可调整这两个预算，`raw_response` 保留流式正文并由调用方消费或丢弃。

`TestResponse` 提供状态、响应头、字节和 JSON 读取以及链式断言；断言失败不会回显正文或头值。状态断言遵循应用配置的 `[api] http_status`，默认为 Laravel 式语义状态；仅显式配置 `always_ok` 时才将框架 JSON 错误映射为 HTTP 200。`shutdown(deadline).await` 等待本应用拥有的资源回收；`reset(deadline).await` 先关闭整个旧应用，再用相同蓝图重建，不把根 scoped 缓存清空冒充完整重置。这里的 deadline 只约束旧应用关闭，新应用的启动与路由构建不受同一期限约束。丢弃 `TestApp` 会立即封闭新解析；存活的 `Tokio` runtime 才能继续异步兜底回收。独立 `TestApp` 持有各自容器，捕获的外部共享 `Arc` fixture 由调用方负责重置。实际业务替换与 Provider 移除示例见 [`modules/example`](../../modules/example/README.md)，CLI 生成项目也包含普通 HTTP 与认证测试。进程内请求不替代真实 TCP 验收。

其他 OpenRust package 所依赖的核心契约包括：

- [`Application`]：项目实现一次该 trait，用于描述应用的组装方式。
- [`ProviderBuilder`] / [`ServiceProvider`] / [`App`] / [`AppState`]：框架、应用模块和第三方 package 使用同一扩展点与容器。`register` 声明绑定和配置区段，`boot` 贡献路由、命令及 middleware。依赖可在所有注册完成后解析；配置覆盖在最终校验前仍可提交。`App::run_command` 允许同一应用依次执行命令；顶层 CLI 在命令结束后调用 `App::shutdown`，HTTP `serve` 在退出时合作取消、限时回收受管资源。
- [`ModuleManifest`](module.rs) 在编译期声明模块描述，启动时从只读状态源取得启用快照；启用的模块 Provider 展平进入同一注册与顺序 boot 生命周期。`ModuleDescriptor::requires` 声明已编译模块的硬依赖；依赖必须同时启用，循环/缺失/禁用依赖启动失败。依赖拓扑优先于 `priority`，相同优先级按清单声明顺序稳定排序，模块 provider 的 register 与 boot 使用同一顺序；不会自动更改状态文件。`ModuleConfigFiles::overlay` 由模块 Provider 在实际配置时点调用，逐文件写入共享配置仓储。文件状态仅 JSON `true` 启用，缺文件/键不启用，坏 JSON 拒绝；禁用不热卸载、不删除迁移历史。默认模块运行根是进程工作目录，应用可显式传入部署根；编译时 `CARGO_MANIFEST_DIR` 不决定运行时资源位置。模块资源、语言、视图和迁移示例见 [`modules/example`](../../modules/example/README.md)。
- [`Command`]：应用二进制对外提供的运行时命令（`serve`、`migrate`、`queue work` 等）。命令默认拒绝额外参数；接受参数的命令须实现 `accepts_args`。
- 控制台入口在引导前确定命令名，Provider 可通过 `BootContext::console_command()` 与 `is_console_command()` 读取本次候选命令。普通 `bootstrap()`、直接 `ProviderBuilder` 与 `TestApp` 均无该上下文。框架按最终匹配的命令实例执行启动前检查；候选名本身不授权跳过校验。内部迁移入口允许空库先执行官方迁移，再由后续正常应用启动校验所需表。
- [`Error`] / [`Result`] / [`ApiResponse<T>`] / [`PageMeta`]：统一错误类型、JSON 响应封装和类型化分页元数据。
- [`ExecutionContext`] / [`ExecutionMetadata`] / [`Cancellation`]：单次 HTTP、命令或 Job 尝试的 task-local 关联信息、时间预算和合作取消；task-local 不自动传播至 `tokio::spawn`。
- [`RateLimiter`] / [`RateLimitStore`]：固定窗口配额服务契约。存储故障向上传播；超限请求返回 429 和 `Retry-After`；每种实现都需说明共享范围与原子性边界。
- [`Readiness`]：进程 listener 生命周期状态，并允许 Provider 注册有界异步依赖探针；HTTP `/ready` 聚合探针结果，`is_ready()` 本身仍只表示 listener 状态。
- `config`：类型化配置按领域组织在 `openrust::config::{app,api,auth,database,logger,mailer,server}`，并由统一加载器合并：crate 默认值 → 根 `config/*.toml` 按文件名排序读取 → 应用工作目录 `.env` → 进程环境变量。领域文件如 `app.toml`、`server.toml`、`cache.toml`；环境差异与密钥放 `.env` 或部署环境变量，不再按环境拆 TOML。`APP_NAME`、`APP_URL` 可直接配置应用名与地址。若进程尚未设置 `APP_ENV`，启动引导阶段先从 `.env` 选择环境。进程环境变量优先；未显式选择目录且默认 `config/` 不存在时可使用内置默认；显式目录缺失、为空、非目录或不可读均失败，已发现的 TOML 必须可读且格式有效。错误、`Debug` 和引导日志不回显目录原值。`APP_CONFIG_DIR` 用于指定运行时 TOML 目录；`src/config/` 仅存放 Rust 配置代码。`Application::config_namespace()` 可选用其他有效前缀，如 `SHOP_`；只读取选定的命名空间。旧配置按领域重新导出以保持兼容。
- [`lifecycle`]：启动顺序、优雅停机信号与应用后台刷新任务回收。

应用内核位于根 `openrust` package 的 `src/core/`；`config`、`http`、`db`、`auth` 和 `mail` 是同一 package 中的兄弟 Rust 模块。应用依赖公开的 `openrust` package。CLI、proc-macro 和可复用测试工具继续作为独立 workspace package，维持工具与构建边界。

`ServerConfig` 包含 `[server].trusted_proxy_ranges`（默认空）、`[server.compression]`（默认启用）和 `[server.cors]`（默认禁用）。可信代理项接受 IP 或 CIDR，由 HTTP `ClientIp` extractor 使用；只有直接 socket peer 可信时才处理转发头，否则忽略任意转发头。CORS 默认允许来源列表为空，方法为 GET/POST/PUT/PATCH/DELETE/OPTIONS，请求头为 `content-type`/`authorization`/`x-request-id`，暴露 `x-request-id`，不启用 credentials，预检缓存 600 秒。环境变量使用 `APP_SERVER_CORS_ENABLED=true`；列表采用 Figment 数组语法，例如 `APP_SERVER_CORS_ALLOWED_ORIGINS='["https://example.com", "https://other.example"]'`。

HTTP 响应使用 `ApiResponse<T>`，包含 `success`、稳定的 `code`、安全的 `message`、有业务载荷时出现的 `data`、可选的类型化 `PageMeta` / `errors`；错误 JSON 额外包含 `request_id` 和 RFC 3339 UTC 字符串 `timestamp`（毫秒精度）。每个 HTTP 响应都通过 `x-request-id` 响应头提供请求关联 ID，成功 JSON 正文不重复携带诊断字段。无载荷时省略 `data`，空集合仍返回 `[]`。错误可使用 `Error::custom(code, message)`（semantic 模式默认状态码为 400）或 `Error::custom_with_status(status, code, message)`。`Error::rate_limited(seconds)` 会生成 `Retry-After`；`Error::validation_with_detail` 提供 422 字段错误。5xx 错误详情会隐藏。HTTP 并发上限触发的 `http_overloaded` 属于预期拒绝，不逐请求写 ERROR 日志；启用指标时由固定标签的过载计数器统计。HTTP middleware 为每个请求设置不可变的 `request_id` 和 `[api] http_status`。默认 `semantic` 使用 [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md) 中的状态码表；只有显式设置 `APP_API_HTTP_STATUS=always_ok` 才将框架 JSON 响应状态映射为 HTTP 200。

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
