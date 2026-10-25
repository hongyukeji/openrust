# OpenRust 框架设计

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust v1 版本框架底座的架构设计：设计原则、crate 与应用目录布局、内核、配置、路由、错误、数据、认证、异步能力、CLI、测试工具与框架代码规范。目标、里程碑与验收见 [gen1-plan.md](gen1-plan.md)。

## 设计原则

框架定位与 v1 已确认需求以 [v1 计划](gen1-plan.md) 为准：提供高性能、开箱即用且可扩展的通用底座，让开发者集中于业务开发；性能结论必须有代表性场景证据。

OpenRust 追求的优雅是 Rust 语境下的优雅：靠类型、trait 和约定减少样板，不靠运行期魔法。

1. **约定优于配置，但约定可见**：目录、命名、路由、迁移和配置键有默认约定；每个约定在生成代码或文档中可查，不藏在宏展开里。
2. **显式依赖**：服务在 `Plugin` 中用 `AppBuilder::insert` 注册，处理函数用 `Inject<T>` 声明依赖，签名即依赖清单；不提供全局 Service Locator、运行期反射容器或静态 Facade。
3. **一件事一个明显做法**：每类能力只有一个官方入口（验证只用 `Validated<T>`，授权只用 `Policy`），不保留多套并存。
4. **错误是一等公民**：框架错误统一为 `openrust::Error`，自动映射为 `ApiResponse<T>` 统一包装响应；库代码禁止 `unwrap` / `expect` / `panic!`。
5. **宏只做派生，不做控制流**：`#[derive(...)]` 和少量属性宏用于消除样板；不用宏改变程序结构或隐藏调用关系。
6. **可替换但不过度抽象**：缓存、队列、存储、邮件各一个 trait、多个 driver；trait 只覆盖已有 driver 的共同能力，不预留假想扩展点。
7. **生成即普通代码**：CLI 生成的文件是开发者会亲手写出的代码，可读、可改、可删，无框架专属注解。
8. **可测试是默认值**：每个能力自带 fake driver 和测试工具，业务测试不需要真实 Redis、SMTP 或对象存储。
9. **最小默认，按需开启**：核心路径只依赖 tokio / axum / tower / serde / tracing；数据库驱动、Redis、S3、OpenTelemetry 等通过 Cargo feature 开启。

## 关键流程的公开扩展点

v1 应在数据库事务、注册/登录、文件上传等常规流程的合理边界提供公开、类型化的钩子或事件，让应用/第三方包通过注册扩展，不修改核心源码。现有 Plugin 构建钩子不能代替业务流程扩展点。

| 边界 | 最低语义 |
| --- | --- |
| 事务提交前 | 在可控事务仍有效时执行必要校验，失败可中止并回滚；不在持锁期间执行邮件/SMS 等不可回滚远程副作用 |
| 事务提交后 | 仅真实提交成功后通知；回滚和提交失败不触发；回调失败不能把已提交事务伪装为已回滚，错误/补偿策略明确 |
| 用户注册前 | 扩展业务校验与允许的资料转换，可明确拒绝；不泄漏凭据或绕过框架安全规则 |
| 用户注册成功后 | 用户数据提交后发出成功事实；欢迎消息、资料初始化等按是否需同事务/异步分别注册，不能模糊执行位置 |
| 登录前/成功/失败 | 登录前可增加风控校验；成功/失败通知在凭据验证与会话/令牌状态明确后触发，不允许监听器绕过认证；载荷脱敏 |
| 文件接收前 | 校验权限、配额、声明元数据和上传策略，可拒绝；仅凭元数据不能完成实际内容检测 |
| 内容可读取、发布前 | 向扩展提供有界流或受控临时对象，允许图片/敏感内容/恶意文件检测及处理；要求检测时，通过前不可公开访问 |
| 上传完成/失败 | 成功后可扩展缩略图、索引、审计或通知；失败/拒绝/取消清理临时资源，持久化与对外发布的成功状态分别明确 |
| 其他能力流程 | 邮件/短信、任务、存储等按真实扩展场景复用同类契约，避免为每个内部函数机械增加 before/after |

