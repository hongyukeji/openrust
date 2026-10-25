# OpenRust 框架设计

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 第一代框架底座的架构设计：设计原则、Laravel 概念到 Rust 的映射、crate 与应用目录布局、内核、配置、路由、错误、数据、认证、异步能力、CLI、测试工具与框架代码规范。目标、里程碑与验收见 [gen1-plan.md](gen1-plan.md)。

## 设计原则

OpenRust 追求的优雅是 Rust 语境下的优雅：靠类型、trait 和约定减少样板，不靠运行期魔法。

1. **约定优于配置，但约定可见**：目录、命名、路由、迁移和配置键有默认约定；每个约定在生成代码或文档中可查，不藏在宏展开里。
2. **显式依赖**：服务在 `ServiceProvider` 中用 `provide` 注册，处理函数用 `Inject<T>` 声明依赖，签名即依赖清单；不提供全局 Service Locator、运行期反射容器或静态 Facade。
3. **一件事一个明显做法**：每类能力只有一个官方入口（验证只用 `Validated<T>`，授权只用 `Policy`），不保留多套并存。
4. **错误是一等公民**：框架错误统一为 `openrust::Error`，自动映射为 RFC 9457 Problem Details 响应；库代码禁止 `unwrap` / `expect` / `panic!`。
5. **宏只做派生，不做控制流**：`#[derive(...)]` 和少量属性宏用于消除样板；不用宏改变程序结构或隐藏调用关系。
6. **可替换但不过度抽象**：缓存、队列、存储、邮件各一个 trait、多个 driver；trait 只覆盖已有 driver 的共同能力，不预留假想扩展点。
7. **生成即普通代码**：CLI 生成的文件是开发者会亲手写出的代码，可读、可改、可删，无框架专属注解。
8. **可测试是默认值**：每个能力自带 fake driver 和测试工具，业务测试不需要真实 Redis、SMTP 或对象存储。
9. **最小默认，按需开启**：核心路径只依赖 tokio / axum / tower / serde / tracing；数据库驱动、Redis、S3、OpenTelemetry 等通过 Cargo feature 开启。

## Laravel 概念映射

| Laravel | OpenRust | 说明 |
| --- | --- | --- |
| Service Container / Facade | `AppState` 类型化注册表 + `Inject<T>` 提取器 + `ServiceProvider` | 处理函数用 `Inject<T>` 声明依赖，缺失注册返回 500 Problem 而非 panic；不提供 Facade |
| Service Provider | `trait ServiceProvider` | 注册服务、路由、迁移、命令、调度；框架能力与业务模块共用同一机制 |
| Artisan | `openrust` CLI + 应用二进制子命令 | `new` / `dev` / `make:*` 由全局 CLI 提供；`serve` / `migrate` / `queue:work` 等由应用二进制提供 |
| Routing | axum `Router` + `Routes` 约定 | 资源路由、路由组、前缀、中间件组、`route:list` |
| Middleware | tower `Layer` / `axum::middleware::from_fn` | 预置请求 ID、追踪、CORS、压缩、超时、限流、认证 |
| Controller | `controllers/<resource>.rs` 中的 handler 函数 | 一个资源一个文件，函数名即动作（`index` / `show` / `store` / `update` / `destroy`） |
| Form Request | `Validated<T>` 提取器 | `#[derive(Deserialize, Validate)]`，失败自动返回 422 |
| Eloquent | SeaORM Entity + ActiveModel + 业务 `impl` | 保留 ActiveRecord 式操作与类型化查询；复杂 SQL 用 SQLx 显式逃生 |
| Migration / Seeder / Factory | `sea-orm-migration` + `Seeder` trait + `Factory` trait | 时间戳前缀迁移文件，只用可移植列类型 |
| Gate / Policy | `trait Policy` | 授权逻辑集中在 `policies/`，控制器只调用 `authorize` |
| Auth Guard | `SessionGuard` / `TokenGuard` + `CurrentUser` 提取器 | Web 用 Cookie 会话，API 用个人访问令牌 |
| Queue / Job | `trait Job` + `Queue` driver | 数据库队列默认，Redis 可选；重试、退避、失败表 |
| Scheduler | `Schedule` 构建器 | cron 表达式与 `daily_at` 式方法，多副本去重 |
| Event / Listener | `trait Event` + `Listener` 注册 | 进程内分发，监听器可标记为队列化 |
| Mail | `Mailable` trait + `Mailer` driver | SMTP / log / fake |
| Storage | `Storage` trait（基于 OpenDAL） | 本地磁盘默认，S3 兼容可选 |
| Cache | `Cache` trait | 进程内（moka）默认，Redis 可选 |
| Blade | 可选 `templates` feature（minijinja） | 第一代 API 优先，模板只服务邮件；页面渲染脚手架为 0.2 候选 |
| Collection | `Iterator` / `itertools` | 不另造集合类型 |
| `.env` / `config/*.php` | `.env` + `config/*.toml` + 类型化 `Config` | 分层合并，缺失必填项启动即报错并指出键名 |
| Tinker | 不提供 | 用测试与 `db:*` 命令替代 |

