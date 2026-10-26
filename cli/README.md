# OpenRust 命令行

`cli` package 提供命令 `openrust`，用于创建源码 workspace 内的应用、生成代码并转发应用命令。无需全局安装或从 registry 获取框架包。

## 创建和运行应用

从框架仓库根目录创建应用：

```bash
cargo run -p cli -- --help
cargo run -p cli -- new blog --db postgres --auth
cd apps/blog
cargo run -p cli -- dev
```

`openrust new <name>` 将应用生成到 `apps/<name>/`，把应用和可选 migration package 追加到现有 workspace，并使用本地 workspace 依赖。也可传 `apps/<name>`；绝对路径、外部目录和嵌套 workspace 不受支持。重复目录、应用 package 名或 migration package 名会在写入前报错。

不带 `--db` 时只生成 HTTP 应用；`--db postgres` 或 `--db mysql` 会生成对应迁移 package，并注册数据库、迁移和 seed 插件。`--auth` 需要 `--db`，会启用 JWT、AuthFlows、Policy 与共享限流 migration，并生成注册、邮箱验证、登录、refresh、登出和密码重置路由。`--http-client`、`--sms-twilio`、`--encryption` 可按需启用出站 HTTP、Twilio 短信和进程内加密。

从应用目录运行后续命令。CLI 会根据 Cargo metadata 选中该目录对应的应用 package，并以应用目录作为子进程工作目录，因而读取该应用自己的 `config/`：

```bash
cargo run -p cli -- key generate
cargo run -p cli -- generate model post title:string published_at:date_time?
cargo run -p cli -- generate migration comments body:text post:references?
cargo run -p cli -- generate model post --uuid-id --soft-delete title:string
cargo run -p cli -- generate migration posts --uuid-id --soft-delete title:string
cargo run -p cli -- g migration labels slug:string
```

`dev` 将 `APP_ENV` 设为 `development` 后运行该应用的 `serve`；`serve` 保留当前环境。其他命令转发给所选应用并保留退出码。当前 dev/serve 只负责启动，不提供文件监听、依赖服务编排、就绪等待或自动迁移。

## 生成代码

`generate model` 在 `src/models/_entities/` 生成 SeaORM entity；`generate migration` 在 `migration/` 生成迁移并写入 registry。字段支持 `string`、`text`、`integer`、`big_integer`、`boolean`、`date_time`、`json` 和 `references`，在字段类型后加 `?` 表示可空。model 使用单数名称，表名采用简单英语复数；migration 接受表名。被引用表须在外键迁移前创建。

两个数据库生成命令均支持 `--uuid-id` 与 `--soft-delete`，model 与对应迁移须使用相同选项。UUID 使用 UUIDv7，并同时为应用与 migration package 启用 `db-uuid`；软删除增加可空 UTC `deleted_at`，只提供显式 `Entity::not_deleted()` scope，不改变普通 `Entity::find()`。

`generate mailer <name>` 生成实现 `openrust::mail::Email` 的纯文本邮件类型；`generate policy <resource>` 生成基于 `openrust::auth::Policy` 的资源授权包装，权限名为 `<resource>:read|create|update|delete`。mailer 与 policy 会登记到各自模块，禁止覆盖已有文件；policy 需要通过 `--auth` 创建项目。

## 配置与验收

复制 `.env.example` 为 `.env` 以进行本地覆盖；不要提交含有密钥的 `.env`。进程环境变量优先，生产环境不加载 `.env`。`openrust key generate` 输出一个 32 字节 Base64 密钥，不写入配置文件；请将输出保存到受保护的 secret store 或被忽略的本地 `.env`。加密只处理应用数据，不会加密数据库备份或 JWT。

启用认证的应用包含真实 HTTP 集成测试 `tests/auth_flows.rs`，使用隔离数据库和 fake 邮件驱动验证端点。运行数据库测试前，设置 `OPENRUST_TEST_DATABASE_URL` 为可创建和删除测试库的 PostgreSQL 或 MySQL 管理连接；变量缺失时测试会跳过数据库路径，不能视为真实数据库验收通过。新生成项目应执行 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；涉及数据库的测试还须在相应真实服务上实际运行。

应用默认安装 `I18nPlugin`。`config/default.toml` 中的 `i18n.default_locale` 与 `i18n.fallback_locale` 设定默认语言和回退；请求的 `Accept-Language` 选择受支持的语言。应用可通过 `MessageCatalog` 扩展 JSON 消息资源。