- 同步钩子用于会影响当前操作的校验/处理，事件用于已经发生的事实，HTTP 管道用于请求横切处理；三者职责明确，复用 Plugin/事件/队列机制，不建立三个重叠的全局扩展系统。
- 注册入口、载荷、可变字段、执行顺序、短路、失败传播、异步/取消与资源边界必须形成 contract。默认不注册扩展即可正常运行；提供独立应用注册示例，不依赖反射、字符串事件名或全局可变注册表。
- 数据库钩子围绕框架管理的事务边界，明确嵌套事务/savepoint、重试与回调执行次数。底层直连/原生 SQL 逃生口的覆盖范围必须说明，不能承诺拦截全部 ORM/驱动调用。
- 文件内容检测是可选扩展，框架交付注册、读取、拒绝、状态与清理机制，不强制内置特定审核算法或供应商。未注册检测时保持普通上传的简单路径。同步检测须有超时/并发上限；异步检测标为待处理，通过前私有隔离，超时/失败不能默认为通过。
- 检测结果必须绑定实际发布的对象版本/内容，检测后再次修改需重新校验；缓存/CDN、临时链接、下载接口及客户端直传完成路径也不得绕过发布门槛。集群异步检测需共享状态和幂等处理，不能依赖另一节点不可访问的本地临时文件。检测会消费流时，定义重读/暂存契约，不无界缓冲整个文件，也不向扩展泄漏存储密钥。
- after-commit 内存回调不等于可靠投递；需跨进程故障恢复的副作用复用事务内数据库任务或 outbox。集群下区分“提交该事务的节点执行一次”与“所有节点广播”，监听器可能重试时要求幂等。
- 验收包含公开注册/顺序、前置拒绝、提交/回滚/提交失败、后置错误、注册登录成功与失败、敏感载荷、重试及集群投递边界；上传另验证检测拒绝/超时、发布前不可读、版本绑定、直传与下载门槛、失败清理和集群恢复。真实事务语义需双库证据。具体 API 名称和布局先按 ADR-006 定稿。

## 跨语言设计参考

OpenSpring 可作为设计参考，编程思想与问题解决方法可以跨语言复用；OpenRust 仍是独立 Rust 框架，不是 Java 项目的移植。

- 在相关功能簇设计时，按需只读查看 `/Users/Home/Codes/Java/openspring` 的实际规则、设计和代码；先定位相关模块，不先全仓扫描。不修改参考仓库，不依赖旧聊天或记忆认定其当前实现。
- 候选参考主题包括模块边界与聚合启动、能力接口与供应商适配、配置与启动校验、权限/审计、事务/事件/幂等、缓存一致性、任务调度与集群治理。这是待核查方向，不表示已证实 OpenSpring 均有可复用实现。
- 每次采纳需说明：来源路径与所解决问题、适用前提、OpenRust 既有对应能力、采用/调整/不采用的理由，以及 Rust 实现与验证方式。重要公共契约变化形成 ADR，普通局部设计写入对应 Spec，不复制执行流水。
- 使用 Rust 的类型、trait、所有权、显式依赖与异步模型表达设计；不机械复制 Java 继承层级、反射/AOP、全局服务定位或注解驱动控制流。已有成熟 Rust 机制足够时优先复用。
- 每类职责保留一个公共入口，先扩展既有能力，避免为“参考模式”新增平行框架。跨项目业务模型、命名、默认参数和历史约束不自动带入本仓库。
- v1 参考通用基础设施与多副本正确性；v2 再参考后台产品模块与操作流程。采纳后仍执行本项目双库、集群、安全、性能与易用性验收，不能用参考项目可用代替本项目证据。
- 借鉴思路不要求复制代码；实际复用代码时核对许可证、记录来源并按要求保留声明。

## v1 通用能力补齐

国际化、WebSocket、统一出站 HTTP、加密与密钥管理、审计和请求幂等纳入 v1。其最小范围、推荐方案、里程碑与验收见 [通用能力计划](v1-common-capabilities.md)。优先复用既有能力 crate 与成熟实现；公共 API、配置和依赖选型先定 contract，不自动为每项能力新增 crate。

## 框架仓库布局

