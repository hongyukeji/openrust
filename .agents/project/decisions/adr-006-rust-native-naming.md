# ADR-006 Rust 生态命名与目录约定

- 状态：Accepted
- 日期：2026-10-25

## 背景

OpenRust 的定位是 Rust 原生框架，但当前代码、设计文档与元数据里还留着 Laravel / Rails / Spring 的术语：`ServiceProvider::register/boot`、`index/show/store/update/destroy`、`make:*`、`config:show` 冒号命令、`resources/mail`、`Mailable`、`Seeder`、`Cache::put/forget/remember`、`storage.disk()`，Cargo `keywords` 里还有 `laravel` / `rails`。其中不少词在 Rust 社区并不通用，Rust 开发者需要额外“翻译”。另有几处命名违反 Rust API Guidelines（下文简称 RAG），或与 std / axum 的惯用语义相反。

开发者已定：资源动作改为 `list/get/create/update/delete`；其余待定项由开发者授权按 Rust 生态约定俗成的做法决定（见「待定项结论」）。

审计依据：RAG（C-CASE、C-CONV、C-GETTER、C-WORD-ORDER、C-CTOR）；std / `http::Extensions` 的 `get`/`insert` 语义；axum（`Router::nest/merge`、`State`、`Extension`、`FromRef`）；bevy（`Plugin::build`、`App::add_plugins`）；actix-web（`web::resource`、`web::Data`）；sea-orm-cli（`migration/` crate、`generate entity`）；sqlx-cli / diesel_cli（`DATABASE_URL`、`migrate run`、空格分隔子命令）；askama / tera / minijinja（`templates/`）；moka（`insert/get/invalidate/get_with`）；OpenDAL（`read/write/delete/presign_read`）；cargo-leptos / cargo-dist（`[package.metadata.*]`）；loco-rs 本身就是 Rails 风格，只作反例或参照，不当作共识依据。

## 决策

1. 公共 API 遵循 RAG 和 std / axum 语义。同一概念已有 Rust 通用词时用通用词，不沿用 PHP/Ruby/Java 框架术语。
2. 已经符合 Rust 习惯的名字不改，即使它在别的生态里也存在（`App`、`Resource`、`Validated`、`Job`、`Policy`、`middleware`、`.env` 等）。
3. 运行期命令改用 clap / cargo 风格：空格分隔的嵌套子命令加 kebab-case，不用冒号分组。
4. 生成项目的布局对齐 sea-orm-cli 和模板引擎的默认约定：`migration/` 独立 crate、`templates/`、项目元数据放在 `[package.metadata.openrust]`。
5. 对外表述不再写“inspired by Laravel and Rails”。与其他框架的概念对照移到 `docs/` 的迁移指南，不进设计原则。

## 待定项结论

本节结论优先于下方变更清单中的备选写法。

| 项 | 结论 | 依据 |
| --- | --- | --- |
| `ServiceProvider { register, boot }` | `Plugin { build, finish }`，`AppBuilder::plugin()`，`HttpProvider` → `HttpPlugin` | bevy：先对所有插件调用 `build`，全部完成后再调用 `finish`，与原两阶段语义一致 |
| 环境变量 | `OPENRUST_ENV`、前缀 `OPENRUST__`；数据库连接同时识别标准 `DATABASE_URL` | Rocket `ROCKET_*`、loco `LOCO_ENV`；sqlx / diesel / sea-orm-cli 读 `DATABASE_URL` |
| `Routes::group` | 改为 `Routes::nest`（空前缀仍 merge） | axum `Router::nest` |
| `controllers/`、`models/` | `handlers/`、`entities/` | axum handler 术语；SeaORM `generate entity` 惯用输出 |
| `Seeder` / `Factory` | 去掉；`src/seed.rs` 普通 async fn + `fake::Dummy` | Rust 测试数据惯例 |
| `Mailable` | `trait Email`，产出 `lettre::Message` | lettre 术语 |
| `storage.disk()`、`SessionGuard` / `TokenGuard` | 不在本 ADR 定名；`openrust-storage` / `openrust-auth` 实现时对齐所选底层库（OpenDAL / object_store、axum-login）的词汇，并在对应 ADR 记录 | 底层库未最终选定，先定原则 |
| `Inject<T>` | 保留 | `shaku_axum::Inject` 先例；备选名均与 tower / axum / actix / bevy 既有类型冲突 |
| `docs/` | M6 以 mdBook 建立；AGENTS.md 注明在此之前用户文档写在 crate `README.md`（rustdoc） | mdBook 是 Rust 官方文档工具 |
| `config/app.toml` | 改为 `config/default.toml` | config-rs 层级惯例 |

