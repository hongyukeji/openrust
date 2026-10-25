# 工程规范

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 框架代码、测试、文档与 Git 提交规范。

## 代码

- Edition 2024；MSRV 写入 workspace `rust-version`，为当前 stable 减两个小版本，CI 单独验证。
- 所有 crate `#![forbid(unsafe_code)]`；库 crate `#![deny(missing_docs)]`，公共入口类型附带可运行 doctest。
- `[workspace.lints]` 启用 `clippy::all` + `clippy::pedantic`；显式 `#[allow]` 必须注明原因。库 crate 拒绝 `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented`；测试与 `openrust-cli` 二进制可放宽 `unwrap_used` / `expect_used`。
- 错误：库 crate 用 `thiserror`，`anyhow` 只出现在二进制边界；所有面向 HTTP 的错误最终进入 `openrust::Error`。
- 异步统一 tokio；trait 中使用原生 `async fn`，需要跨任务时加 `Send` 约束。
- 宏只做派生与属性标注，不改变控制流；宏支持代码放 `__private` 模块并 `#[doc(hidden)]`。
- feature 只做加法；命名 `db-postgres` / `db-mysql` / `queue-redis` / `cache-redis` / `storage-s3` / `mail-smtp` / `templates` / `otel` / `metrics` / `test-util`。
- 不写 `TODO` 注释；活跃任务进 `todo.md`。注释只解释非显然的 WHY、触发边界、失败语义或安全约束。

## 命名

| 对象 | 规则 | 示例 |
| --- | --- | --- |
| crate | `openrust-<capability>` | `openrust-queue` |
| driver 类型 | `<Backend><Capability>` | `RedisCache`、`DatabaseQueue` |
| 配置结构 | `<Capability>Config` | `QueueConfig` |
| 错误类型 | `<Capability>Error` | `QueueError` |
| 插件 | `<Capability>Plugin` | `DatabasePlugin` |
| 提取器 | 名词或形容词 | `CurrentUser`、`Validated<T>` |
| crate 内部布局 | `lib.rs` / `config.rs` / `error.rs` / `plugin.rs` / `drivers/` / `testing.rs` | — |

## 框架规范

权威来源：[ADR-006](decisions/adr-006-rust-native-naming.md) 与 [ADR-007](decisions/adr-007-aggregate-server-entry.md)。本节是动手前的检查清单，与 ADR 冲突时以 ADR 为准。

### 能力接入 apps/server

- 每个已实现的新能力在同一功能簇内接入 `apps/server`，并补聚合应用集成测试；这是能力完成定义的一部分。
- 不需要外部服务的能力始终启用；需要外部服务的能力按 ADR-007 添加 feature 并转发给门面。当前 M1 不设空 feature。
- `openrust-server` 的正式依赖只包含门面 `openrust`，不承载业务资源。

### 起名前三问

1. 主流 Rust 框架（loco-rs、axum、Rocket、actix-web、Poem、Salvo、Pavex、Bevy、SeaORM）有共识吗？有就照做。
2. 没有共识时，跨语言开发者最熟悉的 Web 框架词是什么？例如 controllers、models、migrations、jobs、mailers、middleware。
3. 这个词是否自造，或者只属于单一生态？例如 `Mailable`、`ServiceProvider`、`boot`、`Guard`、`disk`、`Fairing`、`Operator`。是的话，不用。

ADR-006 未覆盖的公共名称，先在 ADR-006 补一行并写明先例，再写代码。

### 两层分工

| 层 | 目标 | 参照 |
| --- | --- | --- |
| 生成项目布局、CLI、配置文件 | 跨语言开发者一眼熟悉 | loco-rs 优先，其次 Rails / Django / Laravel 共有的通用词 |
| 库 API（类型、trait、方法） | Rust 惯用 | std、axum、tower、[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) |

### 速查

