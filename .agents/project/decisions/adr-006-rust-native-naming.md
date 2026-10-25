# ADR-006 框架命名、目录与命令约定

- 状态：Accepted
- 日期：2026-10-25（同日依主流 Rust 框架调研修订）

## 背景

初版设计按 Laravel 的概念起草，代码与文档里留下了 `ServiceProvider::register/boot`、`index/show/store/update/destroy`、`make:*`、`config:show`、`Mailable`、`Seeder`、`APP_ENV` 等只属于单一生态的词。OpenRust 的用户有很多是从 PHP、Ruby、Python、Java、JS、Go 转来的。目标是新手和老手都能很快上手，开箱即用。所以命名既要让 Rust 开发者觉得地道，也要让跨语言开发者一眼认得，而且不能特立独行。

本 ADR 是 OpenRust 命名、目录与命令的唯一权威来源。日常操作清单见 [conventions.md「框架规范」](../conventions.md#框架规范)。

## 判定规则

按优先级：

1. **主流 Rust 框架已有共识的，照做。** 参照对象：loco-rs、axum、Rocket、actix-web、Poem、Salvo、Pavex、Bevy、SeaORM、sqlx-cli。其中 loco-rs 是唯一流行、基于 SeaORM、带生成器的全栈 Rust 框架，与 OpenRust 定位最接近，是**生成项目布局与 CLI 的首要参照**。
2. **没有共识的，用跨语言通用的 Web 框架词汇**，如 controllers、models、migrations、seeds、jobs、mailers、middleware，前提是不违反 Rust API 惯例。
3. **不自造词，也不用单一生态的行话**，例如 Laravel 的 `Mailable` / `ServiceProvider` / `boot` / `Guard` / `disk`、Rocket 的 `Fairing`、OpenDAL 的 `Operator`。
4. **分两层**：
   - **生成项目的布局与 CLI**：追求一眼熟悉，以 loco 和跨语言框架为准。
   - **库 API**：保持 Rust 惯用写法，以 std、axum 和 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)（下称 RAG）为准，包括 C-CASE、C-CONV、C-GETTER、C-WORD-ORDER、C-CTOR。

新增或修改公共名称前，先查本 ADR。本 ADR 没有覆盖的名称，先在这里补一行并写明先例，再写代码。

## 决策