## 变更清单

### (A) 已实现代码，需要立即改（0.0.0，可破坏性变更）

| 当前 | 建议 | 依据 | 影响文件 | 置信度 |
| --- | --- | --- | --- | --- |
| `Action::{Index,Show,Store,Update,Destroy}`，`ResourceActions::{index,show,store,update,destroy}` | `Action::{List,Get,Create,Update,Delete}`，`ResourceActions::{list,get,create,update,delete}` | 已定；同 Google AIP-131~135 标准方法与 K8s/gRPC 动词 | `openrust-http/src/resource.rs`、`routes.rs` doctest、`tests/resource.rs`（约 30 处）、`tests/router.rs`、`openrust-http/README.md`、`openrust/src/lib.rs` prelude 文档、`todo.md:9`（`#[openrust::resource]` 宏函数名） | 高 |
| `ServiceProvider { register, boot }`、`AppBuilder::provider()`、`HttpProvider` | `Plugin { build, start }`（或 `build`/`finish`）、`AppBuilder::plugin()`、`HttpPlugin`；`<Capability>Provider` 命名规则改为 `<Capability>Plugin` | bevy `Plugin::build(&self, &mut App)` 加 `finish/cleanup` 的两阶段模型是 Rust 社区最熟悉的扩展单元；`register/boot` 是 Laravel ServiceProvider 原词 | `openrust-core/src/provider.rs`、`app.rs`、`lifecycle.rs`、`lib.rs`；`openrust-http/src/provider.rs`；facade prelude；README；`conventions.md` 命名表；`framework-design.md`；ADR-001 文中术语 | 中 |
| `AppState::get -> Result`、`AppState::try_get -> Option` | `get -> Option<Arc<T>>`，`try_get -> Result<Arc<T>>`（或 `require`） | std/`http::Extensions::get` 返回 `Option`；`try_*` 在 std 中表示返回 `Result`（`try_from`、`RefCell::try_borrow`）。现状语义与 std 相反 | `openrust-core/src/app.rs`、`provider.rs` doctest、`openrust-http/src/extract.rs` | 高 |
| `AppBuilder::provide(service)` / `AppBuilder::service::<T>()` | `AppBuilder::insert(service)` / `AppBuilder::get::<T>()`，与 `AppState` 对齐 | `http::Extensions::insert/get`、bevy `insert_resource`；同一注册表两端用同一组动词 | `app.rs`、`lifecycle.rs`、`openrust-http/README.md` 示例、设计文档 | 中 |
| `ServiceNotRegistered`（错误类型） | `MissingServiceError` | RAG C-WORD-ORDER：错误类型以 `Error` 结尾（`ParseIntError`、`RecvError`） | `openrust-core/src/app.rs`、`lib.rs`、facade re-export | 高 |
| `Command::name()` 返回 `config:show`；文档示例 `queue:work`、`db:seed` | `config show`（嵌套子命令）；命名规则改为空格分组加 kebab-case | cargo / sqlx-cli（`sqlx migrate run`）、sea-orm-cli（`migrate up`）、diesel（`migration run`）都用空格嵌套子命令；冒号是 Artisan/Rake 写法 | `openrust-core/src/command.rs`（含 trait 文档）、`app.rs:218` 注释、`lifecycle.rs`、`core/core.md` | 高 |
| 环境变量 `APP_ENV`、前缀 `APP__` | `OPENRUST_ENV`、`OPENRUST__`（或保留 `APP_` 但另加 `OPENRUST_ENV` 别名）；同时识别标准 `DATABASE_URL` | Rocket `ROCKET_PROFILE`/`ROCKET_*`、loco `LOCO_ENV`；sqlx/diesel/sea-orm-cli 都读 `DATABASE_URL`；`APP_ENV` 是 Laravel 原名，同机多应用时容易冲突 | `openrust-core/src/config.rs`（常量与文档）、`lifecycle.rs`、`framework-design.md` 配置节、`context.md` | 中 |
| `Routes::group(prefix, f)` | `Routes::nest(prefix, f)`（空前缀时仍 merge）；或保留 `group` 并注明对应 axum `nest` | axum `Router::nest/merge`；`group` 是 Laravel `Route::group` | `routes.rs`、`tests/router.rs`、README、设计文档 | 中 |
| 示例与设计里用户结构体名为 `struct App`，与 prelude 导出的 `openrust::App` 同名 | 模板里用户类型改为项目名（如 `struct Blog`），或 `prelude` 不导出 `App` | 同名遮蔽能编译，但文档里 `App` 指两个类型，读者分不清 | `openrust/examples/hello.rs`、`framework-design.md` 内核节、`openrust/src/lib.rs` | 中 |
| 框架 `config::AppConfig`（`[app]` 段）与设计中业务配置 `src/config.rs: AppConfig` 同名 | 业务模板改用 `Config`（`type Config = Config;`）或 `BlogConfig` | 避免同一项目内两个 `AppConfig` | `framework-design.md` 布局与内核节 | 中 |
| `Cargo.toml` `keywords = [..., "laravel", "rails"]` | `["framework", "web", "axum", "batteries-included", "backend"]` | crates.io 关键词用于检索 Rust 能力，不用于类比其他框架 | 根 `Cargo.toml` | 高 |
| `README.md`：“takes its developer-experience inspiration from Laravel and Rails”；Goals 中的 `openrust make:*` | “Built on tokio, axum, tower and SeaORM; conventions are expressed as Rust types and traits.”；`openrust generate …` | 对外定位用 Rust 原生表述 | `README.md`、`openrust-cli/README.md:11` | 高 |
| `pub mod http { pub use openrust_http::*; }` | `pub use openrust_http as http;` | 门面 crate 惯用写法（`bevy::prelude` / `tokio` re-export 方式），文档路径更清楚 | `openrust/src/lib.rs` | 低 |