## 框架仓库布局

```text
openrust/
├── Cargo.toml                 # workspace、[workspace.lints]、统一依赖版本
├── crates/
│   ├── openrust/              # 门面 crate：re-export、prelude、feature 开关
│   ├── openrust-core/         # Application、AppBuilder、ServiceProvider、Config、Error、生命周期
│   ├── openrust-http/         # Router 约定、提取器、中间件栈、Problem Details、健康检查
│   ├── openrust-db/           # 连接池、迁移运行器、Seeder、Factory、事务、方言边界
│   ├── openrust-auth/         # 用户约定、密码哈希、Guard、Policy、密码重置、邮箱验证
│   ├── openrust-queue/        # Job、Queue driver、重试、失败表、调度器
│   ├── openrust-events/       # Event / Listener
│   ├── openrust-cache/        # Cache trait、memory / redis driver
│   ├── openrust-storage/      # Storage trait、OpenDAL 适配
│   ├── openrust-mail/         # Mailable、Mailer、smtp / log / fake driver
│   ├── openrust-macros/       # 派生宏：Job、Event、Validate 辅助
│   ├── openrust-test/         # TestApp、HTTP 测试客户端、数据库隔离、fake driver 断言
│   └── openrust-cli/          # `openrust` 二进制：new / dev / make:* / 运行命令代理
├── templates/                 # `openrust new` 项目模板（按数据库后端渲染）
├── examples/blog/             # 黄金路径示例应用，CI 每次构建
├── docs/                      # mdBook 文档站
└── xtask/                     # 仓库维护任务：双库矩阵、模板快照测试、发布检查
```

crate 拆分按发布与编译边界确定。第一代允许 `events` 与调度器先作为 `core` / `queue` 的模块，出现独立发布需求再拆出；不为拆而拆。

## 生成的应用布局

```text
blog/
├── Cargo.toml
├── openrust.toml              # 项目元数据：数据库后端、模板版本（CLI 读取）
├── .env.example
├── config/
│   ├── app.toml               # 默认配置
│   ├── development.toml       # 按 APP_ENV 覆盖，可选
│   └── production.toml
├── src/
│   ├── main.rs                # 仅调用 openrust::run::<App>()
│   ├── app.rs                 # impl Application：providers、routes、migrations、schedule
│   ├── config.rs              # 业务配置结构 AppConfig
│   ├── routes.rs              # 路由表：组、前缀、中间件
│   ├── schedule.rs            # 定时任务
│   ├── controllers/           # HTTP handler，一资源一文件
│   ├── models/                # SeaORM entity（生成）+ 业务 impl
│   ├── services/              # 领域 / 应用服务，持有业务规则与事务边界
│   ├── policies/              # 授权策略
│   ├── jobs/                  # 后台任务
│   ├── events/                # 事件与监听器
│   ├── mails/                 # Mailable
│   ├── middleware/            # 业务中间件
│   ├── migrations/            # sea-orm-migration 文件
│   └── seeders/               # 种子数据与 Factory
├── resources/mail/            # 邮件模板
├── tests/                     # 集成测试，使用 openrust-test
├── compose.yaml               # 开发依赖：数据库、Mailpit、可选 Redis
└── Dockerfile                 # 多阶段构建，单二进制镜像
```

