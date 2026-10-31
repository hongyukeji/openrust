# OpenRust 命令行

`cli` package 提供命令 `openrust`，用于创建用户应用、生成代码并转发应用命令。无需全局安装或从 registry 获取框架包。
生成应用和仓库根应用均通过框架内置命令执行 `config:cache` 与 `config:clear`；全局 `openrust` 只转发，缓存位于应用根的 `bootstrap/cache/config.json`，生成的 `.gitignore` 排除该运行产物。
命令帮助中的说明、分组标题及帮助/版本选项均使用简体中文；命令、参数和配置字段保留原名。

## 创建和运行应用

从框架仓库根目录创建应用：

```bash
cargo run -p cli -- --help
cargo run -p cli -- new blog --db postgres --auth
cd blog
cargo run -p cli -- dev
```

`openrust new <目标目录>` 创建独立应用 workspace，支持相对和绝对路径；本地 framework/tool 依赖在生成 manifest 中明确解析，源框架 workspace 不被修改。已有目标、非法目录名、冲突 package 名在写入前拒绝。应用 `src/main.rs` 是薄入口，装配在 `bootstrap/app.rs`，通用路由在 `routes/web.rs`，应用装配在 `app/`，业务模块在 `modules/<业务>/`，资源在 `resources/`，迁移与种子在 `database/`。

生成项目默认启用可选 `view` feature、注册 `ViewServiceProvider`，并以 `resources/views/index.html` 渲染 `/`。应用级 HTML 页面模板放在 `resources/views/`，CSS、图片等静态资源放在 `public/`；`root_target` 可切换为静态文件、应用内转发或重定向。需要纯 API 应用时，可在生成后的 `Cargo.toml` 移除 `view` feature 和 Provider 注册，再将 `root_target` 设为其他模式。

不带 `--db` 时只生成 HTTP 应用；`--db postgres` 或 `--db mysql` 会生成对应迁移 package，并注册数据库、迁移和 seed Provider。`--auth` 需要 `--db`，会启用 JWT、AuthFlows、Policy 与共享限流 migration，并生成注册、邮箱验证、登录、refresh、登出和密码重置路由。`--auth-mfa` 还要求 `--encryption`，会启用 TOTP 多因素认证，并生成两步登录、TOTP enrollment、确认、禁用和恢复码轮换路由；MFA 路由使用 JWT 认证，配置邮件、JWT 与加密密钥后须先运行 `openrust migrate`。`--http-client`、`--sms-twilio`、`--encryption` 可按需启用出站 HTTP、Twilio 短信和进程内加密。

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

## 接入自己的业务模块

新项目包含空的编译清单 `modules/mod.rs` 与内容为 `{}` 的 `modules_statuses.json`。`src/main.rs` 与生成的集成测试声明同一编译入口，`bootstrap/app.rs` 的 `Application::modules` 将运行根传入 `manifest(root)`。默认不复制框架仓库的综合示例，也没有第二个容器或应用入口。认证 TCP 测试同样通过 `ProviderBuilder::for_application` 使用生产 Provider/模块清单。

例如在生成应用创建 `modules/orders/mod.rs`，仅使用公开 API：

```rust
use openrust::core::{BootContext, BoxFuture, ModuleDescriptor, Result, ServiceProvider};
use openrust::http::{Json, Routes};

struct OrdersServiceProvider;
impl ServiceProvider for OrdersServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Routes| routes.get("/api/orders", index))?;
            Ok(())
        })
    }
}
pub fn descriptor(root: &std::path::Path) -> ModuleDescriptor {
    ModuleDescriptor::new(
        "orders",
        root.join("modules/orders"),
        0,
    ).provider(OrdersServiceProvider)
}
async fn index() -> Json<std::collections::BTreeMap<&'static str, &'static str>> {
    Json(std::collections::BTreeMap::from([("module", "orders")]))
}
```

在应用的 `modules/mod.rs` 声明已编译模块，并保留默认文件激活源：

```rust
pub mod orders;
use openrust::core::ModuleManifest;
pub fn manifest(root: &std::path::Path) -> ModuleManifest {
    ModuleManifest::from_file(root.join("modules_statuses.json"))
        .module(orders::descriptor(root))
}
```

在应用根 `modules_statuses.json` 中启用它：

```json
{"orders": true}
```

若 `orders` 必须使用另一个业务模块提供的服务，可在应用清单声明依赖：

```rust
ModuleManifest::from_file(root.join("modules_statuses.json"))
    .module(orders::descriptor(root).requires("shared"))
    .module(shared::descriptor(root))
```