### (B) 设计文档与计划（未实现 crate）

| 当前 | 建议 | 依据 | 影响文件 | 置信度 |
| --- | --- | --- | --- | --- |
| 生成器 `openrust make:model|controller|...` | `openrust generate <kind>`（别名 `g`） | sea-orm-cli `generate entity`、diesel `migration generate`、`cargo generate`；`make:` 是 Artisan 写法 | `framework-design.md` CLI 节、`gen1-plan.md`、README、`core/cli.md` | 高 |
| 应用命令 `db:seed` `db:reset` `queue:work` `queue:failed` `queue:retry` `schedule:run` `schedule:list` `route:list` `key:generate` `user:create` | `db seed` `db reset` `queue work` `queue failed` `queue retry` `schedule run` `schedule list` `routes`（同 `cargo loco routes`）`secret generate` `user create`；建议应用二进制内部也用 clap 解析 | 同 A 中命令行条目 | `framework-design.md`、`gen1-plan.md`、`plan.md` | 高 |
| `src/controllers/` 与 “Controller” 术语 | `src/handlers/`；文档统称 handler | axum 官方把处理函数称为 handler，zero2prod 等常见 axum 项目用 `routes/`/`handlers/`；controller 是 MVC/Rails 词汇 | `framework-design.md` 布局、路由节、Laravel 映射表 | 中 |
| `src/models/` | `src/entities/`（sea-orm-cli `generate entity` 的惯用输出），业务 `impl` 同文件或放 `services/` | SeaORM 文档与 CLI 的 Entity 术语 | 布局、数据层节 | 中 |
| `src/migrations/` | 独立 workspace 成员 `migration/`（`migration/src/lib.rs` 中 `Migrator`、`m20260101_000001_create_users.rs`） | `sea-orm-cli migrate init` 默认生成，loco 同样采用；可单独 `cargo run -p migration` | 布局、`.migrations(migrations::all)` 示例、ADR-002 | 高 |
| `src/seeders/` + `Seeder` trait + `Factory` trait（`UserFactory::new().count(3)`） | `src/seed.rs`（`db seed` 入口，普通 async fn）；测试数据用 `fake` crate 的 `Dummy` derive 加 builder，不另造 `Factory` trait | Rust 测试数据惯例是 `fake::Dummy` / builder；Seeder/Factory 是 Laravel 类名 | 布局、测试工具节、`openrust-db` 规划 | 中 |
| `resources/mail/` 邮件模板 | `templates/mail/` | askama、tera、minijinja 示例的默认目录都是 crate 根下的 `templates/` | 布局、邮件节 | 高 |
| `src/mails/` + `Mailable` trait | `src/mail/` + `trait Email`（产出 `lettre::Message`），`Mailer` 保留 | lettre 的 `Message`/`Transport` 术语；`Mailable` 是 Laravel 专有词 | 邮件节、`openrust-mail` 规划、`conventions.md` | 中 |
| `openrust.toml` 项目元数据 | 根 `Cargo.toml` 的 `[package.metadata.openrust]` | cargo-leptos `[package.metadata.leptos]`、cargo-dist、cargo-deb 均用 metadata 表；少一个根文件 | 布局、`openrust-cli` 规划 | 中 |
| `config/app.toml` + `config/<env>.toml` | 保留，或把 `app.toml` 改为 `default.toml` | config-rs 层级示例使用 `config/default.toml` + `config/{run_mode}.toml` | `config.rs`、布局 | 低 |
| `Cache::put/forget/remember` | `insert/remove/get_or_insert_with`，保留 `get/increment` | moka `insert/invalidate/get_with`；std `HashMap::insert/remove`、`Option::get_or_insert_with` | 缓存节、`openrust-cache` 规划 | 高 |
| `storage.disk("public").put/get/delete/url/temporary_url` | `storage.operator("public")`（或 `bucket`），方法 `write/read/delete/url/presign_read` | OpenDAL `Operator::write/read/presign_read`；`disk` 是 Laravel Filesystem 术语 | 存储节、`openrust-storage` 规划 | 中 |
| `impl Handle for Job { async fn handle(self, ctx) }` | 把方法并入 `trait Job { async fn run(self, ctx: &JobContext) }` | apalis / fang 以 `run`/`execute` 为主；单独 `Handle` trait 是 Laravel `handle()` 的影子 | 队列节、`openrust-queue` 规划 | 中 |
| `SessionGuard` / `TokenGuard` | `SessionAuth` / `TokenAuth`（或 axum-login 的 backend 术语） | axum-login `AuthSession`/`AuthnBackend`；Guard 是 Laravel Auth 术语 | 认证节、ADR-004 | 低 |
| `TestApp::boot()` | `TestApp::spawn().await` | zero2prod `spawn_app`、axum-test `TestServer::new`；`boot` 是 Laravel 生命周期词 | 测试工具节 | 中 |
| `Error` 变体列表含 `UnprocessableEntity`，实现为 `Validation` | 设计文档改为与代码一致 | 文档漂移 | `framework-design.md` 错误模型节 | 高 |
| “Laravel 概念映射”一节、“文档站信息架构对齐 Laravel” | 改名“与其他框架的概念对照”并移入 `docs/` 迁移指南；信息架构参考 Rust 官方书籍与 Bevy Book：Getting Started → Guides → Reference → Deployment | 设计原则不以外部框架为基准 | `framework-design.md`、`conventions.md:53`、`gen1-plan.md:11` | 高 |
| 模型生成 DSL `title:string published_at:datetime?` | 保留语法，类型名对齐 Rust/SeaORM（`String`、`Text`、`Option<DateTimeUtc>`），或同时接受两种写法 | 与生成代码中的类型对应 | CLI 节 | 低 |

