# 变更日志

OpenRust 的重要变更记录于此，格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)。OpenRust 尚未发布正式版本。

## [Unreleased]

### Fixed

- 在 API 响应中保留类型化 `PageMeta`；明确 CORS 预检响应正文为空，同时普通跨域错误仍使用统一包装。

### Added

- 新增共享固定窗口限流，以及使用 PostgreSQL/MySQL 存储、绑定用途的一次性验证码。

- 新增 OpenRust `Json` / `Path` / `Query` 提取器、`Created::with_location`，并为常见 HTTP 拒绝响应提供 JSON envelope 回退。

- 新增不发布的 `apps/server` 集成宿主，并设为 workspace 中 `cargo run` 的默认目标。
- 建立 Cargo workspace、共享 lint 策略、双许可证、CI 基线和框架架构决策。
- 新增应用核心，包括分层配置、类型化服务注册表、插件生命周期、运行时命令、结构化 HTTP 错误和可配置 JSON envelope。
- 新增 HTTP 路由、资源操作、类型化响应、校验与注入提取器、健康路由、中间件、CORS 和压缩。
- 新增 facade 应用运行入口、最小示例，以及可将运行时命令转发给项目二进制的 CLI。
- 新增贡献、安全和社区指南。

### Changed

- 错误响应中的 `timestamp_ms` Unix 毫秒数改为 `timestamp` RFC 3339 UTC 字符串（毫秒精度）。

- 将默认运行时配置命名空间切换为 `APP_`；应用可通过 `Application::config_namespace()` 选择一个自定义前缀。
- 将所有 OpenRust JSON 成功和错误响应统一为 `ApiResponse<T>`，并新增 `[api] http_status`（`always_ok` 或 `semantic`）、请求 ID 正文/响应头一致性及限流重试响应头。

- 将 facade prelude 切换为 OpenRust 提取器，并按照 ADR-008 统一校验、超时和未匹配路由响应。

- 按照 ADR-006 在已实现 API 中统一采用以下名称：`Plugin::build/finish`、`AppBuilder::plugin/insert/get`、`AppState::get/try_get`、`HttpPlugin`、`Routes::nest`，以及资源操作 `list/get/create/update/delete`。
- 将运行时配置切换为 `config/default.toml`、`OPENRUST_ENV` 和 `OPENRUST_<SECTION>__<KEY>`；`DATABASE_URL` 和 `PORT` 映射到对应配置区段，带前缀的变量优先。
- 将配置命令改为 `config show`，并记录未来的 `openrust generate` CLI 规划。
