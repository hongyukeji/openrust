# 变更日志

OpenRust 的重要变更记录于此，格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)。OpenRust 尚未发布正式版本。

## [Unreleased]

### Changed

- 应用端口环境键统一为 `APP_PORT`（自定义命名空间如 `SHOP_PORT`），映射至内部 `server.port`；`.env` 与生成应用使用 `APP_URL="http://localhost:${APP_PORT}"`，旧端口别名不再读取。

- 认证示例及生成应用的密码重置确认字段改为 Laravel 式 `password`，保留密码首尾空白，与登录使用相同规范化规则。

- API/framework JSON 默认使用语义 HTTP 状态（如创建 201、校验失败 422、资源缺失 404）；`always_ok` 仅在配置中显式选择时启用，统一 envelope 与机器码保持不变。

### Fixed

- 本地存储回收 staging 时允许其他应用实例关闭并删除已列举的 run 目录；权限和其他 I/O 错误继续阻止初始化。

- 修复容器缓存完成通知早于构造帧结束的误报循环，以及生命周期关闭通知持锁时的同步 Waker 重入死锁。

- 配置目录显式错误不再静默退默认；已发现 TOML 精确读取，来源错误安全传播，Debug/启动日志不输出目录原值。

- 在 API 响应中保留类型化 `PageMeta`；明确 CORS 预检响应正文为空，同时普通跨域错误仍使用统一包装。

### Added

- 新增 Laravel 式 Cookie 读取、值工厂、请求隔离队列与响应组合；受保护模式复用现有密钥环，保留原始头的严格认证检查。唯一模块及生成应用展示 Cookie 往返，显式明文模式不提供加密保护。

- `Request` 新增 Laravel 式标量、字符串、集合、枚举、clamp 与条件读取；共用有界输入内核，`Fluent` 继承请求日历上下文。日期和区间使用 PHP 格式与语义，应用时区通过 `app.timezone` / `APP_TIMEZONE` 配置，可注入请求级时钟；唯一模块与生成应用提供公开 API 使用示例。

- 新增统一正文所有者 `Request`，提供 Laravel 式输入来源合并、dot/通配读取、存在性、筛选与输入修改；`Json` / `Query` 共享默认 trim/null 转换，公开 `InputNormalization` 支持路由或请求覆盖，原始正文保持不变。

- 新增只读 `RequestInfo` 提取器：检查路径、方法、严格 Host、头与 bearer，并按固定 Laravel/Symfony 行为执行内容协商；与 `Json<T>` 共存，限制元信息大小且 Debug 隐藏敏感内容。唯一模块示例展示 HTML/JSON 协商与正文读取。

- HTTP 路由支持 `RouteGroup::domain` 与 `DomainParams<T>`：域路由按声明顺序在普通路由前匹配，具名域 URL 由登记模板、单标签参数和 `app.url` scheme/port 生成；畸形 authority 失败关闭，转发 Host 不受信任，业务 404/405 不触发二次派发。

- HTTP 路由支持 `.name(...)`、按原样组合的组名 `name_prefix` 与 `RouteUrlGenerator`；路由参数安全编码，额外参数生成有序 query，URL 来源固定为 `app.url`。

- Provider 可登记可复用的 HTTP middleware alias 与递归 group，路由及嵌套组可按名称选择应用；未知名称、重名与 group 循环显式报错，无应用注册表时 `Routes::into_router()` 也会失败关闭。

- HTTP 路由新增 Laravel 式 `Routes::fallback`；支持静态 RouteGroup 前缀与 middleware，GET/HEAD 未命中时优先服务 public 静态文件（含 `/api`、`/ws`），原始 URI/body 传递给业务 handler，同 scope 重复 fallback 稳定报错。参数化/通配符组前缀暂报 `http_fallback_scope_unsupported`，避免 Axum catch-all panic。

- R1-E 在唯一 `modules/example` 中加入有所有者约束的 Article CRUD，并以实际业务路径展示租户、附件、缓存、队列、认证、实时通信、数据交换、国际化与观测等当前公开能力；具体真实服务范围和仍未关闭的边界见示例 README 与验收记录。

- `openrust new --grpc` 生成 typed Echo 服务与 Provider，并提供应用二进制 `serve all` 的 HTTP + TLS gRPC 联合运行测试。`--auth-mfa` 生成的 HTTP 验收现覆盖真实 TOTP 挑战、恢复码一次性与撤销；与 gRPC、认证、数据库组合时也生成可运行测试配置。

- R2-M 新增编译期 `ModuleDescriptor`、运行时 `ModuleManifest`/文件激活快照、模块配置与资源路径加载、命名空间视图和翻译，以及按全局名称/批次执行的统一迁移历史。根应用与生成项目从运行根读取模块状态；示例业务完整移至 `modules/example/`。管理写命令与完整发布流程仍属后继。

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

- 业务示例通过公开模块清单和统一 Provider 接入根唯一应用，专属迁移、资源、协议和测试位于 `modules/example/`；根不保留重复应用或 `examples/` 目录。CLI 生成项目也提供模块清单和文件状态入口。

- 根 CLI 运行命令按 Cargo metadata 默认应用转发，根 server 从根职责目录读取配置与静态资源。

- 错误响应中的 `timestamp_ms` Unix 毫秒数改为 `timestamp` RFC 3339 UTC 字符串（毫秒精度）。

- 将默认运行时配置命名空间切换为 `APP_`；应用可通过 `Application::config_namespace()` 选择一个自定义前缀。
- 将所有 OpenRust JSON 成功和错误响应统一为 `ApiResponse<T>`，并新增 `[api] http_status`（`always_ok` 或 `semantic`）、请求 ID 正文/响应头一致性及限流重试响应头。

- 将 facade prelude 切换为 OpenRust 提取器，并按照 ADR-008 统一校验、超时和未匹配路由响应。

- 按照 ADR-006 在已实现 API 中统一采用以下名称：`Plugin::build/finish`、`AppBuilder::plugin/insert/get`、`AppState::get/try_get`、`HttpPlugin`、`Routes::nest`，以及资源操作 `list/get/create/update/delete`。
- 将运行时配置切换为 `config/default.toml`、`OPENRUST_ENV` 和 `OPENRUST_<SECTION>__<KEY>`；`DATABASE_URL` 和 `PORT` 映射到对应配置区段，带前缀的变量优先。
- 将配置命令改为 `config show`，并记录未来的 `openrust generate` CLI 规划。