### (C) 需要补的元文件

| 文件 | 建议 | 依据 | 置信度 |
| --- | --- | --- | --- |
| `CHANGELOG.md`（根目录，单一） | Keep a Changelog 格式，带 `## [Unreleased]`；git-cliff 用 keepachangelog 模板生成 | ripgrep、cargo-dist 用根目录 CHANGELOG；本仓库版本已统一为 `workspace.package.version`，不必每个 crate 一份 | 高 |
| `CONTRIBUTING.md` | 面向人类贡献者：构建与验证命令、提交格式、双许可声明，链接 `AGENTS.md` | tokio、axum、bevy、rust-analyzer 均有 | 高 |
| `SECURITY.md` | 写明私下报告渠道（GitHub Security Advisories），0.1 前补齐 | tokio、rustls、hyper 均有；框架涉及认证与会话 | 中 |
| `CODE_OF_CONDUCT.md` | 直接引用 Rust Code of Conduct | tokio、axum 的做法 | 中 |
| `docs/` | M6 以 mdBook 建立；在此之前 AGENTS.md 注明用户文档写在 crate `README.md` | 消除悬空引用 | 中 |
| `rustfmt.toml` | **不新增**。edition 2024 会被 cargo fmt 自动识别，默认风格就是社区共识；`imports_granularity` 等仍是 nightly 选项 | rust-analyzer、tokio 用默认风格 | 高 |
| README 徽章 | 首次发布后加 crates.io、docs.rs、CI、MSRV 1.92、license 徽章 | axum、tokio README 惯例 | 低 |
| `.github/` PR/Issue 模板 | 可选，暂缓 | — | 低 |

