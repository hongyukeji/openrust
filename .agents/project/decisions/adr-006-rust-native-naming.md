# ADR-006 框架命名、目录与命令约定

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-25（同日依主流 Rust 框架调研修订）

## 背景

初版设计按 Laravel 的概念起草，代码与文档里留下了 `ServiceProvider::register/boot`、`index/show/store/update/destroy`、`make:*`、`config:show`、`Mailable`、`Seeder` 等带有其他生态习惯的词。OpenRust 的用户有很多是从 PHP、Ruby、Python、Java、JS、Go 转来的。目标是新手和老手都能很快上手，开箱即用。所以命名既要让 Rust 开发者觉得地道，也要让跨语言开发者一眼认得，而且不能特立独行。

本 ADR 维护通用命名规则；能力语义与后续替代关系见 [ADR 索引](README.md)，当前 API 事实以源码为准。整套源码交付、内部包命名与 apps 生成方式按 [ADR-039](adr-039-source-workspace-distribution.md)。日常操作清单见 [conventions.md「框架规范」](../conventions.md#框架规范)。

## 判定规则

按优先级：

1. **优先遵循 Rust 生态惯例，并综合对照目标框架。** axum、Tower、Rocket、actix-web、Poem、Salvo、Pavex、Bevy、SeaORM、sqlx-cli 用于核对 Rust API、HTTP、插件和数据层惯例；V1 按 Loco、Laravel、Goravel、OpenSpring 的顺序综合评估，RuoYi 若依、芋道仅作为 V2 参考，见 [框架设计「框架参考与对标」](../plans/framework-design.md#框架参考与对标)。不存在单一框架对所有设计拥有优先权。
2. **没有共识的，用跨语言通用的 Web 框架词汇**，如 controllers、models、migrations、seeds、jobs、mailers、middleware，前提是不违反 Rust API 惯例。
3. **不自造词，也不用单一生态的行话**，例如 Laravel 的 `Mailable` / `ServiceProvider` / `boot` / `Guard` / `disk`、Rocket 的 `Fairing`、OpenDAL 的 `Operator`。
4. **分两层**：
   - **生成项目的布局与 CLI**：保持 Rust 项目惯例清晰，并按[框架参考与对标](../plans/framework-design.md#框架参考与对标)的版本分工综合检查；采用通用、可读的概念，不因单一项目的术语或目录形式形成特例。
   - **库 API**：保持 Rust 惯用写法，以 std、axum 和 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)（下称 RAG）为准，包括 C-CASE、C-CONV、C-GETTER、C-WORD-ORDER、C-CTOR。

新增或修改公共名称前，先查本 ADR。本 ADR 没有覆盖的名称，先在这里补一行并写明先例，再写代码。

Laravel 式开发体验与成熟组件整合遵循 [ADR-001](adr-001-own-kernel-on-axum.md#成熟组件优先与-laravel-式开发体验)。设计思想和用户体验可以借鉴；本 ADR 的 Rust 命名规则不要求机械改写既有 API，也不要求重新封装上游已合适的类型。

## 决策

### 1. 生成项目布局（`openrust new`）

以下为完整模板的目标职责，不要求最小无数据库模板生成空目录。已实现选项见 [CLI 事实](../core/cli.md)；对应能力/生成器落地时才生成其实际文件。

```text
<app>/
├── Cargo.toml              # [package.metadata.openrust] 存放项目元数据，不另建 openrust.toml
├── .env.example
├── config/
│   ├── default.toml        # 默认配置
│   ├── development.toml
│   ├── test.toml
│   └── production.toml
├── src/
│   ├── main.rs             # 调用 openrust::run::<用户应用类型>()
│   ├── app.rs              # impl Application：plugins、routes、jobs、schedule
│   ├── config/             # Rust 配置加载、业务配置类型与校验模块
│   │   └── mod.rs
│   ├── routes.rs
│   ├── schedule.rs
│   ├── seed.rs             # `db seed` 入口，普通 async fn
│   ├── controllers/        # handler 函数，一资源一文件
│   ├── models/             # 业务 impl；SeaORM 生成的实体放 models/_entities/
│   ├── services/
│   ├── policies/
│   ├── jobs/
│   ├── events/
│   ├── mailers/
│   └── middleware/
├── migration/              # 独立 workspace 成员 crate（sea-orm-cli 默认）
├── tests/
├── compose.yaml
├── Dockerfile
└── AGENTS.md               # 供 AI 辅助的新用户使用（loco 先例）
```

上述根 `config/` 的环境命名示意已由 [ADR-030](adr-030-modular-configuration.md) 更新为按领域命名的 TOML 文件；本 ADR 的目录图仅记录当时的方案。

依据：
- loco 生成项目使用 `src/{controllers,models,models/_entities,workers,mailers,tasks}` 与 `migration/`。
- salvo-cli 使用 `models/`。
- 文档中把函数称为 handler，与 axum 和 loco 一致；应用目录采用通用的 `controllers/`。
- V1 须交付普通 Rust builder/fixture 形式的显式轻量测试数据工具，复用 `testing::TestDatabase`，不提供隐式 `Seeder` / `Factory` trait 或第二套 ORM；具体签名与随机数据组件在实施 Spec 确定，`fake` 仅为候选。范围见 [补齐计划](../plans/v1-foundation-completion.md)。

### 2. CLI

- 子命令用空格嵌套，名称用 kebab-case，不用冒号。依据：cargo、sqlx-cli（`sqlx migrate run`）、sea-orm-cli、`cargo loco db migrate`。
- **当前全局命令**：`openrust new <name> [--db postgres|mysql]`、`openrust dev`、`openrust serve`、`openrust generate`、`openrust key generate`；其余命令转发给项目二进制。
- **生成器**：`openrust generate <kind>`，别名 `g`。当前 kind 为 `model`、`migration`、`mailer`、`policy`，以 [CLI 事实](../core/cli.md) 为准；v1 待补 `scaffold`、`controller`、`job`、`command`，按当前任务队列实现和验证。
  - 字段 DSL 使用 loco / Rails 的小写类型名，例如 `title:string published_at:date_time?`。`references` 表示外键，`?` 表示可空。
- V1 须在既有生成器入口支持应用模板覆盖、清单/导出/预览与版本检查，详见补齐计划；具体 CLI 签名待 Spec，不把目标写成已实现命令。
- **运行期命令**由应用注册的 Plugin 提供，不因全局 CLI 可转发就自动可用。现有入口包括 `serve`、`help`、`config show`、`migrate`、`db seed`、`queue work`、`schedule run`、`schedule list`；队列其他命令以 `QueuePlugin` 为准。`migrate` 接受 `up [steps]`、`down [steps]` 和 `status`。`db reset`、`db status`、`routes`、`doctor` 是待实现的 v1 入口；`doctor` 只读语义按 ADR-023。

### 3. 配置与环境变量

本节由 [ADR-016](adr-016-neutral-application-namespaces.md) 校准：应用默认 `APP_ENV`、`APP_CONFIG_DIR` 与 `APP_<SECTION>__<KEY>`，数据库、Redis、队列、邮件和短信使用领域前缀变量。未采用 `DATABASE_URL` / `REDIS_URL` 通用环境别名；不恢复旧 OPENRUST_ 应用配置前缀。

TOML 分层、环境引导与生产 `.env` 保护保持有效，具体兼容、表前缀与实现验收按 ADR-016；不混读其他应用命名空间。

### 4. 库 API

| 领域 | 名称 | 依据 |
| --- | --- | --- |
| 扩展单元 | `Plugin::build(&self, &mut AppBuilder) -> Result<()>`；`finish` 返回 `BoxFuture<Result<()>>`；`AppBuilder::plugin()` | 先全部 build（含期间新增）再冻结，按序异步 finish；worker/scheduler 生命周期按 ADR-037，不新增平行 Hooks |
| 数据库迁移 | `MigrationPlugin<M>::new()` 注册 `migrate [--database <name>] [up [steps]|down [steps]|status]`；`M` 实现 `sea_orm_migration::MigratorTrait` | SeaORM migration 的 `MigratorTrait` 与其 up/down/status 生命周期；迁移由应用 crate 定义，数据库能力 crate 负责运行 |
| 数据库连接选择 | `Databases::default_database()` / `Databases::get(name)`；配置 `[database.instances.<name>]`；migration 用 `--database <name>` 显式选择 | `Db` 继续作为兼容的 default handle；具名实例契约见 ADR-045，不引入通用服务定位器 |
| 服务注册表 | `AppBuilder::insert(service)` / `AppBuilder::get::<T>()`；`AppState::get -> Option<Arc<T>>`；`AppState::try_get -> Result<Arc<T>>`；缺失时返回 `MissingServiceError` | `http::Extensions::insert/get`；std 中 `try_*` 返回 `Result`；RAG C-WORD-ORDER |
| 提取器 | `Inject<T>`、`Validated<T>`、`CurrentUser` | `shaku_axum::Inject`；axum-valid `Valid<T>`。备选名会与 tower、axum、actix、bevy 的已有类型重名 |
| 路由 | `Routes::nest(prefix, f)`（空前缀或 `/` 时 merge）、`merge`、`layer`、`resource::<R>(path)`、`resource_with` | axum、Poem、loco 都用 `nest`；actix `web::resource` |
| 资源动作 | `Action::{List, Get, Create, Update, Delete}`；`ResourceActions::{list, get, create, update, delete}`；映射为 list → GET path、create → POST path、get → GET path/{id}、update → PUT 和 PATCH path/{id}、delete → DELETE path/{id} | 开发者已定；与 Google AIP 标准方法一致 |
| 资源控制器宏 | 在控制器类型的 inherent `impl` 上标注 `#[openrust::resource]`；宏读取其中名为 `list` / `get` / `create` / `update` / `delete` 的无 receiver 方法并生成 `Resource` 实现，未识别的方法保留为普通辅助方法 | handler 是关联函数，不会与模块级 `axum::routing::{get, delete}` 导入冲突；方法名继续使用领域动作词 |
| 响应 | `Created<T>`、`NoContent`、`PageParams { page, per_page }`、`PageMeta { page, per_page, total, total_pages }`、`Paginated<T>`、`ApiResponse<T>` | axum 的类型化响应风格；`per_page` 与 GitHub REST API 一致；`PageMeta` 沿用 `PageParams` 的 `Page` 前缀并明确表示分页元数据 |
| 命令 | `trait Command`，名称为空格分隔的多个词，如 `config show`；`accepts_args()` 声明是否接收名称后的参数，默认不接收 | 同第 2 节；参数需由命令显式声明，参照 clap 的参数定义 |
| 邮件 | `trait Email`，产出 `lettre::Message`，由 `Mailer` 发送 | lettre 与 loco 的 `Mailer` |
| 短信 | `SmsMessage` / `SmsSender` / `SmsDriver` | ADR-033；独立于 Mailer，provider receipt 只表示受理 |
| 国际化 | `I18n` 服务、`I18nPlugin`、`Locale`、`RequestLocale`、`MessageCatalog`；请求上下文使用 `RequestLocale`，显式覆盖用 `I18n::with_locale` | Rust `locale` / `catalog` 通用词；避免自造 `TranslatorContext` 或全局 locale API |
| 缓存（待实现） | `Cache`、default/named stores；方法名 `get` / `insert` / `remove` / `get_or_insert_with` / `increment` | 按 ADR-014 定义能力与错误，具体签名在 M5 contract 收口 |
| 存储（待实现） | `Storage`、命名实例与 `read` / `write` / `delete` / `url` / `presign_read` 等操作 | 目标命名，stream/授权/发布门槛及具体签名先定 contract，不表示 API 已存在 |
| 队列 | `Job::NAME` 与 `handle(&self, JobContext) -> BoxFuture<'_, Result<()>>`；显式 `QueuePlugin::register_job` | 与已提交 `src/queue.rs` 对齐；旧 `Job::run`/属性宏示意不再作为 contract |
| 认证 | 由认证 ADR 决定，不使用 `Guard`，因为 Rocket 的 request guard 和 actix 的路由谓词已占用这个词 | — |
| 测试 | `testing::TestDatabase` 已有；`TestApp`/HTTP assertions 与显式轻量 fixture 工具待 M6 contract | ADR-023 与补齐计划；不固化未实现的 `spawn()` 调用，不要求真实 socket 或隐式 Factory trait |
| 错误 | 统一 `openrust::Error`；具体变体以 `src/core/error.rs` 为准，不假设存在 `Database`/`Queue` 变体 | RAG C-GOOD-ERR；公共 HTTP 错误语义见 ADR-008 |
| 门面 | 根模块 `openrust::http` 等与 `openrust::prelude` re-export；用户应用类型避免叫 `App` | 根 package 自身实现框架，不恢复旧 `openrust_http` 等已移除依赖 |

### 5. 仓库与元数据

- 仓库实现布局按 [ADR-021](adr-021-loco-style-module-layout.md)：主框架能力按模块组织在根 `src/`；只为 CLI、proc-macro 等真实技术边界保留独立 package。应用生成布局仍按本 ADR §1。
- `Cargo.toml` 的 keywords 设为 `framework`、`web`、`axum`、`batteries-included`、`backend`，不写其他框架名。
- README 介绍 OpenRust 自身定位与实际能力，可以如实说明 Laravel 等设计来源；不把借鉴思想表述为 API 兼容、完整复刻或未经验证的能力优势。
- 根目录补齐 `CHANGELOG.md`（Keep a Changelog 格式）、`CONTRIBUTING.md`、`SECURITY.md`、`CODE_OF_CONDUCT.md`（引用 Rust Code of Conduct）。不新增 `rustfmt.toml`。
- `docs/` 在 M6 用 mdBook 建立，按 Diátaxis 分为 Tutorials、How-to、Reference、Explanation 四部分。其中设一页“从其他框架迁移”，集中维护 Laravel、Rails、Django、Spring、Express/NestJS、axum 与 OpenRust 概念对照；设计和说明文档可记录借鉴思想，公共 API 仍遵循本 ADR。另设一页 coming-from-axum。

### 6. 开箱即用

- v1 完整标准模板的验收目标包含注册/登录、scaffold、邮件、job、请求测试及依赖编排。当前最小模板与显式 `--db`/`--auth` 路径继续保留；完整模板的选项/默认组合按 M6 contract 定稿，不能把目标写成当前 CLI 行为。
- `doctor` 的只读检查和 `routes` 的路由清单仍待交付；`dev` 当前仅选择开发环境并启动 serve，依赖复用/创建、迁移与文件监听重启仍需实现并验收。

## 明确不改

`App` / `AppBuilder` / `AppState` / `Application`、`Resource` trait、`Validated`、`Inject`、`Policy`、`Job`、`.env`、`compose.yaml`、feature 名（`db-postgres`、`test-util`）、CLI 的二进制名 `openrust`、`thiserror` / `anyhow` 的分工。事件能力名称随对应 contract 登记，不因命名约定推断 derive 或注册 API 已实现。

## 影响

- **影响边界**：根 `openrust` 的 module/public API、prelude、工具包、生成模板、示例、README 与测试；目录变化按 ADR-021，不恢复已合并的能力 package。
- **设计与计划文档**：`plans/framework-design.md`、`plans/gen1-plan.md`、`architecture.md`、`context.md`、`todo.md`、`roadmap.md`，以及 ADR-001、ADR-002、ADR-004 中的术语。决策内容不变。

## 执行

规范即约束：Agent 开发时按本 ADR 与 conventions.md「框架规范」起名，不设额外的旧名扫描门禁。新增或修改公共名称时，本 ADR 中应能找到对应条目；找不到的先补条目。

## 调研依据

- **loco**：
  - [base_template](https://github.com/loco-rs/loco/tree/HEAD/loco-new/base_template)
  - [app.rs](https://github.com/loco-rs/loco/blob/HEAD/src/app.rs)
  - [environment.rs](https://github.com/loco-rs/loco/blob/HEAD/src/environment.rs)
  - [cli.rs](https://github.com/loco-rs/loco/blob/HEAD/src/cli.rs)
  - [scaffold controller](https://github.com/loco-rs/loco/blob/HEAD/loco-gen/src/templates/scaffold/api/controller.t)
  - [storage](https://github.com/loco-rs/loco/tree/HEAD/src/storage)
  - [coming-from-axum](https://github.com/loco-rs/loco/blob/HEAD/website/src/content/docs/docs/explanation/coming-from-axum.md)
- **Rocket**：[fairing](https://github.com/rwf2/Rocket/blob/HEAD/core/lib/src/fairing/mod.rs)、[config](https://github.com/rwf2/Rocket/blob/HEAD/core/lib/src/config/config.rs)
- **Pavex**：[template](https://github.com/LukeMathWalker/pavex/tree/HEAD/compiler/pavexc_cli/template)
- **Salvo**：[salvo-cli classic](https://github.com/salvo-rs/salvo-cli/tree/HEAD/templates/classic)、[Depot](https://github.com/salvo-rs/salvo/blob/HEAD/crates/core/src/depot.rs)
- **summer-rs**：[Plugin](https://github.com/summer-rs/summer-rs/blob/HEAD/summer/src/plugin/mod.rs)、[env](https://github.com/summer-rs/summer-rs/blob/HEAD/summer/src/config/env.rs)
- **Bevy**：[Plugin](https://github.com/bevyengine/bevy/blob/HEAD/crates/bevy_app/src/plugin.rs)
- **SeaORM**：[axum_example](https://github.com/SeaQL/sea-orm/tree/HEAD/examples/axum_example)
- **Poem**：[router](https://github.com/poem-web/poem/blob/HEAD/poem/src/route/router.rs)
- 未逐条复核、依据 docs.rs 既有知识：axum `Router`、actix-web `web::scope`、Tauri `Builder::plugin`。