| 场景 | 规则 | 示例 |
| --- | --- | --- |
| 扩展单元 | `Plugin { build, finish }`，注册用 `app.plugin(..)` | `HttpPlugin` |
| 取值方法 | `get` 返回 `Option`，`try_get` 返回 `Result`；不加 `get_` 前缀（C-GETTER） | `state.get::<T>()` |
| 转换方法 | `as_` 为廉价借用，`to_` 为有开销的转换，`into_` 消耗自身（C-CONV） | `into_router()` |
| 错误类型 | 以 `Error` 结尾 | `MissingServiceError` |
| 路由分组 | `nest`，不用 `group` / `scope` | `routes.nest("/api", ..)` |
| 资源动作 | `list` / `get` / `create` / `update` / `delete` | `Action::List` |
| CLI 子命令 | 空格嵌套，kebab-case，不用冒号 | `db seed`、`config show` |
| 生成器 | `generate <kind>`，别名 `g` | `openrust g scaffold post title:string` |
| 环境变量 | 环境用 `OPENRUST_ENV`；覆盖用 `OPENRUST_<SECTION>__<KEY>`；同时识别 `DATABASE_URL` / `REDIS_URL` / `PORT` | `OPENRUST_SERVER__PORT=8080` |
| 配置文件 | `config/default.toml`、`config/{env}.toml`，格式为 TOML | — |
| 项目元数据 | 写入 `[package.metadata.openrust]` | 不另建元数据文件 |
| 生成项目目录 | `controllers/`、`models/`（含 `_entities/`）、`services/`、`policies/`、`jobs/`、`events/`、`mailers/`、`middleware/`，外加 `src/seed.rs`、`migration/` crate、`templates/mail/` | 见 ADR-006 §1 |
| 邮件、缓存、队列与测试 | `Email`；`Cache::get/insert/remove/get_or_insert_with`；`Job::run`；`TestApp::spawn().await` | 见 ADR-006 §4 |
| 其他框架的名词 | 只允许出现在 `docs/` 的「从其他框架迁移」对照页 | — |

## 验证

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
cargo semver-checks            # 发布前
```

| 改动范围 | 验证 |
| --- | --- |
| 任意 crate 源码 | 上述命令 + 该 crate 单元测试 |
| `openrust-db` / `openrust-queue` / `openrust-auth` | 另在 PostgreSQL 与 MySQL 各跑集成测试，见 `memory/testing.md` |
| 模板与生成器 | 双库模板快照测试：生成项目后 `cargo check` + `cargo test` |
| 文档-only | `git diff --check` + 相对链接检查 + `rg` 旧路径扫描 |

测试原则：框架是底层通用机制，每个公共行为都应有单元或集成测试；新建测试文件前必须能一句话说清它能发现哪个缺陷。fake driver 的断言放在 `openrust-test`。

## 文档

- 每个 crate 的 README 即 `lib.rs` 文档（`#![doc = include_str!("../README.md")]`）。
- 正式文档在 `docs/`（mdBook，M6 建立），信息架构按 Diátaxis：Tutorials → How-to → Reference → Explanation，另设「从其他框架迁移」对照页与 coming-from-axum 页；M6 前用户文档写在 crate `README.md`。
- `.agents/project/` 写法见 [style.md](style.md)。

## Git 提交

提交使用项目脚本：

```bash
AI_AGENT_SOURCE="<Agent 名称> <模型>[ (<档位>)]" ./.agents/project/scripts/commit <type>[(scope)] "<中文主题>" "<说明>" -- <pathspec>...
./.agents/project/scripts/commit validate HEAD
```

- type：`feat` / `fix` / `docs` / `style` / `refactor` / `perf` / `test` / `build` / `ci` / `chore` / `revert`；scope 可选，建议用 crate 短名（`core` / `http` / `db` / `cli`）。
- 主题用中文描述实际变化；正文最后一个非空行必须是 `AI-Agent: <Agent 名称> <模型>[ (<档位>)]`。
- 脚本优先读 `AI_AGENT_SOURCE`，其次 `AI_AGENT`，再次 `AI_AGENT_NAME` + `AI_AGENT_MODEL`（可附 `AI_REASONING_EFFORT` / `CLAUDE_CODE_EFFORT_LEVEL`）。Claude Code 会话中 `AI_AGENT` 可能被运行环境预置为非规范值，提交时显式传 `AI_AGENT_SOURCE`。
- 新增文件先 `git add` 再用 pathspec 提交；一个功能簇一个 commit，不夹带无关改动。
- 不 force push；推送 `main` 前确认工作区干净、验证通过。