## 不改的项与理由

| 项 | 理由 |
| --- | --- |
| workspace `crates/*`、`resolver = "3"`、`xtask/`、`examples/` | 与 rust-analyzer、bevy、ripgrep 的布局一致；xtask 是 matklad 提出的通行模式 |
| crate 名 `openrust-*`（kebab-case）与 `openrust-cli` → 二进制 `openrust` | 同 tokio-*、sea-orm-cli、diesel_cli → `diesel`。RAG C-CASE 不建议给 crate 加 `-rs`/`-rust` 前后缀，但 `openrust` 是整体品牌名，不是 `rust-` 前缀；名称已按 ADR-005 预留 |
| `Application` / `AppBuilder` / `App` / `AppState` | bevy、actix 都用 `App`；Builder 模式符合 RAG；`AppState` 是 axum 文档的标准叫法 |
| `Resource` trait 与 `Routes::resource` | actix-web 有 `web::resource(path)`；“resource”是 REST 通用词，不是 Rails 专有 |
| `Validated<T>` | 同 axum-valid `Valid<T>`、axum_garde `WithValidation`，是 Rust 社区的提取器形态 |
| `Inject<T>` | 保留。Rust 已有先例 `shaku_axum::Inject`；备选名 `Service<T>`、`Extension<T>`、`Data<T>`、`Res<T>` 分别与 tower、axum、actix、bevy 的既有类型冲突 |
| `Created<T>` / `NoContent` / `PageParams { page, per_page }` / `Paginated` | 类型化响应是 axum 风格；`per_page` 与 GitHub REST API 一致 |
| `AppBuilderHttpExt`、`into_router`、`to_problem`、`as_str`、`from_env`、`resource_with` | 分别符合 `*Ext` 扩展 trait 命名、C-CONV、C-CTOR 惯例 |
| `only` / `except` | 英语里自然，且不是框架专有词；不引入谓词闭包，避免过度设计 |
| `Policy`、`Job`、`Event`/`Listener`、`middleware/`、`services/`、`jobs/`、`events/`、`tests/` | 都是通用软件术语；`Policy` 在 casbin、cedar-policy、oso 中通用 |
| `.env` + dotenvy、`compose.yaml`、`Dockerfile`、`development/test/production` | sqlx、diesel、loco 都用 `.env`；其余与语言生态无关 |
| `openrust new` / `openrust dev` | 同 `cargo new`；`dev` 与 cargo-leptos `watch`、trunk `serve` 同类，语义清楚 |
| feature 名 `db-postgres`、`test-util` 等 | `test-util` 与 tokio 一致；kebab-case 符合 Cargo 惯例 |
| `Error` 枚举变体、`thiserror`/`anyhow` 分工 | 符合 RAG C-GOOD-ERR 与社区分工 |
| `ValidationErrors` 与 `validator::ValidationErrors` 同名 | 门面内不会同时 glob 导入，暂不改；用户确实冲突时再考虑 `FieldErrors`（低） |

