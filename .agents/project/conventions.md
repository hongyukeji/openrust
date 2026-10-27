# 工程规范

> 受众：开发者与 AI Agent
> 更新：2026-10-26
> 摘要：OpenRust 框架代码、测试、文档与 Git 提交规范。

## 代码

- Edition 2024；MSRV 写入 workspace `rust-version`，为当前 stable 减两个小版本，CI 单独验证。
- 所有 crate `#![forbid(unsafe_code)]`；库 crate `#![deny(missing_docs)]`，公共入口类型附带可运行 doctest。
- `[workspace.lints]` 启用 `clippy::all` + `clippy::pedantic`；显式 `#[allow]` 必须注明原因。库 crate 拒绝 `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented`；测试与 `cli` 二进制可放宽 `unwrap_used` / `expect_used`。
- 错误：库 crate 用 `thiserror`，`anyhow` 只出现在二进制边界；所有面向 HTTP 的错误最终进入 `openrust::Error`。
- 异步统一 tokio；静态分发 trait 优先原生 `async fn`，需要对象安全或异构注册时按既有 contract 使用 `BoxFuture`；跨任务 future 保证 `Send`，不为形式统一重写已定公开接口。
- 宏只做派生与属性标注，不改变控制流；宏支持代码放 `__private` 模块并 `#[doc(hidden)]`。
- feature 只做加法；已实现的名称以 `Cargo.toml` 为准，如 `db-postgres`、`db-mysql`、`auth`、`queue`、`queue-redis`、`http-client`、`mail-smtp`、`sms-twilio`、`encryption`、`signing`、`test-util`。计划中的 `storage-s3`、`otel`、`metrics` 等在能力落地时加入，不预建空 feature。
- Rustdoc、代码/配置注释和 CLI 解释文字遵循 [语言规范](style.md#语言规范)：简体中文优先，技术标识符保留原名；从生成器源头保证生成说明使用中文。
- 不写 `TODO` 注释；活跃任务进 `todo.md`。注释只解释非显然的原因、触发边界、失败语义或安全约束。

## 命名

- 应用层可见配置、表名、模板、部署示例及内部 package 采用中性名称；仅根 `openrust`、公共路径和 CLI 命令保留产品身份。源码交付和内部包命名以 [ADR-039](decisions/adr-039-source-workspace-distribution.md) 为准。按 [ADR-006](decisions/adr-006-rust-native-naming.md) 与 [ADR-016](decisions/adr-016-neutral-application-namespaces.md) 执行。

| 对象 | 规则 | 示例 |
| --- | --- | --- |
| 主框架与内部工具包 | 根 `openrust` 内按 module 划分；工具 package 与目录同名，所有 package 显式 `publish = false` | `cli/` → `cli`、`macros/` → `macros`、`testing/` → `testing` |
| 仓库内应用 | `apps/<name>`，包名使用应用职责名，`publish = false`；业务依赖本地 workspace | `apps/server` 的 package/二进制目标均为 `server`；保留现有宿主验收职责 |
| driver 类型 | `<Backend><Capability>` | `RedisCache`、`DatabaseQueue` |
| 配置结构 | `<Capability>Config` | `QueueConfig` |
| 错误类型 | `<Capability>Error` | `QueueError` |
| 插件 | `<Capability>Plugin` | `DatabasePlugin` |
| 提取器 | 名词或形容词 | `CurrentUser`、`Validated<T>` |
| crate 内部布局 | `lib.rs` / 复杂配置模块 `config/mod.rs` / `error.rs` / `plugin.rs` / `drivers/` / `testing.rs` | — |

## 框架规范

权威来源：[ADR-006](decisions/adr-006-rust-native-naming.md) 、[ADR-007](decisions/adr-007-aggregate-server-entry.md) 与 [ADR-008](decisions/adr-008-api-conventions.md)。本节是动手前的检查清单，与 ADR 冲突时以 ADR 为准。

### API 速查

- 资源路径用复数、kebab-case 名词，JSON API 挂在 `/api`；字段与枚举用 `snake_case`。
- 单资源返回统一包装的 `data` 对象；分页集合用 `Paginated<T>` 的 `data` 数组与强类型 `PageMeta`。
- 时间使用 RFC 3339 UTC（`Z`）；错误 JSON 以 `timestamp` 字段携带毫秒精度时间字符串，成功 JSON 不包含此诊断字段。JSON 成功与错误均为 `application/json` 的 `ApiResponse<T>` 包装，正文 `request_id` 与响应头 `x-request-id` 一致。CORS 预检（`OPTIONS` 且带 `Access-Control-Request-Method`）只返回协议响应头，成功与拒绝时正文均为空，不包装。
- `[api] http_status` 默认 `always_ok`（包装响应一律 200），可选 `semantic`；状态码与稳定 `code` 对照见 [ADR-008 §4](decisions/adr-008-api-conventions.md#4-错误)。

### 能力接入 apps/server

- 每个已实现的新能力在同一功能簇内接入 `apps/server`，并补聚合应用集成测试；这是能力完成定义的一部分。
- 无需外部服务的默认能力可直接装配；加密等需要显式密钥的可选能力仍按 feature/config 启用。外部服务能力须验证 feature 转发、插件/迁移注册及实际运行入口，不以声明 feature 代替装配。
- 当前 `server` 正式依赖只包含 `openrust`，不承载业务资源。后续若按 ADR-021 移除或迁移宿主，先为每项启动/测试职责提供等价入口和通过证据，再更新完成定义中的入口引用。

### 起名前三问

1. Rust 生态（axum、Tower、Rocket、actix-web、Poem、Salvo、Pavex、Bevy、SeaORM）有惯例吗？优先采用成熟惯例；跨框架能力对标按 [框架参考与对标](plans/framework-design.md#框架参考与对标) 的职责分工进行。
2. 没有共识时，跨语言开发者最熟悉的 Web 框架词是什么？例如 controllers、models、migrations、jobs、mailers、middleware。
3. 这个词是否自造，或者只属于单一生态？例如 `Mailable`、`ServiceProvider`、`boot`、`Guard`、`disk`、`Fairing`、`Operator`。是的话，不用。

ADR-006 未覆盖的公共名称，先在 ADR-006 补一行并写明先例，再写代码。

### 两层分工

| 层 | 目标 | 参照 |
| --- | --- | --- |
| 生成项目布局、CLI、配置文件 | Rust 惯例清晰，跨语言开发者易理解 | v1 仅按 Loco、Laravel、Goravel、OpenSpring 的顺序综合评估；v2 再按原顺序增加若依、芋道，见 [阶段参考规则](plans/framework-design.md#框架参考与对标)；采用通用词，不照搬单一生态术语 |
| 库 API（类型、trait、方法） | Rust 惯用 | std、axum、tower、[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) |

v1 新增通用机制的范围与验收以 [底层补齐计划](plans/v1-foundation-completion.md) 为准。可选启用的能力仍须完整实现与验证；优先扩展已有模块、配置、生命周期和错误契约，不为每项机制新增 package。生成器模板通过应用级覆盖定制，测试数据通过显式 factory/fixtures 构造，不隐藏持久化或覆盖用户文件；实际 API 和选型须先形成 contract。

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
| 环境变量 | 应用默认 `APP_ENV` / `APP_<SECTION>__<KEY>`，启动前可定制命名空间（ADR-016）；同时识别 `DATABASE_URL` / `REDIS_URL` / `PORT` | `APP_SERVER__PORT=8080` |
| 配置文件 | `config/default.toml`、`config/{env}.toml`，格式为 TOML | — |
| 本地敏感配置 | `.env` 仅作本地/非生产环境覆盖；提交 `.env.example` 占位模板，所有 `.env` 与 `.env.*` 默认忽略，例外仅为 `*.example`；不要把真实凭据写入被跟踪配置文件 | `cp .env.example .env` |
| 项目元数据 | 写入 `[package.metadata.openrust]` | 不另建元数据文件 |
| 生成项目目录 | `controllers/`、`models/`（含 `_entities/`）、`services/`、`policies/`、`jobs/`、`events/`、`mailers/`、`middleware/`，按实际能力生成，外加 `src/seed.rs`、`migration/` crate；本地化邮件复用 i18n catalog | 见 ADR-006 §1 |
| 邮件与队列 | `Email`；`Job::NAME` / `Job::handle(&self, JobContext) -> BoxFuture<'_, Result<()>>` | 现有公开入口；见 ADR-006 §4 与 ADR-037 |
| 缓存与测试目标 | `Cache` 方法按 ADR-014；`TestApp` 的 builder/HTTP API 待 M6 contract | 不把未实现的示意调用当作现有 API |
| 其他框架的概念对照 | 用户迁移说明放 `docs/` 对照页；设计参考与取舍放 ADR/框架计划，公共 API 采用已定通用名称 | 不把参考框架术语直接当 OpenRust API |

## 验证

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
# M6：对已有源码版本做 API 兼容性比较；先明确源码 baseline，再执行 semver 检查。
```

| 改动范围 | 验证 |
| --- | --- |
| 任意 crate 源码 | 上述命令 + 该 crate 单元测试 |
| `openrust::db` / `openrust::queue` / `openrust::auth` 等数据模块 | 另在 PostgreSQL 与 MySQL 各跑集成测试，见 `memory/testing.md` |
| 模板与生成器 | 双库模板快照测试：生成项目后 `cargo check` + `cargo test` |
| 文档-only | `git diff --check` + 相对链接检查 + `rg` 旧路径扫描 |
| 生成项目模板 | 双库模板快照测试；检查真实 `.env` / `.env.*` 忽略且 `.env.example` 可跟踪，生成项目后 `cargo check` + `cargo test` |

测试原则：框架是底层通用机制，每个公共行为都应有单元或集成测试；新建测试文件前必须能一句话说清它能发现哪个缺陷。能力 fake 可随该模块的 `test-util` 暴露；通用测试组合工具放在 `testing`，不得为目录统一引入根 package 对测试包的反向依赖。分目录迁移测试时核对 Cargo targets 和运行用例，避免子目录文件未被编译执行。

### 构建产物与临时资源

- 本节适用于开发者与 AI Agent。同一功能簇内可复用构建缓存，但不能等到功能簇收尾才首次检查空间：开始任一构建前、每个构建/测试命令结束后及长验证矩阵的每个阶段边界，都要检查仓库内所有 `target/` 总量和所在磁盘剩余空间，并在验收记录注明结果。仓库全部 `target/` 软上限为 10 GiB、硬上限为 20 GiB；所在磁盘可用空间低于总量 30% 为软告警，低于 20% 为硬告警。发现软告警时先停止继续扩展验证矩阵，检查并清理确认归属本任务的临时产物；若本仓库 target 非空，结束当前命令后立即完整清理，再评估能否运行下一条必要的定向门禁。达到硬告警时不启动任何新构建，包括定向测试；先停止本任务构建、确认无并发使用者并清理本仓库完整 target，只有磁盘余量恢复到 20% 以上才可继续。磁盘容量不会因清空仓库 target 恢复到软阈值时，暂停构建并报告，不能删除仓库外或归属不明的数据。每个功能簇验证结束后仍须执行 `cargo clean --dry-run` 复核再 `cargo clean`，清空可再生产物；临时源码副本和独立示例的 target 单独核算、单独清理。每次清理后记录实际回收量、仓库 target 总量及磁盘剩余空间。不得等磁盘耗尽或 target 达硬上限才处理；清理失败/被占用时说明路径和原因，不因此重跑整组测试。开发者操作入口见 [贡献指南](../../CONTRIBUTING.md#构建空间管理)。
- 临时生成应用、干净源码副本及其 target 放在任务专属临时目录，记录创建归属，成功、失败或取消时均回收。诊断需要暂留时说明路径和保留原因；不得删除其他任务的目录。临时源码验证默认不把 `CARGO_TARGET_DIR` 指向仓库共享 target，避免一次性包名与 feature 组合长期堆积。
- `cargo clean` 只清理当前仓库配置的 Cargo target，不清理 Cargo 下载缓存；清理后下一次构建会重新编译。仓库 target 仅在当前功能簇的多轮门禁之间复用；达到软告警时不得继续堆叠验证任务，功能簇结束后必须清理，不能长期保留。清理前复核范围并记录回收量。
- 删除前检查真实路径、符号链接、Git 跟踪状态和任务归属，确认目标只含生成数据；检查 Cargo、rustc、rustdoc 及关联构建进程。清理共享目录时还须使用已验证与当前 Cargo 兼容的构建锁，持锁期间保留锁文件；无法取得锁或发现其他使用者时延后，不能只凭一次进程快照判断安全。锁文件是否存在不代表锁是否被占用。
- 不清理仓库外的全局 Cargo 缓存，也不按文件名哈希或修改时间猜测删除 `deps`，不为清理擅自修改全局 Cargo 配置、profile 或关闭开发者的增量编译。每簇收尾仅对明确归属本仓库且经 dry-run 复核的 target 执行完整 `cargo clean`；先检查并发构建锁和实际回收量，并说明下一次构建会重建哪些产物。目标为临时副本或独立示例时，只清理该副本/示例自己的 target。
- Cargo 的下载缓存回收不负责回收 target 构建产物；本规则是开发者与 AI Agent 的任务收尾责任，不代表 Cargo 已安装自动清理钩子。依据：[构建缓存](https://doc.rust-lang.org/cargo/reference/build-cache.html)、[cargo clean](https://doc.rust-lang.org/cargo/commands/cargo-clean.html)、[全局缓存回收](https://doc.rust-lang.org/cargo/reference/config.html#global-caches)。

## 文档

- 默认文档入口使用简体中文，英文仅作必要术语对照；完整英文文档成熟后再评估。范围与例外统一见 [语言规范](style.md#语言规范)，翻译需保持内容语义、链接和可运行示例。

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
