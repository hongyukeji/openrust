# `openrust` 根 package 当前事实

> 更新：2026-10-26
> 摘要：根 package 是框架唯一公共 facade 与实现 package；能力模块位于根 `src/`，包括 `openrust::mail`、`openrust::sms` 与可选 `openrust::security`。

- 根 `src/lib.rs` 声明 config/core/db/http/mail/sms，以及 feature-gated auth 与 security；对外主路径继续是 `openrust::...`。
- 根直接 re-export `ApiResponse<T>`、`PageMeta`、`#[openrust::resource]`、`ServiceExt` 与 Tokio。`openrust::prelude` 提供常用 HTTP、core、mail、db、auth 类型；Axum 类型由 `openrust::http::axum` 公开。
- `db-postgres` / `db-mysql` 启用 SeaORM 对应 backend，并提供 `openrust::database` 别名与常用 DB 类型；默认 feature 不含具体数据库 driver。
- `auth` 启用 JWT、Session、认证流程与授权模块；依赖 DB feature 的认证流程在运行时要求配置相应数据库及迁移。
- `mail-smtp` 可选启用 SMTP transport；`sms-twilio` 复用可选 `http-client` feature；`test-util` 提供邮件/SMS fake。log 仅供非生产默认路径，生产启动拒绝 log；运行配置保持在应用项目根 `config/`。
- `encryption` 可选启用 AES-256-GCM-SIV 应用数据加密；`EncryptionPlugin` 注册持有轮换 key ring 的 `Encryptor`。仅 active key 写入、历史 key 解密；密钥需由受控配置注入，不含默认 key。
- Workspace 保留 package：根 `openrust` 框架、`openrust-cli` 命令行与模板、`openrust-macros` proc-macro、`openrust-test` 测试工具、`openrust-server` 集成宿主。各理由见 [ADR-021](../decisions/adr-021-loco-style-module-layout.md)。
- `openrust-test` 单向依赖 `openrust`；根 package 不依赖测试工具，避免依赖环。

`cargo metadata --no-deps --format-version 1` 验收根 package `openrust` 及 workspace 五个成员；default、auth/database features 与 all-features 的验证按 [testing memory](../memory/testing.md) 记录。