同时在 `modules_statuses.json` 中启用 `orders` 和 `shared`。启用的模块依赖缺少编译声明、未启用或构成循环时启动失败；框架不自动编辑启用状态。依赖图优先于模块 `priority`，同一批就绪模块仍按 priority 和清单顺序稳定排列。

源码声明、注册清单与状态文件属于应用配置，无需修改框架 `src/` 或依赖清单。默认状态文件和模块资源路径相对于运行工作目录，与默认 `config/` 加载规则一致。请从部署应用根启动；`cargo --manifest-path` 本身不会替应用切换运行工作目录，从其他目录执行时不能假定它读取项目根。TestApp 的显式 Figment 不会改变这一资源根约定，也不修改全局 cwd/env。生成应用已有 `Configuration.modules_root` 可设置绝对运行根；`Application::modules(config)` 将其同时传入状态文件与所有模块 descriptor；`APP_CONFIG_DIR` 只覆盖配置目录，不会自动替模块选择资源根。缺少文件或键表示关闭；`false`、字符串 `"true"`、数字等非 JSON `true` 值也关闭。损坏 JSON 会使启动失败，框架不会覆盖原文件；可读状态变更在下次应用构建才生效，当前实例不热卸载。此处没有新增状态管理写命令。

启动后请求 `/api/orders`；默认语义状态成功 envelope 的 `data.module` 为 `"orders"`。在生成的普通应用 `tests/test_app.rs` 中，可沿用已有 `TestApp<bootstrap::应用类型>` 和显式测试来源，对该路径调用 `request` 后 `assert_json`，再显式 `shutdown`。`TestAppBuilder::remove_module("orders")` 只移除本次测试应用的模块；默认缺失路由返回 HTTP 404 和正文 `code="not_found"`；只有显式设置 `always_ok` 才返回 HTTP 200。

```bash
cargo check
cargo test
cargo run
# 另一终端请求；这一步才提供 TCP 证据。
curl --noproxy '*' http://127.0.0.1:3000/api/orders
```

模块根必须在部署目录真实存在；默认相对路径在启动时解析，也可显式传入配置的绝对运行根，不能绑定编译机源码位置；编译声明与运行时启用分别维护。禁用影响新应用启动，不热卸载当前应用。模块私有资源、配置、迁移与测试留在该模块内部，通过公开 Provider 接口接入。普通模块共享当前应用 Cargo 依赖解析；第三方业务包仍在应用清单声明，Provider 不隔离依赖版本或 feature。这是手工业务模块接入，未提供 R13 的完整模块生成、发布和管理命令。

## 生成代码

`generate model` 在 `app/models/_entities/` 生成 SeaORM entity；`generate migration` 在 `database/migrations/` 生成迁移并写入 registry。字段支持 `string`、`text`、`integer`、`big_integer`、`boolean`、`date_time`、`json` 和 `references`，在字段类型后加 `?` 表示可空。model 使用单数名称，表名采用简单英语复数；migration 接受表名。被引用表须在外键迁移前创建。

两个数据库生成命令均支持 `--uuid-id` 与 `--soft-delete`，model 与对应迁移须使用相同选项。UUID 使用 UUIDv7，并同时为应用与 migration package 启用 `db-uuid`；软删除增加可空 UTC `deleted_at`，只提供显式 `Entity::not_deleted()` scope，不改变普通 `Entity::find()`。

`generate mailer <name>` 生成实现 `openrust::mail::Email` 的邮件类型；`generate policy <resource>` 生成基于 `openrust::auth::Policy` 的资源授权包装，权限名为 `<resource>:read|create|update|delete`。`generate job <name>` 需要数据库应用，会启用队列 feature、登记 `QueueMigration`，生成显式失败的 handler 骨架，并把任务注册到应用队列 Provider；请先实现 handler，再启动 worker。上述生成器会登记模块且不覆盖已有文件；policy 需要通过 `--auth` 创建项目。

## 配置与验收

