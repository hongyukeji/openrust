# `openrust` 框架库当前事实

> 更新：2026-10-28
> 摘要：openrust package 是框架唯一公共 facade 与实现 package；能力模块位于根 `src/`，包括 `openrust::audit`、`openrust::mail`、`openrust::sms`、可选 `openrust::security` 与 `openrust::signing`。

- `src/lib.rs` 声明框架 modules 与 feature-gated 能力；根 `src/main.rs` 提供 `server` bin。对外主路径继续是 `openrust::...`。
- 库直接 re-export `ApiResponse<T>`、`PageMeta`、`#[openrust::resource]`、`ServiceExt` 与 Tokio。`openrust::prelude` 提供常用 HTTP、core、mail、db、auth 类型；Axum 类型由 `openrust::http::axum` 公开。
- `openrust::validation::{Validate, validate}` 复用 validator 与统一字段错误；`ExecutionContext` / `ExecutionMetadata` / `Cancellation` 提供单次请求、命令及 Job attempt 的显式作用域，细节见 [core](core.md) 与 [ADR-044](../decisions/adr-044-cross-protocol-execution-context.md)。
- `db-postgres` / `db-mysql` 启用 SeaORM 对应 backend，并提供 `openrust::database` 别名与常用 DB 类型；默认 feature 不含具体数据库 driver。
- `auth` 启用 JWT、Session、认证流程与授权模块；依赖 DB feature 的认证流程在运行时要求配置相应数据库及迁移。
- `queue` 启用 `openrust::queue` 与 `openrust::schedule`；数据库 driver、worker 和 UTC 调度通过应用显式注册，当前功能与验收边界见 [queue](queue.md) / [schedule](schedule.md)。
- `mail-smtp` 可选启用 SMTP transport；`sms-twilio` 复用可选 `http-client` feature；`test-util` 提供邮件/SMS fake。log 仅供非生产默认路径，生产启动拒绝 log；运行配置保持在应用项目根 `config/`。
- `encryption` 可选启用 AES-256-GCM-SIV 应用数据加密；`EncryptionServiceProvider` 注册持有轮换 key ring 的 `Encryptor`。仅 active key 写入、历史 key 解密；密钥需由受控配置注入，不含默认 key。
- `signing` 可选启用用途绑定的限时签名链接；独立 link key ring 与 JWT/加密密钥隔离；逐条撤销需应用加入共享数据库迁移并配置 `SignedLinkRevocationStore`。
- Workspace package：根 `openrust` package 提供名为 `openrust` 的 library，同一根 package 提供默认 server bin，另有 `cli` 命令行与模板、`macros` proc-macro、`testing` 测试工具和 `performance` 基准工具。源码交付与命名见 [ADR-039](../decisions/adr-039-source-workspace-distribution.md)。
- `testing` 单向依赖 `openrust` package（dependency alias `openrust`）；框架 package 不依赖测试工具，避免依赖环。
- CLI、proc-macro 与 testing 工具包保留独立编译/依赖边界；根集成宿主布局按 [ADR-073](../decisions/adr-073-root-application-and-example-module.md) 执行。不能把路径移动当作能力验收完成，也不能把 proc-macro 机械并入普通库。
- `openrust::cache`、`openrust::storage`、`openrust::realtime`、数据库、认证、队列、gRPC 等能力按各自可选 feature/扩展契约交付；Redis client 由对应 feature 启用，插件须显式安装和配置。各模块当前状态与未完成范围以 `core/<crate>.md` 和 `todo.md` 为准。


目录/分发以 [ADR-073](../decisions/adr-073-root-application-and-example-module.md) 和 [architecture](../architecture.md) 为准；独立生成应用不修改源 workspace。根 package 为 openrust library；同一根 package 提供唯一默认 server bin。
