# 应用核心

OpenRust 框架的应用内核。

其他 OpenRust package 所依赖的核心契约包括：

- [`Application`]：项目实现一次该 trait，用于描述应用的组装方式。
- [`AppBuilder`] / [`App`] / [`AppState`]：编译期组装的应用与类型化服务注册表。Handler 和命令显式获取所需服务，不使用运行时容器或反射。`insert` / `command` 的同类重复注册由后注册项替换；需要拒绝冲突时使用 `try_insert` / `try_command`。
- [`Plugin`]：框架能力、应用模块和第三方 package 的统一扩展点。宿主按静态顺序运行 `build` 并异步调用 `finish`；依赖插件可用 `try_get` 检查前序服务。后台任务由启动它的命令管理，不提供自动发现、卸载或全局关闭阶段。
- [`Command`]：应用二进制对外提供的运行时命令（`serve`、`migrate`、`queue work` 等）。命令默认拒绝额外参数；接受参数的命令须实现 `accepts_args`。
- [`Error`] / [`Result`] / [`ApiResponse<T>`] / [`PageMeta`]：统一错误类型、JSON 响应封装和类型化分页元数据。
- [`ExecutionContext`] / [`ExecutionMetadata`] / [`Cancellation`]：单次 HTTP、命令或 Job 尝试的 task-local 关联信息、时间预算和合作取消；task-local 不自动传播至 `tokio::spawn`。
- [`RateLimiter`] / [`RateLimitStore`]：固定窗口配额服务契约。存储故障向上传播；超限请求返回 429 和 `Retry-After`；每种实现都需说明共享范围与原子性边界。
- `config`：类型化配置按领域组织在 `openrust::config::{app,api,auth,database,logger,mailer,server}`，并由统一的分层加载器合并：crate 默认值 → `config/default.toml` → `config/<env>.toml` → 非生产环境 `.env` → 进程环境变量。`APP_CONFIG_DIR` 用于指定运行时 TOML 目录（默认是相对于工作目录的 `config`）；`src/config/` 仅存放 Rust 代码。若进程尚未设置 `APP_ENV`，启动引导阶段才从 `.env` 读取；生产环境跳过整个 `.env` 文件。`Application::config_namespace()` 可选用其他有效前缀，如 `SHOP_`；只读取选定的命名空间。进程环境变量优先级最高，当前各能力模块中的配置路径继续通过重新导出保持兼容。
- [`lifecycle`]：启动顺序与优雅停机信号处理。

应用内核位于根 `openrust` package 的 `src/core/`；`config`、`http`、`db`、`auth` 和 `mail` 是同一 package 中的兄弟 Rust 模块。应用依赖公开的 `openrust` package。CLI、proc-macro 和可复用测试工具继续作为独立 workspace package，维持工具与构建边界。

`ServerConfig` 包含 `[server].trusted_proxy_ranges`（默认空）、`[server.compression]`（默认启用）和 `[server.cors]`（默认禁用）。可信代理项接受 IP 或 CIDR，由 HTTP `ClientIp` extractor 使用；只有直接 socket peer 可信时才处理转发头，否则忽略任意转发头。CORS 默认允许来源列表为空，方法为 GET/POST/PUT/PATCH/DELETE/OPTIONS，请求头为 `content-type`/`authorization`/`x-request-id`，暴露 `x-request-id`，不启用 credentials，预检缓存 600 秒。环境变量使用 `APP_SERVER__CORS__ENABLED=true`；列表采用 Figment 数组语法，例如 `APP_SERVER__CORS__ALLOWED_ORIGINS='["https://example.com", "https://other.example"]'`。

HTTP 响应使用 `ApiResponse<T>`，包含 `success`、稳定的 `code`、安全的 `message`、`data`、可选的类型化 `PageMeta` / `errors` 以及 `request_id`。错误可使用 `Error::custom(code, message)`（semantic 模式默认状态码为 400）或 `Error::custom_with_status(status, code, message)`。`Error::rate_limited(seconds)` 会生成 `Retry-After`；`Error::validation_with_detail` 提供 422 字段错误。5xx 错误详情会隐藏。HTTP middleware 为每个请求设置不可变的 `request_id` 和 `[api] http_status`。默认 `always_ok` 将响应状态设为 HTTP 200；`semantic` 使用 [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md) 中的状态码表。设置 `APP_API__HTTP_STATUS=semantic` 可覆盖配置。

`RateLimiter::check(key, limit, window)` 会在已注册的 `RateLimitStore` 中计入一次请求，并返回剩余配额；超限时返回带 `Retry-After` 的 429 错误。核心契约不决定 HTTP 身份提取方式或 middleware 策略。调用方应按策略命名 key，并对同一个 key 使用一致的时间窗口；存储故障不会被当成未超限。数据库 store 保存 SHA-256 摘要而非原始 key，但无法阻止对低熵值的猜测。

`FrameworkConfig.i18n` 定义 locale 默认值。`I18nPlugin` 会注册不可变的 `I18n` 服务；HTTP middleware 解析 `Accept-Language`，并为当前 task 设置请求 locale。框架响应和内置 validator 规则消息使用该 locale，同时保留错误码与字段路径。应用消息可从 JSON 资源合并，并通过 `RequestLocale` 翻译；普通 `{name}` 文本保持兼容，也可用有限 `plural` / `select` 分支。`I18n::format_number` 接受精确十进制字符串；`format_date` 接受已按业务时区确定的日期，`format_datetime` 按传入 Chrono 值的本地墙上时间格式化。显式 task scope 使用 `I18n::with_locale`。

`openrust::validation::validate(value)` 可在命令与 typed Job 中复用 HTTP `Validated<T>` 的 validator 错误转换。`ExecutionContext::current()` 读取单次调用的 metadata、剩余 budget 与 cancellation；子调用 deadline 不超过父级，子 timeout 不会取消父级。队列只允许有界 request ID 和 locale 入库，不序列化身份或请求凭据；见 [ADR-044](../../.agents/project/decisions/adr-044-cross-protocol-execution-context.md)。

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

邮件支持 `LocalizedEmail`，可使用显式选择的 locale 从共享 i18n catalog 渲染主题和正文；其他格式仍可通过自定义 `Email` 实现。SMS 使用 `openrust::sms::{SmsMessage, SmsSender, SmsPlugin}` 和公开的 `SmsDriver` 契约。默认 log driver 仅用于本地；`sms-twilio` 会启用 Twilio Messages adapter 和有界共享 HTTP client。生产环境拒绝 log driver。成功 receipt 仅表示供应商接受请求；异步队列重试和最终送达跟踪是独立能力。
