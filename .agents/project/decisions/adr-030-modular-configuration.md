# ADR-030：按能力模块组织配置

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-27
- 背景：`src/config/mod.rs` 集中放置环境解析、Figment source、框架配置结构与测试，已不利于按配置域定位；数据库、认证和邮件设置又分散在各能力模块。
- 决策：框架配置定义按真实配置域拆到 `src/config/*.rs`，`mod.rs` 保留环境引导、source 合并、通用错误和公开 facade。当前具备的 app、api、auth、database、logger、mailer、server 各有配置模块；未实现能力不创建空配置文件。
- 兼容：已有 `openrust::auth::{JwtConfig, PasswordConfig, SessionConfig}`、`openrust::database::DatabaseConfig`、`openrust::mail::MailConfig` 路径通过 re-export 保留；新增集中路径为 `openrust::config::<domain>::*`。
- 运行配置：应用根 `config/` 采用 Laravel 式按领域命名的 TOML 文件，如 `app.toml`、`database.toml`、`cache.toml`、`mail.toml`；每个文件使用对应领域的顶层 section。统一加载器按文件名排序合并所有 TOML，环境差异和秘密使用 `.env` 或部署进程环境变量。旧的 `default.toml`、`development.toml`、`test.toml`、`production.toml` 不再作为加载入口；`src/config/` 只保存 Rust 配置源码。
- 环境来源：遵循 ADR-016，应用自身与完整结构化配置使用 `APP_` namespace；常用基础能力使用领域前缀：数据库 `DB_*`（含 `DB_TABLE_PREFIX`）、Redis `REDIS_*`（含 `REDIS_DB` 与 `REDIS_DEFAULT_STORE`）、队列 `QUEUE_DRIVER`、邮件 `MAIL_*`、短信 `SMS_*`、国际化 `I18N_DEFAULT_LOCALE`。缓存与 Redis 队列共用同一组 `REDIS_*` 连接设置，不另配队列连接 URL。运行时将数据库和 Redis 分项设置组装为 driver 所需连接地址；不保留 `DATABASE_URL` / `REDIS_URL` 等通用 URL 环境别名。根目录与生成应用的 `.env.example` 按领域列出配置。所有环境均可从应用工作目录加载 `.env`，进程环境变量优先；示例只使用本地开发占位值，不包含真实秘密；文件缺失时继续启动，格式错误或不可读时安全失败。
- 取舍：借鉴 Loco 按能力拆分 typed config 结构、Laravel 按领域拆分配置文件及 `.env` / 外部环境变量优先级；提供 `APP_NAME`、`APP_URL` 等常用短变量映射，并用 Laravel 熟悉的数据库与 Redis 分项变量降低配置阅读成本。OpenRust 在进程启动时一次性加载为类型化配置，不需要 PHP 请求模型下的 `config:cache`，不引入 facade、任意字符串全局 accessor、环境专属 `.env` 变体或第二套解析器。
- 验收：旧公共路径编译、按模块解构与统一 Figment 加载结果一致；覆盖环境选择、层级优先级、secret 脱敏、production `.env` 加载、进程环境覆盖、无效 `.env` 安全失败，并对默认及 `--db/--auth` 生成模板运行测试。配置功能簇的实际命令结果记录于提交说明和 `memory/verification.md`。