## 应用内核与依赖注入

```rust
// src/main.rs
fn main() -> std::process::ExitCode {
    openrust::run::<app::App>()
}

// src/app.rs
pub struct App;

impl Application for App {
    type Config = AppConfig;   // 无应用配置时用 NoConfig

    fn build(app: &mut AppBuilder) {
        app.provider(DatabaseProvider)
            .provider(AuthProvider)
            .provider(QueueProvider)
            .routes(routes::register)
            .migrations(migrations::all)
            .schedule(schedule::register);
    }
}
```

`routes` / `migrations` / `schedule` 是各能力 crate 通过扩展 trait 挂在 `AppBuilder` 上的方法（如 `AppBuilderHttpExt::routes`），注册内容存入 `AppBuilder::extension_mut::<T>()`，构建后由 `App::extension::<T>()` 读取。核心 crate 因此不依赖任何能力 crate。

- `openrust::run` 解析子命令：无参数或 `serve` 启动服务；`migrate` / `db:seed` / `queue:work` / `schedule:run` / `route:list` / `config:show` 由框架与各 provider 注册。应用二进制本身就是运行期 CLI，无需额外安装。
- 生命周期（`openrust_core::lifecycle::bootstrap`）：加载 `.env` → 加载配置 → 初始化 `tracing` → 提取 `Application::Config` 并注册为服务 → `Application::build` → 依次调用 provider `register`（含注册期间新增的 provider）→ 冻结为 `App` → 调用 provider `boot` → 分发命令（默认 `serve`）→ 监听并处理 `SIGTERM` / `SIGINT` 优雅停机。
- `AppState` 是按 `TypeId` 索引的只读服务注册表，业务服务通过 `app.provide(MyService::new(...))` 注册，handler 用 `Inject<MyService>` 取用；缺少注册在请求时返回 500 Problem 并写明类型名，不 panic。框架 crate 之间的硬依赖在 provider `register` 中用 `app.service::<T>()` 检查并在启动时报错。
- `ServiceProvider` 是业务模块与第三方包的唯一扩展入口：一个 crate 暴露一个 provider 即可带入路由、迁移、配置段和命令。不做动态加载。

## 配置

- 分层合并，后者覆盖前者：crate 默认值 → `config/app.toml` → `config/{APP_ENV}.toml` → `.env`（仅非生产环境自动加载）→ 环境变量。
- 环境变量映射使用双下划线：`APP__DATABASE__URL` 对应 `[database] url`。密钥只允许来自环境变量，模板文件中不出现真实值。
- 框架配置为类型化结构（`app` / `server` / `database` / `auth` / `queue` / `cache` / `storage` / `mail` / `log`），业务配置通过 `Application::Config` 关联类型接入，使用 `#[derive(Deserialize)]`。
- 启动时整体校验；缺失或类型错误立即退出，错误信息包含完整配置键与来源层。`config:show` 输出合并结果并遮蔽密钥。

## 路由与控制器

```rust
// src/routes.rs
pub fn register(routes: Routes) -> Routes {
    routes
        .group("/api", |api| {
            api.resource::<controllers::posts>("/posts")
               .middleware(auth::token())
        })
        .get("/health", health::live)
}

// src/controllers/posts.rs
pub async fn store(
    Inject(posts): Inject<PostService>,
    user: CurrentUser,
    Validated(Json(input)): Validated<Json<CreatePost>>,
) -> Result<Created<Json<PostResponse>>> {
    PostPolicy::authorize(&user, PostAction::Create, None)?;
    let post = posts.create(&user, input).await?;
    Ok(Created(Json(post.into())))
}
```

