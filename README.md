# OpenRust

> An opinionated, batteries-included Rust application framework. Create a project, run one command, and start writing business code.

**状态：v1 开发中。** OpenRust 以完整源码 workspace 交付，业务应用位于 `apps/`；框架不单独发布 Cargo package。根框架、内部工具与集成宿主使用中性包名，并显式设置 `publish = false`。v1 覆盖配置、HTTP 约定、PostgreSQL/MySQL、认证与授权、队列、调度、事件、缓存、存储、邮件、CLI、测试工具和部署支持；规划中的能力须有实现和验收证据后才算交付。

## Goals

- **Convention over configuration, without magic.** Directory layout, naming, routing, migrations and configuration keys follow visible conventions. Dependencies are explicit Rust types; there is no runtime container, no reflection, no facade.
- **One obvious way.** Each capability has one official entry point: `Validated<T>` for input, `Policy` for authorization, `Job` for background work.
- **Batteries included, drivers swappable.** The v1 plan uses a database queue, Redis cache, local storage and SMTP mail and pluggable SMS (Twilio adapter), with memory/file cache alternatives and optional S3-compatible storage. Both single-instance and clustered application deployments are planned; drivers are selected through Cargo features and configuration.
- **Generated code is ordinary code.** Everything `openrust generate` produces is readable, editable and testable Rust.
- **Testable by default.** Capabilities provide fake drivers, while the Mailpit Compose service supports real local SMTP verification; business tests do not require paid SMTP/SMS accounts or object storage.

OpenRust is built on `tokio`, `axum`, `tower` and `SeaORM`, with explicit Rust types and conventional project structure.

## 在仓库中运行

集成宿主 [server](apps/server/README.md) 提供 HTTP、请求语言与本地日志邮件/短信驱动，不依赖外部服务。请在应用目录运行命令，让配置从 `apps/server/config` 加载。数据库、认证、队列和调度集成由显式组装的测试应用验证；启用 feature 不会自动安装这些服务：

```bash
cd apps/server
cargo run -p server
curl --noproxy '*' http://127.0.0.1:3000/health
cargo run -p server -- config show
cargo run -p server -- help
cargo test -p server
```

应用默认从当前工作目录的 `config/` 读取配置。`APP_SERVER__PORT=4000 cargo run -p server` 或 `PORT=4000 cargo run -p server` 可覆盖监听端口；若两者同时设置，`APP_SERVER__PORT` 优先。应用可通过 `Application::config_namespace()` 选择如 `SHOP_` 的变量前缀。框架代码位于仓库根 package，能力模块位于 `src/audit.rs`、`src/config/`、`src/core/`、`src/db/`、`src/http/`、`src/i18n.rs`、`src/mail/`、`src/sms.rs`，以及按 feature 启用的 auth、encryption、signing、queue 和 schedule。每个应用的 TOML/YAML 配置保存在其根目录的 `config/` 中；`apps/server` 是本 workspace 的集成宿主。
Application data encryption is an opt-in `encryption` feature. Register `openrust::security::EncryptionPlugin`, configure a 32-byte key ring under `[security.encryption]`, and generate local key material with `openrust key generate`. The active key encrypts new values; prior keys decrypt old values during a controlled rotation. Keep keys outside version control.

Purpose-bound expiring links are available through the `signing` feature and `openrust::signing::SignedLinks`. They use a separate key ring under `[security.signed_links]`; add `SignedLinkRevocationMigration` and a shared database store when individual links need cross-instance revocation. See [the signed-links guide](src/signing/README.md).

Explicit audit records use `openrust::audit::Audit` with the database sink or a custom `AuditSink`. Add `DatabaseAuditMigration` to the application migrator. Details are denied by default, require an allowlist, and redact sensitive field names. Critical database changes can insert a validated audit record in the same transaction; see the [audit module guide](src/audit/README.md).

