# `openrust` 根 package 当前事实

> 更新：2026-10-26
> 摘要：根 package 是框架唯一公共 facade 与实现 package；能力模块位于根 `src/`，包括 `openrust::audit`、`openrust::mail`、`openrust::sms`、可选 `openrust::security` 与 `openrust::signing`。

- 根 `src/lib.rs` 声明 audit/config/core/db/http/i18n/mail/sms，以及 feature-gated auth、queue/schedule、security 与 signing；对外主路径继续是 `openrust::...`。
- 根直接 re-export `ApiResponse<T>`、`PageMeta`、`#[openrust::resource]`、`ServiceExt` 与 Tokio。`openrust::prelude` 提供常用 HTTP、core、mail、db、auth 类型；Axum 类型由 `openrust::http::axum` 公开。
- `db-postgres` / `db-mysql` 启用 SeaORM 对应 backend，并提供 `openrust::database` 别名与常用 DB 类型；默认 feature 不含具体数据库 driver。
- `auth` 启用 JWT、Session、认证流程与授权模块；依赖 DB feature 的认证流程在运行时要求配置相应数据库及迁移。
- `queue` 启用 `openrust::queue` 与 `openrust::schedule`；数据库 driver、worker 和 UTC 调度通过应用显式注册，当前功能与验收边界见 [queue](queue.md) / [schedule](schedule.md)。
- `mail-smtp` 可选启用 SMTP transport；`sms-twilio` 复用可选 `http-client` feature；`test-util` 提供邮件/SMS fake。log 仅供非生产默认路径，生产启动拒绝 log；运行配置保持在应用项目根 `config/`。
- `encryption` 可选启用 AES-256-GCM-SIV 应用数据加密；`EncryptionPlugin` 注册持有轮换 key ring 的 `Encryptor`。仅 active key 写入、历史 key 解密；密钥需由受控配置注入，不含默认 key。
- `signing` 可选启用用途绑定的限时签名链接；独立 link key ring 与 JWT/加密密钥隔离；逐条撤销需应用加入共享数据库迁移并配置 `SignedLinkRevocationStore`。
- Workspace 保留 package：根 `openrust` 框架、`openrust-cli` 命令行与模板、`openrust-macros` proc-macro、`openrust-test` 测试工具、`openrust-server` 集成宿主。各理由见 [ADR-021](../decisions/adr-021-loco-style-module-layout.md)。
- `openrust-test` 单向依赖 `openrust`；根 package 不依赖测试工具，避免依赖环。
- 工具包已在工作区迁至 `cli/`、`macros/`、`testing/`；当前结构簇仍待验证与提交，不能把路径移动当作验收完成。独立 package 的编译边界与目录承接位置须先评估，不能把 proc-macro 机械并入普通库。
- Cache、文件存储、通用事件总线、实时通信及 M5 可观测性仍待实现，未从 facade 导出；现有 feature 不代表自动安装对应 plugin。

`cargo metadata --no-deps --format-version 1` 验收根 package `openrust` 及 workspace 五个成员；default、auth/database features 与 all-features 的验证按 [testing memory](../memory/testing.md) 记录。
