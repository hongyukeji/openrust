# OpenRust 命令行

`cli` package 提供命令 `openrust`，用于创建源码 workspace 内的应用、生成代码并转发应用命令。无需全局安装或从 registry 获取框架包。
命令帮助中的说明、分组标题及帮助/版本选项均使用简体中文；命令、参数和配置字段保留原名。

## 创建和运行应用

从框架仓库根目录创建应用：

```bash
cargo run -p cli -- --help
cargo run -p cli -- new blog --db postgres --auth
cd blog
cargo run -p cli -- dev
```

`openrust new <目标目录>` 创建独立应用 workspace，支持相对和绝对路径；本地 framework/tool 依赖在生成 manifest 中明确解析，源框架 workspace 不被修改。已有目标、非法目录名、冲突 package 名在写入前拒绝。应用 `src/main.rs` 是薄入口，装配在 `bootstrap/app.rs`，路由在 `routes/web.rs`，业务在 `app/`，资源在 `resources/`，迁移与种子在 `database/`。

生成项目默认启用可选 `view` feature、注册 `ViewPlugin`，并以 `views/index.html` 渲染 `/`。HTML 页面模板放在 `views/`，CSS、图片等静态资源放在 `public/`；`root_target` 可切换为静态文件、应用内转发或重定向。需要纯 API 应用时，可在生成后的 `Cargo.toml` 移除 `view` feature 和插件注册，再将 `root_target` 设为其他模式。

不带 `--db` 时只生成 HTTP 应用；`--db postgres` 或 `--db mysql` 会生成对应迁移 package，并注册数据库、迁移和 seed 插件。`--auth` 需要 `--db`，会启用 JWT、AuthFlows、Policy 与共享限流 migration，并生成注册、邮箱验证、登录、refresh、登出和密码重置路由。`--auth-mfa` 还要求 `--encryption`，会启用 TOTP 多因素认证，并生成两步登录、TOTP enrollment、确认、禁用和恢复码轮换路由；MFA 路由使用 JWT 认证，配置邮件、JWT 与加密密钥后须先运行 `openrust migrate`。`--http-client`、`--sms-twilio`、`--encryption` 可按需启用出站 HTTP、Twilio 短信和进程内加密。

启用 MFA 的应用中，`POST /auth/login` 对未启用 MFA 的账户返回 `status: "authenticated"` 和 `credentials`；对已启用 MFA 的账户只返回 `status: "mfa_required"`、短时 `challenge` 与 `expires_at`。客户端把 challenge 与 TOTP 或恢复码提交到 `POST /auth/mfa/login`，成功后才会收到 access/refresh 凭据。已登录用户使用 JWT 调用 `POST /auth/mfa/enrollment`（邮箱与密码重新认证）获取一次性 provisioning URI，再以 `POST /auth/mfa/enrollment/confirm` 确认 TOTP 并领取一次性恢复码。`POST /auth/mfa/disable` 和 `POST /auth/mfa/recovery-codes` 同样要求 JWT、密码及当前 TOTP/恢复码；成功后撤销 refresh token。无状态 access JWT 仍会在其配置 TTL 到期前有效。

从应用目录运行后续命令；从 workspace 根执行时选择 Cargo metadata 中唯一的默认应用成员。CLI 会选中对应应用 package，并以应用目录作为子进程工作目录，因而读取该应用自己的 `config/`：

```bash
cargo run -p cli -- key generate
cargo run -p cli -- generate model post title:string published_at:date_time?
cargo run -p cli -- generate migration comments body:text post:references?
cargo run -p cli -- generate model post --uuid-id --soft-delete title:string
cargo run -p cli -- generate migration posts --uuid-id --soft-delete title:string
cargo run -p cli -- g migration labels slug:string
```

`dev` 将 `APP_ENV` 设为 `development` 后运行该应用的 `serve`；`serve` 保留当前环境。若默认缓存 store 配置为 loopback Redis，`dev` 会先探测服务：已有服务可用时直接复用，不可用时通过仓库 `compose.ci.yaml` 启动 Redis 并保留容器供后续本地开发复用。远端 Redis 由应用直接连接。其他命令转发给所选应用并保留退出码；当前不提供文件监听、就绪等待或自动迁移。

