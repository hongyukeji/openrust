# 变更日志

OpenRust 的重要变更记录于此，格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)。OpenRust 尚未发布正式版本。

## [Unreleased]

### Fixed

- 修复容器缓存完成通知早于构造帧结束的误报循环，以及生命周期关闭通知持锁时的同步 Waker 重入死锁。

- 配置目录显式错误不再静默退默认；已发现 TOML 精确读取，来源错误安全传播，Debug/启动日志不输出目录原值。

- 在 API 响应中保留类型化 `PageMeta`；明确 CORS 预检响应正文为空，同时普通跨域错误仍使用统一包装。

### Added

- R2-D 新增与生产共用 Provider/Router 启动路径的 `TestApp<A>`、可重放的 Provider 移除与注册后绑定覆盖、有界请求和安全响应断言；完整 `reset` 先关闭旧应用再重建。普通及认证项目生成模板包含对应测试，综合示例验证业务服务替换和路由移除。

- R2-C 为HTTP流式响应、命令、队列每次尝试和调度每次触发接入独立容器scope与受管清理；公开 `Scope::run`、`Scope::spawn`、`ScopeFactory` 和自定义队列driver可显式等待的 `JobAttempt`。

- 新增 R2-A 类型化容器基元及安全错误 Debug；生产 Provider/请求生命周期尚未切换。

- 新增共享固定窗口限流，以及使用 PostgreSQL/MySQL 存储、绑定用途的一次性验证码。

- 新增 OpenRust `Json` / `Path` / `Query` 提取器、`Created::with_location`，并为常见 HTTP 拒绝响应提供 JSON envelope 回退。

- 新增不发布的 `apps/server` 集成宿主，并设为 workspace 中 `cargo run` 的默认目标。
- 建立 Cargo workspace、共享 lint 策略、双许可证、CI 基线和框架架构决策。
- 新增应用核心，包括分层配置、类型化服务注册表、插件生命周期、运行时命令、结构化 HTTP 错误和可配置 JSON envelope。
- 新增 HTTP 路由、资源操作、类型化响应、校验与注入提取器、健康路由、中间件、CORS 和压缩。
- 新增 facade 应用运行入口、最小示例，以及可将运行时命令转发给项目二进制的 CLI。
- 新增贡献、安全和社区指南。

### Changed

- 应用关闭同步封闭容器的新解析与受管任务登记；`TestApp` 析构时即使没有运行时也先封闭，异步回收仍需显式 `shutdown` 或存活的 Tokio runtime。已持有的服务 `Arc` 由调用者管理。

- `App::run_command` 每次执行使用新scope，应用资源在显式关闭时回收；队列任务与调度触发不继承调用者的请求或租户身份。

- `ConfigLoader::figment()` 返回 `Result`，直接调用方须传播配置来源错误。

- 根 Cargo package openrust 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs；Laravel 应用职责目录回根，default-members=["."]、default-run="server"，根无参数 cargo run 启动唯一应用。

- 当前过渡 `app/example` 通过公开 Plugin 注册根唯一应用，专属迁移、资源、协议和测试保留模块内；不保留重复应用或根 examples 目录。`modules/example` 与统一 Provider/模块生命周期为后继目标，完整 CRUD 与能力示例尚未交付；CLI 保留用户应用生成功能。

- 根 CLI 运行命令按 Cargo metadata 默认应用转发，根 server 从根职责目录读取配置与静态资源。

- 错误响应中的 `timestamp_ms` Unix 毫秒数改为 `timestamp` RFC 3339 UTC 字符串（毫秒精度）。

- 将默认运行时配置命名空间切换为 `APP_`；应用可通过 `Application::config_namespace()` 选择一个自定义前缀。
- 将所有 OpenRust JSON 成功和错误响应统一为 `ApiResponse<T>`，并新增 `[api] http_status`（`always_ok` 或 `semantic`）、请求 ID 正文/响应头一致性及限流重试响应头。

- 将 facade prelude 切换为 OpenRust 提取器，并按照 ADR-008 统一校验、超时和未匹配路由响应。

- 按照 ADR-006 在已实现 API 中统一采用以下名称：`Plugin::build/finish`、`AppBuilder::plugin/insert/get`、`AppState::get/try_get`、`HttpPlugin`、`Routes::nest`，以及资源操作 `list/get/create/update/delete`。
- 将运行时配置切换为 `config/default.toml`、`OPENRUST_ENV` 和 `OPENRUST_<SECTION>__<KEY>`；`DATABASE_URL` 和 `PORT` 映射到对应配置区段，带前缀的变量优先。
- 将配置命令改为 `config show`，并记录未来的 `openrust generate` CLI 规划。