```text
openrust/
├── Cargo.toml                 # workspace、[workspace.lints]、统一依赖版本
├── crates/
│   ├── openrust/              # 门面 crate：re-export、prelude、feature 开关
│   ├── openrust-core/         # Application、AppBuilder、Plugin、Config、Error、生命周期
│   ├── openrust-http/         # Router 约定、提取器、中间件栈、统一响应包装、健康检查
│   ├── openrust-db/           # 连接池、迁移运行器、事务、方言边界
│   ├── openrust-auth/         # 用户约定、密码哈希、认证策略、Policy、密码重置、邮箱验证
│   ├── openrust-queue/        # Job、Queue driver、重试、失败表、调度器
│   ├── openrust-events/       # Event / Listener
│   ├── openrust-cache/        # Cache trait、memory / file / redis driver
│   ├── openrust-storage/      # Storage trait、OpenDAL 适配
│   ├── openrust-mail/         # Email、Mailer、smtp / log / fake driver
│   ├── openrust-macros/       # Resource 属性宏；M4 加 Job、Event derive
│   ├── openrust-test/         # TestApp、HTTP 测试客户端、数据库隔离、fake driver 断言
│   └── openrust-cli/          # `openrust` 二进制：new / dev / generate <kind> / 运行命令代理
├── templates/                 # `openrust new` 项目模板（按数据库后端渲染）
├── examples/blog/             # 黄金路径示例应用，CI 每次构建
├── docs/                      # mdBook 文档站
└── xtask/                     # 仓库维护任务：双库矩阵、模板快照测试、发布检查
```

crate 拆分按发布与编译边界确定。v1 版本允许 `events` 与调度器先作为 `core` / `queue` 的模块，出现独立发布需求再拆出；不为拆而拆。

## 生成的应用布局

`openrust new` 生成以下结构，项目元数据写在 `[package.metadata.openrust]`，不另建配置元文件：

```text
<app>/
├── Cargo.toml              # [package.metadata.openrust]
├── .env.example
├── config/
│   ├── default.toml
│   ├── development.toml
│   ├── test.toml
│   └── production.toml
├── src/
│   ├── main.rs             # 只调用 openrust::run::<App>()
│   ├── app.rs              # impl Application：plugins、routes、jobs、schedule
│   ├── config.rs
│   ├── routes.rs
│   ├── schedule.rs
│   ├── seed.rs             # db seed 入口，普通 async fn
│   ├── controllers/        # 一资源一文件的 handler 函数
│   ├── models/             # 业务 impl
│   │   └── _entities/      # SeaORM 生成实体
│   ├── services/
│   ├── policies/
│   ├── jobs/
│   ├── events/
│   ├── mailers/
│   └── middleware/
├── migration/              # 独立 workspace 成员 crate
├── templates/mail/         # 邮件模板
├── tests/
├── compose.yaml
├── Dockerfile
└── AGENTS.md
```

测试数据通过 `fake::Dummy` 与普通 builder 构造，不设专用种子或工厂 trait。

默认模板包含可运行的注册与登录、一个 scaffold 资源、一封邮件、一个 job 及请求测试。`compose.yaml` 提供开发依赖，`openrust dev` 一条命令启动完整样例。

## 应用内核与依赖注入

```rust
// src/main.rs
fn main() -> std::process::ExitCode {
    openrust::run::<app::Blog>()
}

// src/app.rs
pub struct Blog;

impl Application for Blog {
    type Config = AppConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(DatabasePlugin)
            .plugin(AuthPlugin)
            .plugin(QueuePlugin)
            .routes(routes::register)
            .jobs(jobs::register)
            .schedule(schedule::register);
    }
}
```

`routes` / `jobs` / `schedule` 是各能力 crate 通过扩展 trait 挂在 `AppBuilder` 上的方法（如 `AppBuilderHttpExt::routes`），注册内容存入 `AppBuilder::extension_mut::<T>()`，构建后由 `App::extension::<T>()` 读取。核心 crate 因此不依赖任何能力 crate。

