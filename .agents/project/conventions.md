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

新目标按 [ADR-070](decisions/adr-070-laravel-first-rebuild.md)；各功能簇先定公共 contract，再迁移所有调用方/模板/测试。旧 ADR 的行为事实作为回归输入，不能用旧命名禁令否决新设计。 Laravel官方有对应时，按其概念、命名、配置、默认行为、生命周期和公开扩展契约实施；官方没有时优先优秀Laravel生态，再参考成熟跨语言设计，仍接同一Laravel机制，不为下划线机械一致另造协议。用户明确批准的差异保留并分类记录，不擅自改掉。

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

### 功能簇提交前集中验证

- 默认先实现完整功能簇：源码、调用方、唯一示例、生成模板、测试代码与文档一并完成，再静态审查并登记ready_for_verify。实现期间不逐方法/文件反复执行测试、Clippy或整套门禁，不自动启动TDD；明确的类型/API编译阻断可运行一次最小必要`cargo check`诊断，记录原因，不扩成测试矩阵。用户另行明确要求TDD/回归实验时按该范围执行。
- 总控将代码稳定、写入者已交接的功能簇排入验证窗口；先fmt/廉价静态检查，再模块与新增行为定向测试，最后按变更范围执行集成、MySQL及后端专属定向/网络/生成项目与完整门禁，尽早发现廉价失败，不边改公共接口边跑整套验证。已有在途命令自然结束并收取结果，不为切换流程强杀他人构建。
- 独立任务分支提交前执行本簇适用fmt/Clippy、单元/契约测试、受影响consumer，以及必要真实后端/生成项目验证；记录未覆盖的main组合门禁，不将分支交付标成完整框架验收。main集成保持串行，在可审阅的集成结果上执行AGENTS规定的workspace fmt/Clippy/tests/docs及适用feature/MSRV/deny、MySQL及必要后端定向/真实服务和生成验证，通过后才创建集成本地提交。直接在main开发的功能簇同样提交前执行完整适用门禁，不先提交未验收代码。
- 失败后先修复，再重验失败目标与实际受影响范围；只有源码/依赖/编译feature/模板/配置或环境变化影响已通过项目时才使其失效。相同最终运行输入的有效证据可复用，无关Agent计划/进度文档变化不使Rust测试失效；必须记录输入、变更影响和证据，无法证明未受影响就重验，不沿用旧簇测试冒充新能力。快照差异须人工审源后接受，不能自动改期望来过关。
- 未通过适用验证不提交、不报完成；外部条件缺口保持未完成并继续独立项。每簇区分代码实现、验收、提交与main集成状态，汇报实际失败、等待及耗时，不以测试数作为进度。所有任务共享资源预算，禁止重复运行同一套矩阵争抢机器。
- 一个功能簇覆盖可独立交付的完整能力或紧密关联API家族；不按单个方法拆成反复验收的小簇，也不为凑大批次混入不相关能力。先完成该簇全部编码，再冻结验证输入；验收等待不阻止下一独立簇编码。
- 验证负责人一次领取完整适用检查链，按已确认的资源和顺序连续执行、失败即停并保存已通过证据；不在每个成功子阶段停下来等待模型重新派发。输入漂移、资源越界或新增授权边界才重新协调，不绕过AGENTS的三次同类失败规则。
- 每次验证保留一份最终结果索引，关联实际命令、输入、退出码、日志和资源状态；总控集中复核一次。稳定代码做一轮独立审查并批量反馈，后续只复核修复及受影响范围；不反复要求模型全文重读、重算同一证据。基线HEAD变化但实际验证输入相同的，记录对应关系，不仅因提交号变化重跑。
- 从现有日志区分编译/链接、测试执行、资源排队、交接及清理耗时；优先消除最长的可避免等待，不以新增代理数或测试数宣称提速。不另建高频监控或基准系统；只有同等输入与资源条件下的实际耗时才能支持提速结论。

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
| main功能簇/集成源码 | 提交前集中执行上述命令及受影响单元测试 |
| 独立任务分支源码 | 按本节分支限定门禁验证，main组合门禁另在集成提交前执行 |
| `openrust::db` / `openrust::queue` / `openrust::auth` 等数据模块 | 日常默认真实MySQL8.4；后端专属风险必要定向，发布前全部承诺后端完整适用验收，见 `memory/testing.md` |
| 模板与生成器 | 模板快照检查；日常普通与MySQL生成项目，后端专属例外及发布全后端另验；生成项目后 `cargo check` + `cargo test` |
| 文档-only | `git diff --check` + 相对链接检查 + `rg` 旧路径扫描 |
| 生成项目模板 | 模板快照检查；日常数据库型生成默认MySQL，后端专属例外及发布全后端另验；检查真实 `.env` / `.env.*` 忽略且 `.env.example` 可跟踪，生成项目后 `cargo check` + `cargo test` |

