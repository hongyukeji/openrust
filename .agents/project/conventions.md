# 工程规范

> 设计替代：目录、命名、CLI、响应/认证默认值按 [ADR-070](decisions/adr-070-laravel-first-rebuild.md) 重新设计；下文区分当前实现与新目标，不能以目标描述代替实现验收。代码安全、验证、WIP 与 Git 规则继续有效。

> 受众：开发者与 AI Agent
> 更新：2026-10-28
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

- 产品身份保留 OpenRust；应用名称、数据表、配置和目录按 Laravel 心智模型与实际职责定稿，不为保留历史前缀增加兼容层。
- 当前包/模块以 Cargo.toml 和源码为准；未来目录与默认命令按 [全面重构计划](plans/laravel-rebuild.md) R1 验证后落地。此规范不宣称新 API 已实现。
- 类型/trait 使用 PascalCase，方法/变量/模块使用 snake_case；优先保留 Laravel 用户熟悉的概念，Rust 语言语法遵循生态惯例。

## 框架规范

新目标按 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)；各功能簇先定公共 contract，再迁移所有调用方/模板/测试。旧 ADR 的行为事实作为回归输入，不能用旧命名禁令否决新设计。

### API 速查

- 目标以语义 HTTP 状态为默认；HTML、JSON Resource、redirect、stream、文件下载均有清楚语义。旧 ApiResponse/always_ok 仍是迁移前实现，不作为新接口强制规则。
- Resource 输出须支持授权、敏感字段排除、关系/分页；响应格式和错误 schema 按 R3/R5 定稿并测试。
- HTTP 协议边界保持正确：CORS、HEAD、204/304、流式响应不能套错误的统一包装；request ID/日志和错误渲染脱敏。

### 能力接入根集成宿主

- 新能力在同簇接入当前可运行应用、公共 API 测试和生成项目；迁移目录时保留原根 server 的集成验收职责。
- 外部服务和密钥能力须显式配置，实际验证 feature、provider/迁移注册与运行入口，不以声明替代装配。
- 测试、模板、示例、部署脚本和 CI 跟随目标目录迁移，不能只改框架源码。

### 普通业务模块布局

本仓目标唯一综合模块为 modules/example（当前 app/example 待迁移）；新增业务可参考 modules/<业务>，遵循 [ADR-074](decisions/adr-074-laravel-modules-directory.md)。以下是有实现才采用的职责约定，不是要求预建整棵空目录：

| 位置 | 职责 |
| --- | --- |
| mod.rs | 模块声明与接入导出，不堆放业务实现 |
| providers/ | 业务公开注册；当前使用真实 Plugin API，R2 后统一 ServiceProvider |
| routes/api.rs、routes/web.rs | 模块路由定义与组合 |
| http/controllers/ | 请求处理与业务调用 |
| http/requests/、http/resources/ | 输入 DTO/校验与输出资源 |
| models/、services/ | 实际实体与确有需要的业务编排 |
| database/migrations/、database/seeders/ | 模块专属迁移与种子 |
| resources/views/ | 模块视图与其他实际资源 |
| jobs/、events/、listeners/、notifications/ | 相应真实异步/事件业务，按需创建 |
| extensions/ 或既有 extensions.rs | 公开 driver/Plugin 扩展示例 |
| tests/ | 模块专有测试与 fixture |
| proto/、generated/ | 仅实际 gRPC 示例需要的协议与已生成代码 |

模块只保留实际业务与专属资源，不复制 app/bootstrap/main/Cargo 完整骨架。除 modules/example 外均为框架通用结构和代码；应用 Provider/模块清单及 Rust 编译声明是公开接入入口，根其余职责只提供通用加载/公开挂载，不加入样例逻辑、专有 build.rs 或 build 依赖。框架核心不反向依赖业务。

新能力在同功能簇同步 modules/example 实际调用/扩展与测试；文档或独立 consumer 不能替代综合模块覆盖。当前 Plugin 不是 Laravel ServiceProvider 等价声明，R2 收敛 register/boot 时统一迁移，不能形成双内核；provider 不替代 Cargo 依赖解析。

### 起名前三问

1. Laravel 中对应的概念和典型调用是什么，开发者能否直接找到它？
2. Goravel 如何跨语言表达，Rust 的类型/所有权/异步如何自然落地？
3. 名称和签名能否清楚表达职责、错误、资源与生命周期，而不要求用户猜隐式状态？

允许 ServiceProvider、Mailable、Factory、Seeder、Guard、disk 和冒号命令；普通 Rust 方法使用 snake_case。新增公共 contract 写入对应领域 ADR，不强制所有名称继续追加到旧 ADR-006。

### 两层分工

| 层 | 目标 | 参照 |
| --- | --- | --- |
| 应用布局、术语、配置与命令 | Laravel 使用者可快速找到入口、完成工作流 | Laravel 为首要依据，Goravel 为跨语言参考 |
| 类型、trait、资源与执行 | Rust 类型安全、显式 async、可靠并发与工具支持 | Rust/Cargo、成熟运行时与库；语言差异写明理由 |

已有能力允许复用、适配或重写，但一个职责最终保留一套公共机制。依赖和 feature 变更同步 stack.md；旧接口删除后扫描全部调用方，不为了消除编译错误丢弃行为测试。

### 速查