- `openrust::run` 解析子命令：无参数或 `serve` 启动服务；`migrate` / `db seed` / `queue work` / `schedule run` / `routes` / `config show` 由框架与各 plugin 构建。应用二进制本身就是运行期 CLI，无需额外安装。
- 生命周期（`openrust_core::lifecycle::bootstrap`）：确定环境，非生产时加载 `.env` → 加载配置 → 初始化 `tracing` → 提取 `Application::Config` 并注册为服务 → `Application::build` → 依次调用 plugin `build`（含构建期间新增的 plugin）→ 冻结为 `App` → 调用 plugin `finish` → 分发命令（默认 `serve`）→ 监听并处理 `SIGTERM` / `SIGINT` 优雅停机。
- `AppState` 是按 `TypeId` 索引的只读服务注册表，业务服务通过 `app.insert(MyService::new(...))` 注册，handler 用 `Inject<MyService>` 取用；缺少注册在请求时返回 500 包装，内部错误包含类型名，不 panic。框架 crate 之间的硬依赖在 plugin `build` 中用 `app.get::<T>()` 检查并在启动时报错。
- `Plugin` 是业务模块与第三方包的唯一扩展入口：一个 crate 暴露一个 plugin 即可带入路由、迁移、配置段和命令。不做动态加载。

## 配置

- 分层合并，后者覆盖前者：crate 默认值 → `config/default.toml` → `config/{env}.toml` → `.env`（仅非生产环境自动加载）→ 环境变量。
- `OPENRUST_CONFIG_DIR` 指定配置目录，默认是工作目录下的 `config/`；进程 `OPENRUST_ENV` 优先，否则仅从 `.env` 读取环境名，若最终为 production 则不注入 `.env` 的任何变量。
- 环境变量前缀后接单下划线、层级间用双下划线：`OPENRUST_DATABASE__URL` 对应 `[database] url`。敏感值优先来自环境变量，模板文件中不出现真实值。
- 通用变量 `DATABASE_URL` / `PORT` 分别映射 `database.url` / `server.port`；同一键的 `OPENRUST_` 变量优先。Redis 配置段实现时接入 `REDIS_URL`。
- 当前 `FrameworkConfig` 的类型化结构包含 `app` / `server` / `log`；后续能力 crate 各自定义 `database` / `auth` / `queue` / `cache` / `storage` / `mail` 配置段，业务配置通过 `Application::Config` 关联类型接入，使用 `#[derive(Deserialize)]`。
- 启动时整体校验；缺失或类型错误立即退出，错误信息包含完整配置键与来源层。`config show` 输出合并结果并遮蔽密钥。

## 路由与控制器

```rust
// src/routes.rs
pub fn register(routes: Routes) -> Routes {
    routes
        .nest("/api", |api| {
            api.resource::<controllers::posts>("/posts")
               .middleware(auth::token())
        })
        .get("/health", health::live)
}

// src/controllers/posts.rs
pub async fn create(
    Inject(posts): Inject<PostService>,
    user: CurrentUser,
    Validated(Json(input)): Validated<Json<CreatePost>>,
) -> Result<Created<Json<PostResponse>>> {
    PostPolicy::authorize(&user, PostAction::Create, None)?;
    let post = posts.create(&user, input).await?;
    Ok(Created(Json(post.into())))
}
```

- `resource` 按约定映射 `list` / `get` / `create` / `update` / `delete`，缺失动作不注册；`only` / `except` 裁剪。
- 预置中间件栈从外到内：请求 ID 设置 → 请求 ID 回传 → 追踪 span → CORS → 超时 → 压缩 → panic 捕获；业务中间件在路由组上追加。
- 输入验证只通过 `Validated<T>`；授权只通过 `Policy`；事务边界在 `services/`，控制器不直接开事务。
- 响应辅助类型：`Created` / `NoContent` / `Paginated<T>`；分页参数与响应结构全框架统一。
- OpenAPI 由 `utoipa` 生成，`/docs` 在非生产环境默认开启。

## 错误模型