- `resource` 按约定映射 `index` / `show` / `store` / `update` / `destroy`，缺失动作不注册；`only` / `except` 裁剪。
- 预置中间件栈从外到内：请求 ID 设置 → 请求 ID 回传 → 追踪 span → 超时 → 压缩 → CORS → panic 捕获；业务中间件在路由组上追加。
- 输入验证只通过 `Validated<T>`；授权只通过 `Policy`；事务边界在 `services/`，控制器不直接开事务。
- 响应辅助类型：`Created` / `NoContent` / `Paginated<T>`；分页参数与响应结构全框架统一。
- OpenAPI 由 `utoipa` 生成，`/docs` 在非生产环境默认开启。

## 错误模型

- 单一 `openrust::Error` 枚举：`NotFound` / `Unauthorized` / `Forbidden` / `Validation` / `Conflict` / `UnprocessableEntity` / `RateLimited` / `Config` / `Database` / `Queue` / `Internal`。业务自定义错误通过 `Error::custom(status, code, message)` 或 `impl From<DomainError> for Error` 接入。
- `Error` 实现 `IntoResponse`，输出 `application/problem+json`：`type`（`urn:openrust:problem:<code>`）/ `title` / `status` / `code` / `detail`，验证错误追加 `errors: { field: [message] }`。请求关联靠响应头 `x-request-id`，不在正文重复。
- 5xx 错误（`Internal` / `Config`）不返回 `detail`，细节只进入日志；`Error::Internal` 包装任意 `std::error::Error`，数据库等能力 crate 的错误通过 `From` 进入该变体或映射为 `NotFound` / `Conflict`。
- 库 crate 使用 `thiserror`；`anyhow` 只出现在二进制边界。

## 数据层

- SeaORM 2.x 作为 ORM，`sea-orm-migration` 管理迁移；SQLx 直连通过 `db.sqlx()` 显式获取，使用点必须在代码评审中可见。
- 命名约定：表名 snake_case 复数；主键 `id BIGINT` 自增；`created_at` / `updated_at` 必备；软删除通过 `--soft-delete` 生成 `deleted_at` 并接入查询默认过滤；`--uuid` 生成 UUIDv7 主键。
- 时间统一 UTC：PostgreSQL 用 `timestamptz`，MySQL 用 `DATETIME(6)`；JSON 列 PostgreSQL 用 `jsonb`，MySQL 用 `json`；框架表不使用数组、枚举等单库特性。
- 双库边界：迁移只用 `sea-orm-migration` 可移植列类型；必须分方言的 SQL 集中在 `openrust-db::dialect`，每个分支必须同时提供两库实现并有双库测试。
- 事务：`db.transaction(|tx| async move { ... }).await`，闭包返回 `Err` 自动回滚；服务层接收 `&DatabaseTransaction` 或 `&Db` 的统一 `ConnectionTrait`。
- 连接池参数、慢查询日志阈值、语句超时在 `[database]` 配置段统一。
- 测试隔离：每个测试用例使用独立数据库，由迁移后的模板库复制（PostgreSQL `CREATE DATABASE ... TEMPLATE`，MySQL 迁移结果缓存后重放），测试结束丢弃。

## 认证与授权

- 用户表约定：`id` / `name` / `email`（唯一）/ `password_hash` / `email_verified_at` / `created_at` / `updated_at`；业务通过迁移追加字段，不要求继承框架结构体。
- 密码 Argon2id，参数可配置并带安全默认值；登录、注册、密码重置端点默认接入限流。
- `SessionGuard`：Cookie 会话，数据库存储默认，Redis 可选；配合表单时启用 CSRF。`TokenGuard`：个人访问令牌，存储 SHA-256 哈希，支持前缀、能力列表和过期时间。JWT 不是默认 driver，第一代仅在确认无状态场景后作为可选 feature。
- `CurrentUser` 提取器按路由组所选 Guard 解析用户；`OptionalUser` 用于匿名可访问路由。
- 内置流程：注册、登录、登出、密码重置（令牌表 + 邮件）、邮箱验证（签名 URL）。
- 授权：`Policy` trait 按资源定义动作枚举；`roles` / `permissions` / 关联表提供最小角色权限模型，`user.can("posts.publish")` 与 `Policy` 组合。DataScope、多租户不在第一代。