## 生成代码

`generate model` 在 `app/models/_entities/` 生成 SeaORM entity；`generate migration` 在 `database/migrations/` 生成迁移并写入 registry。字段支持 `string`、`text`、`integer`、`big_integer`、`boolean`、`date_time`、`json` 和 `references`，在字段类型后加 `?` 表示可空。model 使用单数名称，表名采用简单英语复数；migration 接受表名。被引用表须在外键迁移前创建。

两个数据库生成命令均支持 `--uuid-id` 与 `--soft-delete`，model 与对应迁移须使用相同选项。UUID 使用 UUIDv7，并同时为应用与 migration package 启用 `db-uuid`；软删除增加可空 UTC `deleted_at`，只提供显式 `Entity::not_deleted()` scope，不改变普通 `Entity::find()`。

`generate mailer <name>` 生成实现 `openrust::mail::Email` 的邮件类型；`generate policy <resource>` 生成基于 `openrust::auth::Policy` 的资源授权包装，权限名为 `<resource>:read|create|update|delete`。`generate job <name>` 需要数据库应用，会启用队列 feature、登记 `QueueMigration`，生成显式失败的 handler 骨架，并把任务注册到应用队列插件；请先实现 handler，再启动 worker。上述生成器会登记模块且不覆盖已有文件；policy 需要通过 `--auth` 创建项目。

## 配置与验收

生成项目采用按领域命名的配置文件，例如 `config/app.toml`、`config/cache.toml`、`config/database.toml`；Rust 配置类型和默认值位于框架 `src/config/`。`.env.example` 按领域列出常用值：数据库使用 `DB_CONNECTION=postgres|mysql` 及 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME`、`DB_PASSWORD`、`DB_TABLE_PREFIX`；Redis 使用 `REDIS_HOST`、`REDIS_USERNAME`、`REDIS_PASSWORD`、`REDIS_PORT`、`REDIS_DB`、`REDIS_DEFAULT_STORE`；队列使用 `QUEUE_DRIVER`；邮件、短信和国际化分别使用 `MAIL_*`、`SMS_*` 和 `I18N_DEFAULT_LOCALE`。缓存和 Redis 队列共用同一组 Redis 连接设置。应用配置采用单下划线的扁平环境变量，如 `APP_SERVER_PORT=4000`、`APP_LOG_LEVEL=info`、`APP_NAME` 和 `APP_URL`，由框架映射到相应配置字段。复制 `.env.example` 为 `.env` 以配置本地或部署环境；不要提交含有密钥的 `.env`。生产环境可使用权限受控的 `.env`，进程环境变量优先；应用根目录不得作为静态公开目录。`openrust key generate` 输出一个 32 字节 Base64 密钥，不写入配置文件；请将输出保存到受保护的 secret store 或被忽略的 `.env`。加密只处理应用数据，不会加密数据库备份或 JWT。

启用认证的应用包含真实 HTTP 集成测试 `tests/auth_flows.rs`，使用隔离数据库和 fake 邮件驱动验证端点。运行数据库测试前，设置 `OPENRUST_TEST_DATABASE_URL` 为可创建和删除测试库的 PostgreSQL 或 MySQL 管理连接；变量缺失时测试会跳过数据库路径，不能视为真实数据库验收通过。新生成项目应执行 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；涉及数据库的测试还须在相应真实服务上实际运行。

应用默认安装 `I18nPlugin`。`config/i18n.toml` 中的 `i18n.default_locale` 与 `i18n.fallback_locale` 设定默认语言和回退；请求的 `Accept-Language` 选择受支持的语言。应用可通过 `MessageCatalog` 扩展 JSON 消息资源。

## API、worker 与 scheduler

参见 [`app/example/`](../../app/example/README.md)：同一应用可分别作为 API、worker、scheduler 进程启动，共用数据库和队列迁移。`serve` 不会隐式启动后台进程。两个数据库后端使用相同示例，只需选择 `db-postgres` 或 `db-mysql` feature。