- 单一 `openrust::Error` 枚举：`BadRequest` / `NotFound` / `Unauthorized` / `Forbidden` / `Validation` / `Conflict` / `RateLimited` / `Config` / `Custom` / `Internal`。业务自定义错误通过 `Error::custom(code, message)` 或 `Error::custom_with_status(status, code, message)` 或 `impl From<DomainError> for Error` 接入。
- `Error` 实现 `IntoResponse`，输出 `application/json` 的 `ApiResponse<T>`：`success` / `code` / `message` / `data` / `request_id`，验证错误追加 `errors: { field: [message] }`；正文 `request_id` 与响应头 `x-request-id` 一致。
- 5xx 错误（`Internal` / `Config`）只返回通用 `message`，细节只进入日志；`Error::Internal` 包装任意 `std::error::Error`，数据库等能力 crate 的错误通过 `From` 进入该变体或映射为 `NotFound` / `Conflict`。
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
- Cookie 会话使用数据库存储，Redis 可选；配合表单时启用 CSRF。个人访问令牌存储 SHA-256 哈希，支持前缀、能力列表和过期时间；具体认证 API 由认证 ADR 定名。JWT 不在 v1 版本，列为后续候选。
- `CurrentUser` 提取器按路由组所选认证方式解析用户；`OptionalUser` 用于匿名可访问路由。
- 内置流程：注册、登录、登出、密码重置（令牌表 + 邮件）、邮箱验证（签名 URL）。
- 授权：`Policy` trait 按资源定义动作枚举；`roles` / `permissions` / 关联表提供最小角色权限模型，`user.can("posts.publish")` 与 `Policy` 组合。DataScope、多租户不在 v1 版本。

## 队列、调度与事件

```rust
#[derive(Serialize, Deserialize)]
#[job(queue = "mail", retries = 3, backoff = "exponential")]
pub struct SendWelcomeEmail { pub user_id: i64 }

impl Job for SendWelcomeEmail {
    async fn run(self, ctx: &JobContext) -> Result<()> { /* ... */ }
}

queue.dispatch(SendWelcomeEmail { user_id }).delay(Duration::from_secs(60)).await?;
```

- Queue driver：`database`（`jobs` 表 + `SKIP LOCKED`，两库皆支持）默认；`redis` 可选 feature；`sync` 用于测试与开发期同步执行。
- 失败处理：超过重试次数进入 `failed_jobs` 表；提供失败任务检查与手动重试能力；worker 收到停机信号后完成当前任务再退出，不丢任务。
- 调度：`schedule.job(PruneTokens).daily_at("03:00")`，`schedule.command("cache clear").every_hour()`；通过数据库锁保证多副本只执行一次；`schedule run` 支持常驻循环与单次 tick 两种运行方式。
- 事件：`#[derive(Event)]` 定义，`app.listen::<UserRegistered, SendWelcomeListener>()` 注册；监听器可标记 `queued` 走队列。v1 版本只做进程内分发，不接外部消息总线。

## 缓存、存储、邮件与短信

- `Cache`：`get` / `insert` / `remove` / `get_or_insert_with` / `increment`；memory（moka）默认，file 文件缓存与可选 redis 按 ADR-009 接入；值使用 serde 序列化；不做 tag 失效。
- `Storage`：封装 OpenDAL `Operator`，`storage.store("public")` 提供 `read` / `write` / `delete` / `url` / `presign_read`；本地磁盘默认，S3 兼容通过 `storage-s3` feature。
- `Mail`：`trait Email` 产出 `lettre::Message`，提供信封与内容（minijinja 邮件模板或纯文本），`mailer.send(mail)` 与 `mailer.queue(mail)`；driver smtp（lettre）/ log / fake；开发环境 compose 中提供 Mailpit。
- 短信：v1 版本提供统一发送入口、可扩展供应商适配、至少一个真实实现及 log/fake；默认本地无付费依赖，生产必须显式配置真实供应商。M4 接入队列化发送；具体命名与供应商 contract 待设计，边界与验收见 [ADR-011](../decisions/adr-011-sms-and-communication-baseline.md)。验证码机制与短信传输分离，短信登录不因发送能力加入而默认启用。

## 可观测性

- `tracing` 为唯一日志与追踪入口；`[log] format = "text" | "json"`，`RUST_LOG` 优先于 `[log] level`；请求 span 含 `request_id` / method / uri，状态与耗时由 `TraceLayer` 记录。
- `x-request-id` 入站沿用、缺失时生成 UUID、出站写回；中间件固定顺序为请求 ID → 请求作用域上下文 → 传播 → 追踪 span → CORS → 超时（408）→ 压缩 → panic 捕获（500 包装）。
- `/health` 只报告进程存活；`/ready` 带超时检查数据库、队列、缓存依赖并逐项报告。
- 可选 feature：`otel` 导出 OTLP；`metrics` 暴露 Prometheus 端点。
- handler panic 由 `catch_panic` 捕获并转换为 500 包装响应，进程不退出。

