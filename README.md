# OpenRust

> 面向约定优于配置、内置常用能力的 Rust 应用框架。创建项目并运行一条命令，即可开始编写业务代码。

**状态：v1 开发中。** OpenRust 以完整源码 workspace 交付，业务应用位于 `apps/`；框架不单独发布 Cargo package。根框架、内部工具与集成宿主使用中性包名，并显式设置 `publish = false`。v1 覆盖配置、HTTP 约定、PostgreSQL/MySQL、认证与授权、队列、调度、事件、缓存、存储、邮件、CLI、测试工具和部署支持；规划中的能力须有实现和验收证据后才算交付。

## 设计目标

- **约定清晰，不依赖魔法。** 目录、命名、路由、迁移和配置键遵循可见约定。依赖通过明确的 Rust 类型表达，不使用运行时容器、反射或门面式动态注入。
- **每项能力都有明确入口。** 例如，输入校验使用 `Validated<T>`，授权使用 `Policy`，后台任务使用 `Job`。
- **常用能力开箱可用，驱动可替换。** v1 计划提供数据库队列、Redis 缓存、本地存储、SMTP 邮件和可扩展短信（含 Twilio 适配器），并支持显式选择 memory/file 缓存及可选的 S3 兼容存储。框架计划支持单机和集群部署；驱动通过 Cargo feature 与配置选择。
- **生成代码就是普通代码。** `openrust generate` 生成的内容均为可读、可编辑、可测试的 Rust 代码。
- **默认可测试。** 各能力提供 fake driver；Compose 中的 Mailpit 服务支持真实本地 SMTP 验证。业务测试无需付费 SMTP/SMS 账号或对象存储服务。

OpenRust 基于 `tokio`、`axum`、`tower` 和 `SeaORM` 构建，采用明确的 Rust 类型与常规项目结构。

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
应用数据加密通过可选的 `encryption` feature 启用。注册 `openrust::security::EncryptionPlugin`，在 `[security.encryption]` 下配置 32 字节密钥环，并使用 `openrust key generate` 生成本地密钥材料。当前密钥用于加密新值；受控轮换期间，历史密钥用于解密旧值。密钥应保存在版本控制之外。

`signing` feature 和 `openrust::signing::SignedLinks` 提供绑定用途并会过期的链接。它们使用 `[security.signed_links]` 下独立的密钥环；若需跨实例撤销单条链接，请添加 `SignedLinkRevocationMigration` 并配置共享数据库存储。详见[签名链接指南](src/signing/README.md)。

显式审计记录通过 `openrust::audit::Audit` 与数据库 sink 或自定义 `AuditSink` 写入。请将 `DatabaseAuditMigration` 加入应用迁移器。默认不接收详情；如需记录，必须配置允许列表，敏感字段名会被脱敏。关键数据库变更可在同一事务中写入经过校验的审计记录，详见[审计模块指南](src/audit/README.md)。

`queue` feature 为 PostgreSQL 和 MySQL 提供持久化数据库任务派发、有界 worker 并发和由数据库协调的调度。将 `QueueMigration` 注册到应用迁移器，并在 `DatabasePlugin` 后安装 `QueuePlugin`；使用 `register_job` 显式注册处理器。可在同一插件上通过 `schedule_interval` 或 `schedule_daily_at` 注册 UTC 调度。`schedule run --once` 扫描一次，`schedule run` 持续运行，`schedule list` 显示已注册项。`Queue::dispatch_unique` 根据队列范围内的键防止任务重叠重复，并返回活动任务 ID；任务完成或移入 `failed_jobs` 后释放该键。`dispatch_chain` 按顺序执行已注册任务，`dispatch_batch` 限制每组任务的并发数。每组是一个持久化父任务；重试可能再次执行已成功子任务的副作用，因此子任务处理器必须保持幂等。每组最多 128 项，批次并发数为 1 到 64。若任务创建需与业务数据共同提交或回滚，请在 `Db` 事务中派发。任务至少执行一次，因此处理器必须保持幂等。当前 worker 支持重试、失败任务保留与手动重试、进度、协作取消、单次尝试超时，以及进程故障后的租约恢复。调度支持固定整秒间隔和每日 UTC 时间；cron 表达式、命名时区和夏令时（DST）不在首版 v1 调度契约范围内。事件支持有序进程内监听器和显式队列监听器；完整集群验收仍待完成。

## 类型化事件

`EventsPlugin` 按声明顺序注册应用事件监听器。`Events::dispatch` 按顺序等待匹配的监听器；遇到首个错误时立即返回，不再调用后续监听器。该派发仅在当前进程内执行，不会持久化或广播。启用 `queue` feature 后，`listen_queued` 可将事件映射为已注册的 `Job`；在业务事务中调用 `dispatch_in_transaction`，可将任务与业务变更原子地持久化。worker 仍需显式启动，任务处理器必须能够应对至少一次重试。

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

同时还需使用 `QueuePlugin` 注册 `WelcomeEmailJob`。如需在事务提交后尽力执行进程内观察，请仅在事务成功返回后调用 `Events::dispatch`；监听器错误无法撤销已提交的事务。不得将密码、token、验证码或原始请求放入事件/任务载荷。

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
## 请求语言

生成的应用默认安装 `I18nPlugin`。`[i18n] default_locale` 和 `fallback_locale` 用于选择默认语言和最终资源回退；`Accept-Language` 用于选择受支持的请求语言。框架内置英文和简体中文 HTTP 消息。应用可将 JSON 资源合并至 `MessageCatalog`，在处理器中使用 `RequestLocale`，或调用 `I18n::with_locale` 显式设置任务作用域。模板支持命名占位符 `{name}`；未知键会原样显示为键名。此层不提供复数规则或本地化日期/数字格式化。详见 [`I18n` API](src/i18n.rs)。

```rust
use openrust::http::{Inject, Json};
use openrust::i18n::{I18n, MessageArgs, RequestLocale};

async fn greeting(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "app.greeting", &args))
}
```

启动时可将应用 JSON 对象加载至 `MessageCatalog`，例如在指定语言下配置 `{"app.greeting":"Hello, {name}!"}`。在请求之外编写邮件或 SMS 时，也应显式使用同一语言设置。

## 文档

当前用户指南位于各模块和 package 的 README 中。[Agent 计划](.agents/project/plans/README.md)记录剩余工作；M6 计划建立独立的公开文档站。规划记录与用户文档分开维护。

## 许可证

您可自行选择采用 [Apache License, Version 2.0](LICENSE-APACHE) 或 [MIT license](LICENSE-MIT) 授权。

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