生成项目采用按领域命名的配置文件，例如 `config/app.toml`、`config/cache.toml`、`config/database.toml`；Rust 配置类型和默认值位于框架 `src/config/`。`.env.example` 按领域列出常用值：数据库使用 `DB_CONNECTION=postgres|mysql` 及 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME`、`DB_PASSWORD`、`DB_TABLE_PREFIX`；Redis 连接使用 `REDIS_HOST`、`REDIS_USERNAME`、`REDIS_PASSWORD`、`REDIS_PORT`、`REDIS_DB`；缓存选择使用 `CACHE_STORE`，映射到 `cache.default`，缺省为已登记的具名 `redis` store；队列使用 `QUEUE_DRIVER`；邮件、短信和国际化分别使用 `MAIL_*`、`SMS_*` 和 `I18N_DEFAULT_LOCALE`。缓存和 Redis 队列共用同一组 Redis 连接设置。监听端口由中性 `SERVER_PORT=3000` 映射到 `server.port`，`APP_URL` 可使用 `${SERVER_PORT}` 插值；应用字段仍使用 `APP_LOG_LEVEL`、`APP_NAME` 等命名空间变量。`APP_PORT`、独立 `PORT`、`REDIS_DEFAULT_STORE` 与 `CACHE_DEFAULT_STORE` 不再控制监听或缓存选择。复制 `.env.example` 为 `.env` 以配置本地或部署环境；不要提交含有密钥的 `.env`。生产环境可使用权限受控的 `.env`，进程环境变量优先；应用根目录不得作为静态公开目录。`openrust key generate` 输出一个 32 字节 Base64 密钥，不写入配置文件；请将输出保存到受保护的 secret store 或被忽略的 `.env`。加密只处理应用数据，不会加密数据库备份或 JWT。

未选择数据库、加密或 gRPC 的普通项目会生成 `tests/test_app.rs`：通过与生产同一 Provider 清单和 Router 的 `TestApp` 验证 `/api`、`/health`，使用显式内存 cache 与 view 目录，不要求 Redis 服务。数据库项目需真实数据库配置；加密项目需显式密钥；gRPC 项目生成自己的 `tests/grpc_flows.rs`，启动生成二进制的 `serve all`，用临时 TLS 证书验证真实 HTTP、Echo RPC、证书拒绝和信号停机。`TestApp` 公共 API 仍可用于这些应用，测试须自行提供相应配置。启用认证的应用会另生成 `tests/auth_flows.rs`，其中既有真实 TCP HTTP 集成，也有隔离数据库和 fake 邮件驱动的 `TestApp` 注册请求；启用 `--auth-mfa` 时还验证生成路由的 MFA 挑战与恢复码。运行数据库测试前，设置 `OPENRUST_TEST_DATABASE_URL` 为可创建和删除测试库的 PostgreSQL 或 MySQL 管理连接；变量缺失时测试会跳过数据库路径，不能视为真实数据库验收通过。新生成项目应执行 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；涉及数据库的测试还须在相应真实服务上实际运行。

应用默认安装 `I18nServiceProvider`。`config/i18n.toml` 中的 `i18n.default_locale` 与 `i18n.fallback_locale` 设定默认语言和回退；请求的 `Accept-Language` 选择受支持的语言。应用可通过 `MessageCatalog` 扩展 JSON 消息资源。

## API、worker 与 scheduler

参见 [`modules/example/`](../../modules/example/README.md)：同一应用可分别作为 API、worker、scheduler 进程启动，共用数据库和队列迁移。`serve` 不会隐式启动后台进程。两个数据库后端使用相同示例，只需选择 `db-postgres` 或 `db-mysql` feature。

生成应用的 `tests/support/request_input.rs` 经生产 Provider 清单追加受管理 `Routes::route(path, http::routing::get(...).post(...))`，验证真实路由名称、`route_is`、最终 URI/方法和缺参数默认值。直接使用 Axum `Router` / `MethodRouter` 属于原生逃生口，不具有上述受管理声明的匹配上下文保证。显式 `Routes::fallback` 按 Laravel 仅接受 GET/HEAD；POST 等方法没有对应业务路由时返回方法不允许；OPTIONS 存在可用方法时合成空响应与 Allow。

### 生成项目的消息包参考

普通生成应用的 `tests/support/message_bags.rs` 通过测试业务Provider和TestApp演示公开MessageBag/ViewErrorBag、递归合并及JSON选项；`tests/test_app.rs` 调用此入口。它只注册测试路由，不增加默认生产接口。数据库认证模板使用独立的 `tests/auth_flows.rs`，不自动运行此helper；业务可参考唯一 `modules/example` 中的生产模块调用。

普通生成应用的 `tests/support/request_redirect.rs` 也只在测试 Provider 挂载业务路由：`Redirector::from_info` 生成普通响应，`from_request` 闪存排除敏感字段后的输入与命名错误包；公开 `ErrorBagCodecs` 扩展恢复自定义错误包，再用两个真实 Cookie 请求验证读取与老化。该测试不添加默认生产路由；数据库认证模板仍按自身测试入口验收。