测试原则：只保留能发现真实回归、保护公共契约/安全边界/数据一致性/故障恢复，或验证跨模块与外部服务集成的高价值测试。按风险和独立行为选测试，不要求每个函数、字段或简单分支各写一例；相同输入、断言和失败原因的重复测试应合并。新增测试前须能用一句话说明它能发现的具体缺陷及为何现有测试发现不了；测试数量与覆盖率不是目标。能力 fake 可随该模块的 `test-util` 暴露；通用测试组合工具放在 `testing`，不得为目录统一引入根 package 对测试包的反向依赖。分目录迁移测试时核对 Cargo targets 和运行用例，避免子目录文件未被编译执行。

### 测试分层与入口整合

- 基础语义矩阵集中在快速unit/contract层；`modules/example`验证公开API、Provider/路由与持久化接线；生成项目验证模板实际编译运行；真实TCP/DB保护协议与后端独有风险。不得在每层、每后端和生成变体机械复制全部边界；Laravel行为严格对应，不要求测试文件或用例数量对应。
- 合并前核对实际Cargo入口、独有缺陷保护和失败原因。纯行为测试可按领域聚合入口并保留子模块文件；进程环境变量、独立feature、真实服务和特殊harness需要的隔离必须保留，不用整套串行执行掩盖竞争。同步Cargo、`#[path]`、feature、模板/快照及消费者，受控删除走`git rm`。
- 先做小范围试点，记录删并理由与保留隔离点，并证明关键负例/回归仍实际执行。比较相同工具链、feature与缓存条件下的文件/用例/target数及编译、链接、执行耗时；数目减少不等于覆盖不变或加速已证实。在途冻结验收不因精简重排，按唯一todo在安全交接点领取。

日常真实数据库默认MySQL8.4；后端专属风险当场定向，发布前全承诺后端统一验收。测试入口整合须保留各后端测试、feature与必要隔离，不能把未执行PG算通过或删减发布矩阵。历史证据不改写。

### 构建产物与临时资源

- 本节适用于开发者与AI Agent。总预算为全部main/worktree/consumer/生成项目的构建产物合计，50 GB软告警、100 GB硬上限（十进制）；磁盘剩余10%软告警、5%硬告警，任一触发即按更严格条件处理。100 GB是上限，不预分配或主动填满；已有缓存占用也计入，不是再增加100 GB。
- 总控在唯一todo的任务资源栏登记owner、target真实路径、复用用途、分配预算及最后检查结果；汇总时按真实路径去重，包含仓库外任务专属CARGO_TARGET_DIR，不能只统计根target。新建验证任务前评估预计新增量，已有产物加预计新增会触线时先回收或缩小本批范围，不等命令完成才发现超额。

| 时机/状态 | 动作 |
| --- | --- |
| 构建前、命令结束、长矩阵阶段边界 | 检查累计产物与磁盘剩余；长命令由本地轻量采样检查磁盘余量，不为监测启动模型轮询或另一套守护；目录体积不做高频全盘扫描 |
| 正常预算内 | 保留近期仍需使用的main和活动任务缓存，复用相同依赖/工具链/feature产物；不每簇cargo clean，不无故复制target |
| 功能簇/生成验收结束，或任务取消 | 收取证据、结束进程后回收一次性生成项目与临时target；主缓存保留；诊断暂留须有owner、理由和下次验收/交接时的复核点 |
| worktree退役 | 已合入、无WIP且无占用后，回收该任务缓存与工作区；不能为释放空间删除未交付源码或证据 |
| 累计达到50 GB或磁盘剩余低于10% | 暂停增加重型构建和新target，先清本任务已退役/一次性产物，再清已确认不再复用的完整target；正常命令先自然结束，不因软告警强杀。清理至低于50 GB且磁盘恢复10%后再扩展；无法恢复时停止新增构建并报告 |
| 累计达到100 GB或磁盘剩余低于5% | 不启动任何新构建；总控协调暂停本任务仍增长的构建，待进程释放锁后安全清理。不得中断他人进程或删除占用目录；无法安全回收即报告。恢复至软阈值以下并重新估算下一命令空间后继续 |