The `queue` feature provides durable database job dispatch, bounded worker concurrency, and database-coordinated scheduling for PostgreSQL and MySQL. Register `QueueMigration` in the application migrator and install `QueuePlugin` after `DatabasePlugin`; handlers are explicitly registered with `register_job`. Use `schedule_interval` or `schedule_daily_at` on the same plugin to register a UTC schedule. `schedule run --once` scans once, `schedule run` runs continuously, and `schedule list` shows registered entries. `Queue::dispatch_unique` prevents overlapping duplicates for a queue-scoped key and returns the active job ID; the key is released when the job completes or is moved to `failed_jobs`. `dispatch_chain` executes registered tasks in order, while `dispatch_batch` bounds concurrency per group. Each group is one durable parent job; a retry can repeat successful child side effects, so child handlers must be idempotent. Groups allow at most 128 tasks and a batch concurrency of 1 to 64. Dispatch inside a `Db` transaction when job creation must commit or roll back with business data. Execution is at least once, so handlers must be idempotent. The current worker supports retries, failed-job retention, manual retry, progress, cooperative cancellation, per-attempt timeouts and lease recovery after process failure. Schedules support fixed whole-second intervals and daily UTC time; cron expressions, named time zones and DST are outside the first v1 scheduling contract. Events support ordered in-process listeners and explicit queue-backed listeners; full cluster acceptance remains pending.

## Typed events

`EventsPlugin` registers application-owned event listeners in declaration order. `Events::dispatch` awaits matching listeners sequentially and returns the first error without calling later listeners. This dispatch is process-local and is not saved or broadcast. With the `queue` feature, `listen_queued` maps an event to a registered `Job`; call `dispatch_in_transaction` inside the business transaction to atomically persist that job with the business change. Workers remain explicit, and their handlers must tolerate at-least-once retries.

```rust,ignore
let mut events = openrust::events::EventsPlugin::new();
events.listen_queued::<UserRegistered, WelcomeEmailJob, _>("mail", |event| {
    Ok(WelcomeEmailJob { user_id: event.user_id })
});
app_builder.plugin(queue_plugin).plugin(events); // QueuePlugin is registered first.
let app = openrust::core::lifecycle::finalize(app_builder).await?;
let events = app.state().try_get::<openrust::events::Events>()?;

db.transaction(|transaction| {
    let events = std::sync::Arc::clone(&events);
    Box::pin(async move {
        create_user(transaction).await?;
        events
            .dispatch_in_transaction(transaction, &UserRegistered { user_id: 42 })
            .await?;
        Ok(())
    })
})
.await?;
```

Register `WelcomeEmailJob` with `QueuePlugin` as well. For best-effort in-process observation after a committed transaction, call `Events::dispatch` only after the transaction returns successfully; a listener error cannot undo that commit. Never put passwords, tokens, verification codes or raw requests in event/job payloads.

## v1 源码应用使用路径

可以在仓库根目录生成应用；生成器会把应用及可选 migration package 加入 workspace，并使用本地依赖：

```bash
git clone https://github.com/hongyukeji/openrust.git
cd openrust
cargo run -p cli -- new blog --db postgres
cd apps/blog
cargo run -p cli -- dev
```

生成的应用应直接依赖仓库内框架源码，通过 `cargo check` 和 `cargo test`，无需发布内部 package 或添加手工 registry patch。依赖编排、自动迁移与 OpenAPI 等保证仍按 v1 计划逐项实现和验收。
## Request languages

Generated applications install `I18nPlugin` by default. `[i18n] default_locale` and `fallback_locale` select the default language and final resource fallback; `Accept-Language` selects a supported request locale. The framework ships English and Simplified Chinese HTTP messages. Applications can merge JSON resources into `MessageCatalog`, use `RequestLocale` in handlers, or call `I18n::with_locale` for an explicit task scope. Templates support named `{name}` substitution; unknown keys stay visible as their key. This layer does not provide plural rules or locale-aware date/number formatting. See the [`I18n` API](src/i18n.rs).

```rust
use openrust::http::{Inject, Json};
use openrust::i18n::{I18n, MessageArgs, RequestLocale};

async fn greeting(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "app.greeting", &args))
}
```

Load an application JSON object into `MessageCatalog` at startup, for example `{"app.greeting":"Hello, {name}!"}` under the selected locale. Use the same explicit locale when composing an email or SMS outside a request.

## Documentation

Current user guides live in module and package READMEs. [Agent plans](.agents/project/plans/README.md) describe remaining work; a separate public documentation site is planned for M6. Planning records will remain separate from user documentation.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