| 场景 | 新目标/继续有效的工程规则 |
| --- | --- |
| 生命周期 | Provider/Container 与已有 Plugin 收敛为一套内核，scope 隔离、关闭/取消/join 显式 |
| 转换方法 | as_ 借用、to_ 转换、into_ 消耗自身；Result/Option 表达失败与缺失 |
| 路由/资源 | 熟悉的 group/resource、index/create/store/show/edit/update/destroy；签名由 R3 定稿 |
| CLI/生成器 | Artisan 式 make:*、route:list、migrate:* 等，根默认 run 不加选包参数；具体实现依阶段验收 |
| 配置 | 领域配置与 .env，类型化校验、可诊断但不泄漏密钥；新默认值由领域 contract 决定 |
| 敏感文件 | .env/.env.* 默认忽略，只提交占位 .example，不放公开目录 |
| 测试 | 从 R2 起建设 TestApp，Factory/Seeder 从 R6 接入；不能把目标签名写成现有 API |
| 文档 | Laravel 概念对照、Rust 差异和可编译例子同步交付；中文优先 |

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

测试原则：只保留能发现真实回归、保护公共契约/安全边界/数据一致性/故障恢复，或验证跨模块与外部服务集成的高价值测试。按风险和独立行为选测试，不要求每个函数、字段或简单分支各写一例；相同输入、断言和失败原因的重复测试应合并。新增测试前须能用一句话说明它能发现的具体缺陷及为何现有测试发现不了；测试数量与覆盖率不是目标。能力 fake 可随该模块的 `test-util` 暴露；通用测试组合工具放在 `testing`，不得为目录统一引入根 package 对测试包的反向依赖。分目录迁移测试时核对 Cargo targets 和运行用例，避免子目录文件未被编译执行。

### 构建产物与临时资源

- 本节适用于开发者与 AI Agent。同一功能簇内可复用构建缓存，但不能等到功能簇收尾才首次检查空间：开始任一构建前、每个构建/测试命令结束后及长验证矩阵的每个阶段边界，都要检查仓库内所有 `target/` 总量和所在磁盘剩余空间，并在验收记录注明结果。仓库全部 `target/` 软上限为 30 GiB、硬上限为 50 GiB；所在磁盘可用空间低于总量 10% 为软告警，低于 5% 为硬告警。发现软告警时先停止继续扩展验证矩阵，检查并清理确认归属本任务的临时产物；若本仓库 target 非空，结束当前命令后立即完整清理，再评估能否运行下一条必要的定向门禁。达到硬告警时不启动任何新构建，包括定向测试；先停止本任务构建、确认无并发使用者并清理本仓库完整 target，只有磁盘余量恢复到 5% 以上才可继续。磁盘容量不会因清空仓库 target 恢复到软阈值时，暂停构建并报告，不能删除仓库外或归属不明的数据。每个功能簇验证结束后仍须执行 `cargo clean --dry-run` 复核再 `cargo clean`，清空可再生产物；临时源码副本和独立示例的 target 单独核算、单独清理。每次清理后记录实际回收量、仓库 target 总量及磁盘剩余空间。不得等磁盘耗尽或 target 达硬上限才处理；清理失败/被占用时说明路径和原因，不因此重跑整组测试。开发者操作入口见 [贡献指南](../../CONTRIBUTING.md#构建空间管理)。
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


## 并行开发与集成

- 总控维护唯一todo依赖图，按本机资源与额度安排多个独立簇；每项登记负责人、冻结Spec/公共接口、前置、基线commit、分支/worktree、文件范围、构建/服务资源、验收、集成与清理状态。依赖未满足只做只读审计，不重复派发现有任务。
- 在同一Git仓库创建本地任务分支（建议`tasks/<任务标识>`）与临时worktree；默认临时目录用`/tmp/openrust-worktrees/<任务标识>`，先确认路径未被占用。独立对话框或子代理均须绑定正确工作目录；不新建源码仓库、不默认给worker启动重复守护。
- 当前main未提交改动由原负责人继续闭环；不stash/reset、复制或强迁移他人WIP。新分支从明确的已提交基线派生，依赖未提交改动的任务须等待前置集成。稳定公共contract变化先由总控同步Spec与依赖，不让各分支各造接口。
- 同目录不并发写同一文件；分支间亦须约定共享核心、Cargo清单/锁、迁移编号和共享文档的归属，集成负责人协调交接。worker不递归派发、不擅改整体contract，不启动冻结产品。
- 分支worktree使用任务专属target、端口和测试数据，所有任务worktree/target纳入本项目30 GiB软上限、50 GiB硬上限总预算及磁盘阈值；Cargo/真实服务任务按CPU、内存、磁盘与额度统一调度；避免重复全量门禁或同时挤占资源。只清理本任务资源，不清其他worktree、共享target或服务。
- 分支交付具备实现、示例/生成项目、文档、适用门禁和独立审查证据后创建本地提交。总控核对工作区、基线/差异、依赖和结果，串行合入main；合入后按实际组合影响执行适用门禁，分支通过不能当main通过。冲突必须重新验证，不通过降低断言处理。
- 验证与提交完毕及时回收任务：先确认所有任务提交被main包含（保留祖先关系；改写/挑选则须等价树与commit映射证据）、无未提交/未追踪工作、无人占用且进程已结束；正常`git worktree remove`后再`git branch -d`，不使用force、`-D`或批量prune绕过检查。清理失败保留目录与原因，不丢弃工作。
- 允许删除已合入的本地临时分支、worktree和任务专属可再生缓存；不授权删除远程分支、改写已推送历史、推送、发布或tag。最终仅保留需要的main代码/文档和有效验收证据，不把工作区或流水日志加入Git。

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