## 影响范围

- 公共 API：`openrust-core`（provider、app、command、config 常量）、`openrust-http`（resource、routes、provider、extract）、门面 `openrust` prelude。0.0.0 尚未发布，没有下游用户。
- 测试：`crates/openrust-http/tests/{resource,router}.rs`、core 单元测试、全部 doctest。
- 文档：根 `README.md`、各 crate `README.md`（即 rustdoc）、`.agents/project/{conventions,context,todo,plan,architecture}.md`、`core/{core,http,cli,facade}.md`、`plans/{framework-design,gen1-plan}.md`、ADR-001/002/004 中的术语。
- 元数据：根 `Cargo.toml` `keywords`。

## 迁移步骤

1. `refactor`（代码）：(A) 全部条目，包括资源动作、`Plugin`、`AppState` / `AppBuilder` 注册表动词、`MissingServiceError`、`config show`、`OPENRUST_ENV` / `OPENRUST__` / `DATABASE_URL`、`nest`、`config/default.toml`、keywords、README 定位、门面 re-export、示例类型名。同步全部调用方、测试、crate README、`.agents/project`。
2. `docs`：`framework-design.md` / `gen1-plan.md` / `conventions.md` / `context.md` 按 (B) 与「待定项结论」改写；Laravel 映射表删除，迁移指南留待 M6 `docs/`。
3. `chore`：新增 (C) 中 `CHANGELOG.md`、`CONTRIBUTING.md`、`SECURITY.md`、`CODE_OF_CONDUCT.md`，AGENTS.md 标注 `docs/` 于 M6 建立。

## 验证

- 代码簇：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo doc --workspace --no-deps`。
- 旧名扫描，结果应为 0：`rg -n --hidden -g '!target' -g '!.git' -e 'Action::(Index|Show|Store|Destroy)' -e '\.(index|show|store|destroy)\(' -e 'ServiceProvider' -e 'ServiceNotRegistered' -e '[a-z]+:(show|work|seed|list|run|generate)\b' -e 'make:' -e 'APP_ENV' -e 'resources/mail' -e 'Mailable' -i -e 'laravel|rails'`。迁移指南中刻意保留的对照词除外。
- 文档簇：`git diff --check`，以及相对链接可解析。
- 新增测试：`AppState::get` 返回 `None` 与 `try_get` 返回 `MissingServiceError` 各一条，用于发现语义回退；`config show` 命令可被 `run_command("config", ["show"])` 或等价形式调用。