## CLI

全局 CLI 提供 `openrust new <name> [--db postgres|mysql]`、`openrust dev`、`openrust generate <kind>`（别名 `g`），其余命令转发项目二进制。生成 kind 为 `scaffold`、`controller`、`model`、`migration`、`job`、`mailer`、`policy`、`command`。字段 DSL 用小写类型名，例如 `title:string published_at:date_time? author:references`，`?` 表示可空。

运行期命令：`serve`（默认）、`migrate`、`db seed`、`db reset`、`db status`、`queue work`、`schedule run`、`schedule list`、`routes`、`config show`、`doctor`。子命令用空格，单词内部用 kebab-case。`doctor` 检查配置、数据库连接和迁移状态；`routes` 列出方法、路径和 handler。

生成器用 minijinja 渲染模板；不覆盖已有文件，除非传入 `--force`；输出路径并对 Rust 文件运行 rustfmt。生成物在 PostgreSQL 与 MySQL 模板下执行 `cargo check`、`cargo test` 和快照测试。

## 测试工具

- `TestApp::spawn().await` 以 `OPENRUST_ENV=test` 启动应用并自动使用 fake driver；`app.client().post("/api/posts").json(&input).send().await` 返回可链式断言的响应（`assert_status` / `assert_json_path` / `assert_envelope`）。
- 数据库隔离按「数据层」约定，每个用例独立数据库。
- fake driver 断言：`Queue::fake().assert_dispatched::<SendWelcomeEmail>()`、`Mailer::fake().assert_sent_to("a@b.c")`、`Storage::fake().assert_exists(path)`。
- 测试数据由 `fake::Dummy` 与普通 builder 构造；数据库初始化通过 `src/seed.rs` 的 async 函数完成。
- 时间通过 `Clock` 服务注入，测试可冻结或推进。

## 框架代码规范

- Rust Edition 2024；MSRV 为当前 stable 减两个小版本，写入 `rust-version` 并在 CI 验证。
- 所有 crate `#![forbid(unsafe_code)]`；库 crate `#![deny(missing_docs)]`，每个公共入口类型附带可运行 doctest。
- `[workspace.lints]` 启用 `clippy::all` + `clippy::pedantic`（显式 allow 需注释原因）；库 crate 拒绝 `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented`。
- `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`、`cargo deny check`、`cargo semver-checks` 为 CI 必过项。
- 异步统一 tokio；trait 中的 async 方法使用原生 `async fn` 并通过 `Send` 约束保证可跨任务。
- feature 命名：`db-postgres` / `db-mysql` / `queue-redis` / `cache-redis` / `storage-s3` / `mail-smtp` / `templates` / `otel` / `metrics`；feature 只做加法，门面 crate 默认不含任何数据库驱动，由生成的项目显式选择。
- 命名：crate `openrust-<capability>`；driver 类型 `<Backend><Capability>`（`RedisCache` / `DatabaseQueue`）；配置 `<Capability>Config`；错误 `<Capability>Error`；plugin `<Capability>Plugin`。
- crate 内部布局：`lib.rs`（文档 + re-export）/ `config.rs` / `error.rs` / `plugin.rs` / `drivers/` / `testing.rs`（`test-util` feature）。
- 公共 API 以 0.x 语义化版本发布，破坏性变更只在次版本号；内部项用 `#[doc(hidden)]` 与 `__private` 模块隔离；变更日志由 git-cliff 从提交生成。
- M6 用 mdBook 建立 `docs/`，按 Diátaxis 分为 Tutorials、How-to、Reference、Explanation；另设「从其他框架迁移」概念对照页与 `coming-from-axum`。每个 crate 的 README 即 `lib.rs` 文档。
- 模板快照测试是最重要的 CI 任务：每次 CI 对两种数据库各生成一个项目并执行 `cargo check` 与 `cargo test`。

## 相关

- [OpenRust v1 版本开发计划](gen1-plan.md)
- [开发计划索引](README.md)