### 1. 生成项目布局（`openrust new`）

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
│   ├── main.rs             # 只调用 openrust::run::<App>()
│   ├── app.rs              # impl Application：plugins、routes、jobs、schedule
│   ├── config.rs           # 业务配置
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
├── templates/mail/         # 邮件模板（askama / tera / minijinja 默认目录）
├── tests/
├── compose.yaml
├── Dockerfile
└── AGENTS.md               # 供 AI 辅助的新用户使用（loco 先例）
```

依据：
- loco 生成项目使用 `src/{controllers,models,models/_entities,workers,mailers,tasks}` 与 `migration/`。
- salvo-cli 使用 `models/`。
- 没有流行的 Rust 生成器使用 `handlers/` 目录。文档中把函数称为 handler，与 axum 和 loco 一致。
- 测试数据用 `fake::Dummy` 加 builder，不提供 `Seeder` / `Factory` trait。

### 2. CLI

- 子命令用空格嵌套，名称用 kebab-case，不用冒号。依据：cargo、sqlx-cli（`sqlx migrate run`）、sea-orm-cli、`cargo loco db migrate`。
- **全局命令**：`openrust new <name> [--db postgres|mysql]`、`openrust dev`。其余命令转发给项目二进制。
- **生成器**：`openrust generate <kind>`，别名 `g`。kind 包括 `scaffold`、`controller`、`model`、`migration`、`job`、`mailer`、`policy`、`command`。
  - 字段 DSL 使用 loco / Rails 的小写类型名，例如 `title:string published_at:date_time?`。`references` 表示外键，`?` 表示可空。
- **运行期命令**：`serve`（默认）、`migrate`、`db seed`、`db reset`、`db status`、`queue work`、`schedule run`、`schedule list`、`routes`、`config show`、`doctor`。

### 3. 配置与环境变量

- 配置文件为 TOML，分层合并：crate 默认值 → `config/default.toml` → `config/{env}.toml` → `.env`（仅非生产环境）→ 环境变量。
- 环境选择用 `OPENRUST_ENV`，取值 `development` / `test` / `production`。依据：loco `LOCO_ENV`、Rocket `ROCKET_PROFILE`。
- 覆盖配置用 `OPENRUST_<SECTION>__<KEY>`：前缀后接单下划线，嵌套层级之间用双下划线，例如 `OPENRUST_SERVER__PORT`、`OPENRUST_SERVER__CORS__ENABLED`。依据：Rocket `ROCKET_*`、Pavex 和 zero2prod 的“前缀 + 单下划线”写法。
- 识别 12-factor 通用变量：`DATABASE_URL` 对应 `database.url`，`REDIS_URL` 对应 Redis 连接，`PORT` 对应 `server.port`。依据：loco、sqlx、diesel、Heroku 惯例。若同时设置了 `OPENRUST_` 前缀的变量，以它为准。
- 不回退读取 `APP_ENV`、`RAILS_ENV`、`NODE_ENV`，避免同机多应用时误读。

### 4. 库 API

| 领域 | 名称 | 依据 |
| --- | --- | --- |
| 扩展单元 | `trait Plugin { fn build(&self, app); fn finish(&self, app) }`；`AppBuilder::plugin()`；能力 crate 导出 `<Capability>Plugin`，如 `HttpPlugin`、`DatabasePlugin` | Bevy `Plugin::build/finish`、summer-rs `Plugin`、Tauri `Builder::plugin`。语义：先对所有插件调用 `build`（含期间新增），冻结后再调用 `finish` |
| 服务注册表 | `AppBuilder::insert(service)` / `AppBuilder::get::<T>()`；`AppState::get -> Option<Arc<T>>`；`AppState::try_get -> Result<Arc<T>>`；缺失时返回 `MissingServiceError` | `http::Extensions::insert/get`；std 中 `try_*` 返回 `Result`；RAG C-WORD-ORDER |
| 提取器 | `Inject<T>`、`Validated<T>`、`CurrentUser` | `shaku_axum::Inject`；axum-valid `Valid<T>`。备选名会与 tower、axum、actix、bevy 的已有类型重名 |
| 路由 | `Routes::nest(prefix, f)`（空前缀或 `/` 时 merge）、`merge`、`layer`、`resource::<R>(path)`、`resource_with` | axum、Poem、loco 都用 `nest`；actix `web::resource` |
| 资源动作 | `Action::{List, Get, Create, Update, Delete}`；`ResourceActions::{list, get, create, update, delete}`；映射为 list → GET path、create → POST path、get → GET path/{id}、update → PUT 和 PATCH path/{id}、delete → DELETE path/{id} | 开发者已定；与 Google AIP 标准方法一致。生成器生成的 handler 函数名需避开与 `axum::routing::{get, delete}` 重名（loco 用 `get_one` / `remove`），具体在生成器实现时定 |
| 响应 | `Created<T>`、`NoContent`、`PageParams { page, per_page }`、`Paginated<T>` | axum 的类型化响应风格；`per_page` 与 GitHub REST API 一致 |
| 命令 | `trait Command`，名称为空格分隔的多个词，如 `config show` | 同第 2 节 |
| 邮件 | `trait Email`，产出 `lettre::Message`，由 `Mailer` 发送 | lettre 与 loco 的 `Mailer` |
| 缓存 | `get` / `insert` / `remove` / `get_or_insert_with` / `increment` | moka、std `HashMap`、loco cache |
| 存储 | `storage.store("public")`，方法为 `read` / `write` / `delete` / `url` / `presign_read` | loco `StoreDriver`、object_store `ObjectStore`、`std::fs` 的 `read` / `write` |
| 队列 | `trait Job { async fn run(self, ctx: &JobContext) }` | apalis、fang |
| 认证 | 由认证 ADR 决定，不使用 `Guard`，因为 Rocket 的 request guard 和 actix 的路由谓词已占用这个词 | — |
| 测试 | `TestApp::spawn().await` | zero2prod `spawn_app`、axum-test `TestServer::new` |
| 错误 | `openrust::Error` 变体以代码为准：`NotFound` / `Unauthorized` / `Forbidden` / `Validation` / `Conflict` / `RateLimited` / `Config` / `Database` / `Queue` / `Internal` | RAG C-GOOD-ERR |
| 门面 | `openrust::prelude` 与 `pub use openrust_http as http;` 这类 crate 级 re-export；示例和模板中的用户类型不叫 `App`，避免与 `openrust::App` 重名 | Bevy 和 tokio 的门面写法 |

### 5. 仓库与元数据

- workspace 结构保持不变：`crates/*`，crate 名为 `openrust-*`（kebab-case），每个 crate 内是 `src/`、`tests/`、`examples/`。与 rust-analyzer、Bevy、ripgrep 一致。
- `Cargo.toml` 的 keywords 设为 `framework`、`web`、`axum`、`batteries-included`、`backend`，不写其他框架名。
- README 用 Rust 原生的表述介绍自己，不写“inspired by Laravel and Rails”。
- 根目录补齐 `CHANGELOG.md`（Keep a Changelog 格式）、`CONTRIBUTING.md`、`SECURITY.md`、`CODE_OF_CONDUCT.md`（引用 Rust Code of Conduct）。不新增 `rustfmt.toml`。
- `docs/` 在 M6 用 mdBook 建立，按 Diátaxis 分为 Tutorials、How-to、Reference、Explanation 四部分。其中设一页“从其他框架迁移”，列出 Laravel、Rails、Django、Spring、Express/NestJS、axum 与 OpenRust 概念的对照。其他框架的词汇只出现在这一页，不进入 API。另设一页 coming-from-axum。

### 6. 开箱即用

- `openrust new` 默认生成可以直接跑通的完整样例：注册和登录、一个 scaffold 资源、一封邮件、一个 job，以及请求测试。配合 `compose.yaml`，`openrust dev` 一条命令即可启动。
- `doctor` 检查配置、数据库连接和迁移状态。`routes` 输出每条路由的方法、路径和 handler。

## 明确不改

`App` / `AppBuilder` / `AppState` / `Application`、`Resource` trait、`Validated`、`Inject`、`Policy`、`Job`、`Event`、`.env` 与 dotenvy、`compose.yaml`、feature 名（`db-postgres`、`test-util`）、`openrust-cli` 的二进制名 `openrust`、`thiserror` / `anyhow` 的分工。这些名称已经符合 Rust 惯例或跨语言通用。

## 影响

- **已实现代码**（0.0.0，可以做破坏性变更）：`openrust-core`（plugin、app、command、config）、`openrust-http`（resource、routes、plugin、extract）、门面 prelude、示例、各 crate README、测试。
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