## 队列、调度与事件

```rust
#[derive(Serialize, Deserialize, Job)]
#[job(queue = "mail", retries = 3, backoff = "exponential")]
pub struct SendWelcomeEmail { pub user_id: i64 }

impl Handle for SendWelcomeEmail {
    async fn handle(self, ctx: &JobContext) -> Result<()> { /* ... */ }
}

queue.dispatch(SendWelcomeEmail { user_id }).delay(Duration::from_secs(60)).await?;
```

- Queue driver：`database`（`jobs` 表 + `SKIP LOCKED`，两库皆支持）默认；`redis` 可选 feature；`sync` 用于测试与开发期同步执行。
- 失败处理：超过重试次数进入 `failed_jobs` 表；`queue:failed` / `queue:retry <id|all>` 可操作；worker 收到停机信号后完成当前任务再退出，不丢任务。
- 调度：`schedule.job(PruneTokens).daily_at("03:00")`，`schedule.command("cache:clear").every_hour()`；通过数据库锁保证多副本只执行一次；`schedule:run` 支持常驻循环与单次 tick 两种运行方式。
- 事件：`#[derive(Event)]` 定义，`app.listen::<UserRegistered, SendWelcomeListener>()` 注册；监听器可标记 `queued` 走队列。第一代只做进程内分发，不接外部消息总线。

## 缓存、存储与邮件

- `Cache`：`get` / `put` / `forget` / `remember(key, ttl, closure)` / `increment`；memory（moka）默认，redis 可选；值使用 serde 序列化；不做 tag 失效。
- `Storage`：封装 OpenDAL `Operator`，`storage.disk("public").put(path, bytes)` / `get` / `delete` / `url` / `temporary_url`；本地磁盘默认，S3 兼容通过 `storage-s3` feature。
- `Mail`：`Mailable` 提供信封与内容（minijinja 邮件模板或纯文本），`mailer.send(mail)` 与 `mailer.queue(mail)`；driver smtp（lettre）/ log / fake；开发环境 compose 中提供 Mailpit。

## 可观测性

- `tracing` 为唯一日志与追踪入口；`[log] format = "text" | "json"`，`RUST_LOG` 优先于 `[log] level`；请求 span 含 `request_id` / method / uri，状态与耗时由 `TraceLayer` 记录。
- `x-request-id` 入站沿用、缺失时生成 UUID、出站写回；中间件固定顺序为请求 ID → 传播 → 追踪 span → 超时（408）→ panic 捕获（500 Problem）。
- `/health` 只报告进程存活；`/ready` 带超时检查数据库、队列、缓存依赖并逐项报告。
- 可选 feature：`otel` 导出 OTLP；`metrics` 暴露 Prometheus 端点。
- handler panic 由 `catch_panic` 捕获并转换为 500 Problem 响应，进程不退出。

## CLI