- 清理遵循最少重编译原则：先明确归属的临时与退役target，后确已不再需要的缓存；不因软告警一律清空活跃根target。需完整清理时先执行cargo clean --dry-run复核目标，按下文锁与归属规则执行；记录回收量、剩余总量及磁盘余量。临时日志/证据须有界，保留可复验摘要，不把日志无限追加当缓存；不因此放宽验收或改写已有通过证据。清理延后说明占用者与解除条件，代码实现和无资源冲突的工作继续。
- 临时生成应用、干净源码副本及其 target 放在任务专属临时目录，记录创建归属，成功、失败或取消时均回收。诊断需要暂留时说明路径和保留原因；不得删除其他任务的目录。临时源码验证默认不把 `CARGO_TARGET_DIR` 指向仓库共享 target，避免一次性包名与 feature 组合长期堆积。
- `cargo clean` 只清理当前仓库配置的 Cargo target，不清理 Cargo 下载缓存；清理后下一次构建会重新编译。仓库target可在正常空间阈值内跨簇复用；达到软告警时停止扩展验证并按本节清理，不允许缓存无限增长。清理前复核范围并记录回收量。
- 删除前检查真实路径、符号链接、Git 跟踪状态和任务归属，确认目标只含生成数据；检查 Cargo、rustc、rustdoc 及关联构建进程。清理共享目录时还须使用已验证与当前 Cargo 兼容的构建锁，持锁期间保留锁文件；无法取得锁或发现其他使用者时延后，不能只凭一次进程快照判断安全。锁文件是否存在不代表锁是否被占用。
- 不清理仓库外的全局 Cargo 缓存，也不按文件名哈希或修改时间猜测删除 `deps`，不为清理擅自修改全局 Cargo 配置、profile 或关闭开发者的增量编译。仅在阈值处置、缓存退役或任务工作区安全回收时，对明确归属且经dry-run复核的target执行完整`cargo clean`；先检查并发构建锁和实际回收量，并说明下一次构建会重建哪些产物。目标为临时副本或独立示例时，只清理该副本/示例自己的 target。
- Cargo 的下载缓存回收不负责回收 target 构建产物；本规则是开发者与 AI Agent 的任务收尾责任，不代表 Cargo 已安装自动清理钩子。依据：[构建缓存](https://doc.rust-lang.org/cargo/reference/build-cache.html)、[cargo clean](https://doc.rust-lang.org/cargo/commands/cargo-clean.html)、[全局缓存回收](https://doc.rust-lang.org/cargo/reference/config.html#global-caches)。

## 文档

- 默认文档入口使用简体中文，英文仅作必要术语对照；完整英文文档成熟后再评估。范围与例外统一见 [语言规范](style.md#语言规范)，翻译需保持内容语义、链接和可运行示例。

- 每个 crate 的 README 即 `lib.rs` 文档（`#![doc = include_str!("../README.md")]`）。
- 正式文档在 `docs/`（mdBook，M6 建立），信息架构按 Diátaxis：Tutorials → How-to → Reference → Explanation，另设「从其他框架迁移」对照页与 coming-from-axum 页；M6 前用户文档写在 crate `README.md`。
- `.agents/project/` 写法见 [style.md](style.md)。


## 并行开发与集成

- 不设人为固定的并行任务、对话框或分支数量上限；总控持续从唯一todo选取前置满足、contract明确且可隔离的任务，按实际空闲槽位扩展并行，不把当前两个任务当作配额。单个总控的子智能体槽位不足时，可由总控通过正式Paseo接口新建独立执行对话框，不把该父会话槽位上限当作整个项目的并行上限；服务商/宿主的真实并发限制、模型额度及CPU/内存/磁盘仍须遵守。优先复用或正常结束已完成代理，不终止有效在途任务。
- Paseo只使用当前仓库对应的既有OpenRust项目，不为任务目录/worktree注册独立项目。先从正式project列表核对项目ID与根路径，再用`paseo workspace create --project <现有项目ID> --isolation worktree ...`在该项目下创建隔离工作区；核对返回workspace/project/cwd与`git common-dir`确属当前仓库。Paseo管理的工作区路径以正式返回值为准，不强行套临时目录约定。
- 项目管理目标为OpenRust下main与tasks任务分组同级，任务分支沿`tasks/<任务标识>`；tasks是显示组织，不是把不同分支的执行者放进一个共享checkout。若正式Paseo接口支持分组则使用，未核实支持时将同项目各独立workspace标题统一为`tasks/<任务标识>`，对话框注明实现/审查职责；名称前缀不冒称真正嵌套分组。不为显示层级新增项目、源码tasks目录或改Paseo内部数据库。
- 独立Paseo执行对话框由总控统一创建、登记和收取结果；核对已安装CLI的`--help`后，使用`paseo run --workspace <该项目任务workspaceID> --provider codex --model gpt-6.1-sol --no-wait --json <任务说明>`，不要仅传`--cwd`而继承父workspace或自动注册新项目。创建后再次核对真实project/workspace/cwd绑定。明确单一子任务、文件范围、原总控ID/预算约束、禁止递归派发及独立完整Goal/重复守护；记录实际agent ID、分支/worktree和状态，轮询或等待不产生重复派发。完成后收取commit/证据，由总控集成；对话框无在途任务后正常归档（不硬删除），再按下文清理任务专属worktree/分支。总控暂停/预算停止时须协调停止其派发的执行任务，不让独立对话框脱离原任务预算继续消费。
- 总控在worker交接、审查结论、门禁完成、槽位释放或阻塞变化时主动收取结果、更新唯一todo、安排后续验证/集成并填补ready任务，不等待用户催促。架构协调会话每次进度检查核对真实提交、执行者、资源与清理状态，发现偏离时给具体提醒；相同未变化状态不反复发消息，不为跟进新建高频模型轮询或重复Goal/守护。
- 总控维护唯一todo依赖图，按本机资源与额度安排多个独立簇；每项登记负责人、冻结Spec/公共接口、前置、基线commit、分支/worktree、文件范围、构建/服务资源、验收、集成与清理状态。依赖未满足只做只读审计，不重复派发现有任务。
- 总控保持两条可重叠的工作流：已冻结簇验证/集成时，前置满足的下一独立簇在隔离worktree实际编码，不只派只读分析。提前确认文件与公共接口边界；需要在途未提交实现的任务仍等待前置，不假定接口就绪。代码就绪后排队重验证，不能为增加并行数复制整套构建缓存。
- 创建临时资源时即登记owner、路径、复用范围与清理条件，结束后按现有锁和归属规则收尾，减少事后反复追查。正常阈值内复用有效构建缓存，避免频繁切换feature/编译配置引发重复构建；不为释放下一簇编码而提前删除仍可复用的缓存。main提交与索引始终单一owner串行，文档更新按短窗口集中处理，不反复打断源码验证。
- 在同一Git仓库创建本地任务分支（建议`tasks/<任务标识>`）与临时worktree；默认临时目录用`/tmp/openrust-worktrees/<任务标识>`，先确认路径未被占用。独立对话框或子代理均须绑定正确工作目录；不新建源码仓库、不默认给worker启动重复守护。
- 若既有任务误登记为独立Paseo项目，先通知原owner并保全分支/WIP/会话/证据；有正式归属迁移接口时优先使用，否则由总控在交接点将已提交工作接到当前项目的正确workspace并验证新执行者绑定，再正常归档旧会话/工作区。不得直接project delete（可能连同workspace一起删除）或修改Paseo内部数据库强制改归属；旧项目登记未安全收尾时保持可见并报告。
- 当前main未提交改动由原负责人继续闭环；不stash/reset、复制或强迁移他人WIP。新分支从明确的已提交基线派生，依赖未提交改动的任务须等待前置集成。稳定公共contract变化先由总控同步Spec与依赖，不让各分支各造接口。
- 同目录不并发写同一文件；分支间亦须约定共享核心、Cargo清单/锁、迁移编号和共享文档的归属，集成负责人协调交接。worker不递归派发、不擅改整体contract，不启动冻结产品。
- 分支worktree使用任务专属target、端口和测试数据，所有任务worktree/target纳入本项目50 GB软告警、100 GB硬上限总预算及磁盘阈值；Cargo/真实服务任务按CPU、内存、磁盘与额度统一调度；避免重复全量门禁或同时挤占资源。只清理本任务资源，不清其他worktree、共享target或服务。
- 验证资源按实际冲突分配，不设全项目Cargo独占锁：相同target、同一清单/锁文件写入、冲突端口或测试数据必须串行；独立target且服务数据隔离的轻量检查/定向测试可在CPU、内存与磁盘预算允许时并行。重链接、全特性和生成项目大构建错峰，避免通过复制target换取表面并行。资源不足时只暂停争用资源的步骤，代码实现、调用方迁移、静态审查和文档继续。
- 验证窗口在实际进程结束并收取结果后立即释放；存在ready任务且资源可用时，总控直接启动下一项，不等待固定汇报周期或另一次用户催促。等待须注明具体占用者/任务句柄、资源及解除条件；不得以其他簇尚未提交为由禁止无依赖任务完成实现。先定向验证已知失败的fixture/快照/行为，再执行剩余矩阵；共享文档变更不触发无关构建。
- 分支交付具备实现、示例/生成项目、文档、适用门禁和独立审查证据后创建本地提交。总控核对工作区、基线/差异、依赖和结果，串行准备可审阅的集成结果，执行main完整组合门禁通过后才创建集成提交，不先生成未验收的合并提交；分支通过不能当main通过。冲突必须重新验证，不通过降低断言处理。
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