| 命令 | 提供方 | 作用 |
| --- | --- | --- |
| `openrust new <name> --db postgres\|mysql [--no-auth]` | 全局 CLI | 生成项目，锁定模板版本与所选驱动 feature |
| `openrust dev` | 全局 CLI | 启动 compose 依赖、等待就绪、执行迁移、监听源码变更重新编译并运行 |
| `openrust make:model Post title:string body:text published_at:datetime? [--controller] [--policy] [--test] [--soft-delete] [--uuid]` | 全局 CLI | 生成迁移、实体、可选控制器 / 策略 / 测试，并打印路由片段 |
| `openrust make:controller\|migration\|policy\|job\|event\|listener\|mail\|middleware\|seeder\|factory\|test\|provider` | 全局 CLI | 单项生成器 |
| `openrust <运行命令>` | 全局 CLI 代理 | 转发为 `cargo run --quiet -- <命令>` |
| `serve` | 应用二进制 | 启动 HTTP 服务 |
| `migrate [--status\|--rollback\|--fresh]` / `db:seed` / `db:reset` | 应用二进制 | 迁移与数据初始化 |
| `queue:work [--queue]` / `queue:failed` / `queue:retry` | 应用二进制 | 队列 worker 与失败任务处理 |
| `schedule:run [--once]` / `schedule:list` | 应用二进制 | 调度 |
| `route:list` / `config:show` / `key:generate` / `user:create` | 应用二进制 | 诊断与运维 |

生成器规范：模板用 minijinja 渲染；不覆盖已存在文件，除非 `--force`；输出每个生成路径；对生成文件执行 `rustfmt`；文件名、类型名、表名、路由名的推导规则在文档中列表说明，并有快照测试覆盖。

## 测试工具

- `TestApp::boot().await` 以 `APP_ENV=test` 启动应用并自动使用 fake driver；`app.client().post("/api/posts").json(&input).send().await` 返回可链式断言的响应（`assert_status` / `assert_json_path` / `assert_problem`）。
- 数据库隔离按「数据层」约定，每个用例独立数据库。
- fake driver 断言：`Queue::fake().assert_dispatched::<SendWelcomeEmail>()`、`Mailer::fake().assert_sent_to("a@b.c")`、`Storage::fake().assert_exists(path)`。
- `Factory`：`UserFactory::new().count(3).create(&db).await`，字段默认值用 `fake` crate 生成，支持状态方法与关联。
- 时间通过 `Clock` 服务注入，测试可冻结或推进。

## 框架代码规范

- Rust Edition 2024；MSRV 为当前 stable 减两个小版本，写入 `rust-version` 并在 CI 验证。
- 所有 crate `#![forbid(unsafe_code)]`；库 crate `#![deny(missing_docs)]`，每个公共入口类型附带可运行 doctest。
- `[workspace.lints]` 启用 `clippy::all` + `clippy::pedantic`（显式 allow 需注释原因）；库 crate 拒绝 `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented`。
- `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`、`cargo deny check`、`cargo semver-checks` 为 CI 必过项。
- 异步统一 tokio；trait 中的 async 方法使用原生 `async fn` 并通过 `Send` 约束保证可跨任务。
- feature 命名：`db-postgres` / `db-mysql` / `queue-redis` / `cache-redis` / `storage-s3` / `mail-smtp` / `templates` / `otel` / `metrics`；feature 只做加法，门面 crate 默认不含任何数据库驱动，由生成的项目显式选择。
- 命名：crate `openrust-<capability>`；driver 类型 `<Backend><Capability>`（`RedisCache` / `DatabaseQueue`）；配置 `<Capability>Config`；错误 `<Capability>Error`；provider `<Capability>Provider`。
- crate 内部布局：`lib.rs`（文档 + re-export）/ `config.rs` / `error.rs` / `provider.rs` / `drivers/` / `testing.rs`（`test-util` feature）。
- 公共 API 以 0.x 语义化版本发布，破坏性变更只在次版本号；内部项用 `#[doc(hidden)]` 与 `__private` 模块隔离；变更日志由 git-cliff 从提交生成。
- 文档站信息架构对齐 Laravel：入门 → 基础（路由、中间件、控制器、请求、响应、验证、错误）→ 深入（CLI、事件、队列、调度、缓存、存储、邮件）→ 数据库 → 安全 → 测试 → 部署。每个 crate 的 README 即 `lib.rs` 文档。
- 模板快照测试是最重要的 CI 任务：每次 CI 对两种数据库各生成一个项目并执行 `cargo check` 与 `cargo test`。

## 相关

- [OpenRust 第一代开发计划](gen1-plan.md)
- [开发计划索引](README.md)
