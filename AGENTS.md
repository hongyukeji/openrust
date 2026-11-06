# AGENTS.md — AI Agent 协作契约

> 适用：OpenRust 框架仓库。本文件是 AI 编程 Agent 在本仓库工作的最高入口规则。项目事实、技术栈、规范与当前状态在 `.agents/project/`，本文件不重复。

适用对象：Codex / Claude Code / Cursor / 其他 AI 编程 Agent。

## 当前设计授权

对外定位为 **OpenRust — Laravel 的 Rust 实现**，Laravel Modules 是默认集成的优秀第三方扩展，不与 Laravel 并列为项目品牌。实现范围不变：用 Rust 1:1 实现固定基线 Laravel 框架能力与 laravel-modules 模块化能力，并持续跟进上游。目录、概念、命名、默认行为、工作流和扩展方式以 Laravel 为准；仅在 Rust 语言、运行时或工具约束确有必要时适配，并记录原因与等价行为证据。上游跟进与版本范围规则见 [总计划](.agents/project/plans/laravel-rebuild.md#长期上游跟进)。

1:1 以逐项功能与可观察语义、默认值、错误/安全边界、Container/Provider 生命周期、扩展契约和 CLI/生成/测试/部署流程为验收；Laravel 使用者可按官方章节完成对应操作，仅 Rust 语法、类型、所有权、async 等确有必要时适配。不是 PHP 运行环境或字节兼容。每项须具备上游源码/测试→Rust API/配置→等价行为测试→modules/example→文档→真实服务证据；名称近似、旧测试或目录相似不算完成。适用核心能力不得因难度、个人偏好或“Rust 惯用”被划为不适用。差异按[对应与分类规则](.agents/project/plans/laravel-rebuild.md#逐项对应与差异分类)公开记录，无法等价保留缺口，不虚报严格 1:1 已达成。

用户已明确授权固定范围内Laravel核心、laravel-modules及官方可选生态全量对应开发，分阶段完成，验收语义始终以官方为准。可选表示开发者按需启用，不表示Agent可省略实现；官方工具自身必要界面/前端配套属于阶段二官方可选包，不等同暂不开发的若依/芋道式业务后台。范围、阶段及Rust必要适配见[官方全量交付规则](.agents/project/plans/laravel-official-coverage.md#全量交付授权与两阶段)，唯一todo按依赖领取；不得仅凭Tokio、同名API或上游PHP实现判定已对应或不适用。

已有优秀能力也必须通过 Laravel 对应机制实现，例如统一返回格式使用 Response / API Resource 与异常渲染的公开扩展。保留能力价值，不保留违背目标的旧 API 或独立体系；仅移动目录、套 Provider 或改名不算融合。这些是已授权的通用增强，不限于语言适配，不得因上游没有相同内置能力就删除；具体边界见 [ADR-001](.agents/project/decisions/adr-001-framework-principles.md)。

[ADR-001](.agents/project/decisions/adr-001-framework-principles.md) 与 [全面重构计划](.agents/project/plans/laravel-rebuild.md) 是当前目标：Laravel 优先、参考 Goravel，允许全面重写目录/API/默认值/实现，无历史兼容义务，保留 Rust 语言与工具优势。旧规定不能阻止已批准的重构；已实现事实和真实验收记录仍须保留。工程质量、WIP 保护和 §5 对外授权边界继续适用。当前任务与后续顺序只在 `todo.md` 领取。

每项 Spec、实现和审查由 Agent 主动对照固定官方源码、默认、生命周期、公开扩展与开发流程；Goravel 仅供跨语言参考，Rust 适配须有具体必要约束和等价证据。现有代码、旧决策或旧测试不能为偏离背书。发现不符主动整改并映射唯一 todo，不能只改名、套 Provider、换目录或自创机制后称 1:1 完成。报告分别标明已编码、对标差距、已验证、已提交，未闭合保留缺口；编码优先和后置运行验证不豁免事先 contract 对照及静态自审。

当前目标布局以 [ADR-001](.agents/project/decisions/adr-001-framework-principles.md) 为准；业务代码已迁至 `modules/example/`，R2-M 的具体验收以当前 todo、源码和 Git 为准，不能将目录相似当作完整能力验收：Laravel 应用职责直接位于根；根应用与框架使用独立 Cargo package：框架目标为 `vendor/openrust/framework/src/lib.rs`，包根保留 `Cargo.toml`、`README.md` 与测试；根 `src/main.rs` 是默认 server 薄入口，default-members=["."]、default-run="server"，保留 `openrust::` 导入及直接 `cargo run`。当前源码尚未迁移，按唯一 todo 的独立迁移簇验收；此前同 package 与平铺 lib.rs 方案已被替代。第一方可选扩展位于 `vendor/openrust/<扩展>/`，依赖方向为应用→扩展→框架及应用→框架，不允许反向依赖或两套框架。根 `app/` 支持 Laravel 正常应用开发职责（Controller、Model、Provider 等）；`modules/<模块名>/` 是可选且可并存的业务组织，两者复用唯一 Container / Provider / routes 机制。示例专有迁移、种子、模板、协议、生成脚本与测试 fixture 全留模块内；框架核心不放示例 build.rs / build 依赖或业务特判。唯一 modules/example 是供开发者参考的普通业务模块；根 app 按 Laravel 职责承载应用代码，bootstrap 承担应用启动装配，app/mod.rs 只作必要 Rust 模块导出；模块经统一 Provider / 模块清单与公开接口接入，只用公开框架 API；不包含完整子应用骨架，不新增 example feature，不保留 demo/example 双应用或根 examples 目录。新布局迁移与验收以当前 todo、源码和 Git 为准。

默认应用配置目标为可编辑的 `config/*.rs`，`.env` 提供部署值；已编译配置函数在 dotenv 加载后向同一 ConfigRepository / Provider 生命周期求值，模块同机制。Rust 源码需显式 mod / manifest 编译，不运行期扫描执行或热编译。旧默认 TOML 是临时实现，整改后仅作显式可选 source；缓存遇不可序列化类型 / 工厂必须明确拒绝，不静默丢弃。管理员 DBSettings 仍是独立领域。

2026-11-05 真人明确纠正：旧 OpenRust MFA 代码与数据均不兼容；移除旧独立体系及一次性/长期兼容、迁移、重绑、接管分支。有效通用安全能力经固定 Fortify 公开机制融合，不删除正常认证或有效安全回归；不将此授权扩大为清空共享数据库或丢弃其他WIP。

最新真人决定：框架、根默认应用与未显式选择JWT的生成路径，以对应固定Laravel官方默认、生命周期和公开机制为准；Fortify原生Web/Session是其对应模式默认，不将所有场景一律Session或一律JWT。API+Bearer JWT是本批完整必交的显式选配，开发者选择后全程无Session/Cookie/CSRF-cookie/remember/隐藏Session/Set-Cookie或Cookie身份兜底。两种传输共用唯一UserProvider/Guard/Fortify三字段/Actions；旧auth-mfa兼容仍删除。此决定覆盖此前JWT为根默认、一律禁中心Cookie等中间口径，不改写历史验收。

## 1. 角色与契约

| 角色 | 职责 |
| --- | --- |
| 开发者 | 方向确认、需求确认、代码审查、发布与对外动作 |
| AI Agent | 规格起草、代码实现、测试验证、记忆维护、变更说明 |

Agent 不替代开发者做方向决策、发布判断和对外动作；开发者不替代 Agent 维护 `.agents/project/`。开发期 Agent 直接推进任务，不按代码风险类别反复评估暂停；除 §5 的授权边界外按 §6 工作循环连续执行。

## 2. 权威来源

信息冲突时按优先级判断：

1. 代码、Git 状态、`Cargo.toml`（事实）
2. `AGENTS.md`（本文件）
3. `.agents/project/README.md`（目录索引）
4. `.agents/project/context.md`（当前上下文与固定约束）
5. `.agents/project/todo.md`（活跃待办）
6. `.agents/project/plan.md`（当前里程碑与路由）
7. `.agents/project/architecture.md` / `conventions.md` / `stack.md`
8. `.agents/project/decisions/`（ADR）
9. `.agents/project/core/<crate>.md`（各 crate 当前事实）
10. `.agents/project/plans/`（v1 版本计划与框架设计）
11. `docs/`（面向用户的正式文档）
12. 聊天记录

不依赖聊天历史或口头记忆；执行前以仓库文件和 Git 状态为准。

## 3. 启动读取

每次会话先读：`AGENTS.md`、`.agents/project/README.md`、`context.md` 的固定约束与本任务相关段落、`todo.md` 的本任务待办、`plan.md` 的当前里程碑。已注入或本会话已读且未变化的内容不重复读取；长文件先用 `rg -n` 定位。

领取或切换功能簇、上下文压缩后恢复、收到新需求时，先核对 Git/WIP、`todo.md` 与 `plan.md`「执行校准」。已完成的根 `src/` 迁移是旧架构事实；新的目录/API 目标按 ADR-001 重新设计，不沿用旧 M0–M6 排序。相关计划有变化时读取最新差异；不得沿用旧提示词中的范围或阶段替代仓库最新要求。

| 任务类型 | 补读 |
| --- | --- |
| 架构 / crate 边界 / 公共 API 设计 | `architecture.md`、`plans/laravel-rebuild.md`、唯一 ADR |
| 编码 / 命名 / 测试 / 提交 | `conventions.md` |
| 依赖 / 版本 / feature | `stack.md` |
| 某个 crate 的实现 | `core/<crate>.md` + 该 crate 的 `README.md` 与源码 |
| 里程碑范围与验收 | `plans/laravel-rebuild.md` |
| 文档写作 | `style.md` |
| 最近验证 | `memory/testing.md` / `memory/verification.md` |

正式用户指南位于 `docs/`，以 mdBook 按固定 Laravel 导航逐步建立；README 和 Rustdoc 分别承担入口补充与 API 参考。首批章节不代表全量文档迁移完成。不全量读取 `docs/`、`target/`、`Cargo.lock`。项目 skills 位于 `.agents/skills/<name>/SKILL.md`，按任务名称与描述匹配后再读正文。

## 4. `.agents/project/` 目录约定

`.agents/project/` 是 Agent 长期记忆目录，由 Agent 自维护，开发者只读：

```text
.agents/project/
├── README.md         # 目录索引
├── style.md          # 文档写作与命名规范
├── context.md        # 当前版本、阶段、约束、对外动作
├── todo.md           # 活跃待办（按里程碑分组）
├── plan.md           # 当前里程碑、路由、验证入口
├── progress.md       # 当前交付与验证摘要
├── architecture.md   # crate 分层与依赖规则
├── conventions.md    # 代码 / 测试 / 文档 / Git 规范
├── stack.md          # 技术栈与版本政策
├── roadmap.md        # v1 版本之后的方向
├── decisions/        # 唯一方向决策 ADR-001
├── core/             # 每个 crate 一文件的当前事实
├── plans/            # v1 版本计划与框架设计
├── memory/           # 测试与验收基线
└── scripts/          # 提交脚本
```

`docs/` 面向用户，`.agents/project/` 面向 Agent，`.agents/skills/` 保存可复用工作流，三者不混用。`.agents/` 根目录不新增其他文件。

## 5. 授权边界与不可逆操作

代码改动本身（删 API、重构、改 Schema、安全逻辑）开发期按目标推进，不为风险类别暂停。以下操作需要开发者明确授权：

- **项目目录之外的写操作**：系统目录、用户 home（含 `~/.cargo` 全局安装、`~/.gitconfig`）、其他仓库。只读访问与 `/tmp` 临时文件不受限。`cargo install` 属于全局写操作。
- **Git 不可逆操作**：force push、删除远程分支、改写已推送历史、修改 CI 配置以跳过验证。推送 `main` 前确认工作区干净、验证通过。
- **对外动作**：创建 GitHub 仓库、发布 crates.io、打 tag、发 release。
- **删除**：`rm -rf` 前确认目标无未提交工作；删除受版本控制的文件走 `git rm`。

编码/测试同类失败时，由 AI Agent 自行诊断并作出最优处理方案：保留失败证据，复核根因、目标 contract、实现和验证方式，必要时调整策略或引入独立审查，修复后执行与影响匹配的验证。不得机械重复重试、删除有效断言或降低质量门禁，也不得仅因达到固定失败次数停下请求开发者判断；只有触及本节明确授权边界、需求方向真正模糊、信息必须由开发者提供或存在无法自行解除的外部阻塞时才询问。模型/网络等运行器临时故障由守护在三次密集恢复后转冷却监测并通知，不因此永久退出或持续密集重试。拿不准方向时先用 5-10 行 Spec 说明意图再动手。

## 6. 工作循环

本轮最新顺序：先收齐当前已在途 main 组合源码、调用方、模板、唯一示例和说明，冻结后一次集中适用验收，修复失败并通过后本地提交；不再追加新簇延长本批，也不以全计划最后测试无限推迟提交。随后处理今日尚未吸收的整改，再按 ADR-001 持续推进。未验证源码保持 WIP，旧证据不冒充本批通过；默认20%额度保留；构建产物合计80GB软/100GB硬、磁盘剩余8%软/5%硬，取更严格条件。

当前执行覆盖：先完成当前在途组合，冻结后集中适用验收并本地提交，再处理今日剩余整改和ADR001后续；大功能簇完整编码及配套后再测试，不逐小改动重复矩阵。未验证源码保持WIP，不报交付。

每个功能簇采用 **Spec → Implement → Verify → Commit**。默认先完成代码与配套测试代码，再在功能簇提交前集中执行验证，不逐小改动反复测试；分支交付与main组合门禁分层，具体见[集中验证](.agents/project/conventions.md#功能簇提交前集中验证)。验证未通过不得提交或报完成。

### 6.1 Spec

复杂或多步任务先写 5-15 行：要改什么、为什么、怎么验证、影响哪些 crate 与公共 API。Spec 是理清思路的工具，不是请求确认。轻量任务可省略。

### 6.2 Implement

- 一个功能簇 = 一个工程闭环，不夹带无关需求。
- 设计与重写遵循 ADR-001：以 Laravel 能力和体验为首要依据，参考 Goravel 的跨语言实现；Spec 说明 Rust 适配、复用/重写选择和验收。允许重写已验证实现，但保留并重新验证其有效行为证据。
- 公共 API 变更先评估下游（其他 crate、模板、`modules/example/`、`tests/consumers/`、`docs/`）兼容性；0.x 允许破坏性变更，但必须同步更新全部调用方、文档与 changelog 条目。
- 新增依赖必须有理由并写入 `stack.md`；主版本升级写入 `stack.md`、必要时更新 ADR-001 与 changelog。
- 遵守 `conventions.md`：`#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`、库 crate 禁止 `unwrap` / `expect` / `panic` / `todo` / `unimplemented`、不写 `TODO` 注释。
- 顺手修边界清楚、可当场验证的小问题并纳入说明；需要跨 crate 设计或改变方向的问题写入 `todo.md`。
- 优先编辑既有文件；不做未需要的抽象，不为假设的未来需求预留扩展点。
- 第一方文档、Rustdoc、代码/配置注释及生成模板说明以简体中文为主、英文为辅；新增改动立即执行，既有英文按待办整改。术语、技术标识符和来源例外见 [语言规范](.agents/project/style.md#语言规范)。
- 注释只解释非显然的原因、触发边界、失败语义或安全约束。

### 6.3 Verify

提交前自查并自纠，不把半成品上报为完成：

Laravel 对应能力须按[纠偏规则](.agents/project/plans/laravel-rebuild.md#执行偏离的纠正)核对实际生产调用链及行为证据，不能只查目录和命名。发现偏离及时修正；未修正不关闭受影响行为。结构子交付与内核/业务行为完成分别报告，不能用旧测试通过替代新 contract 验收。

1. 重读修改过的每个文件；`git diff` 与声明一致；链接与路径可解析；重命名后 `rg` 扫旧名称。
2. 按范围验证：

| 改动范围 | 验证 |
| --- | --- |
| main功能簇/集成的crate源码 | 提交前集中执行`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo doc --workspace --no-deps`；其余适用门禁不变 |
| 独立任务分支源码 | 提交前执行本簇fmt/Clippy、单元/契约与受影响consumer、必要真实服务/生成验证；完整main组合门禁在集成提交前执行，不当作分支已通过 |
| `openrust::db` / `openrust::queue` / `openrust::auth` 等数据相关模块 | 日常实际数据库验收仅真实MySQL8.4；其他数据库及最低版本运行矩阵统一发布前完整适用验收，见[当前政策](.agents/project/memory/testing.md#当前数据库运行政策) |
| 模板与生成器 | 生成项目后 `cargo check` + `cargo test` |
| 文档 / skill-only | `git diff --check` + 链接检查 + `rg` 旧引用扫描，不跑全量构建 |

3. 框架是底层通用机制，每个公共行为都应有测试；新建测试文件前必须能一句话说清它能发现哪个缺陷。测试失败先自行修复。
4. 在途命令自然结束、证据收齐后，按 [构建产物与临时资源](.agents/project/conventions.md#构建产物与临时资源) 集中清理完成簇、多feature、生成与测试的可再生产物；跨簇仅保留下一实际已安排步骤明确需要的一份缓存。确认归属/无占用后可完整清任务或共享target再按需重建，不每条命令clean、不等分支合入才清缓存；保护源码/WIP、依赖源码、服务与验收证据，记录真实前后总量和磁盘余量。

### 6.4 Commit

验证通过后直接创建本地 commit，格式与脚本以 `conventions.md`「Git 提交」为唯一来源。WIP 不 commit，但在 `todo.md` 标注状态与阻塞点；不把已完成工作长期搁置不 commit。

## 7. WIP 与完成定义

总控可按依赖图批准多个契约明确、前置满足且相互独立的功能簇并行 in_progress；每项记录负责人、分支/worktree、基线与集成状态。main 集成与最终门禁串行。功能簇完成必须满足：

- [ ] §6.3 匹配范围的验证实际执行并通过
- [ ] 公共行为变化有测试；新增或变化能力在唯一 modules/example 综合业务模块同步真实调用/公开扩展、根装配/配置及行为验证，不以文档链接或独立 consumer 替代示例覆盖
- [ ] 新能力已接入当前获准架构的可运行应用，并有公开 API、生成项目及集成测试；迁移前根 `server` 的验收责任在新结构中保留
- [ ] 受影响 `docs/`、crate `README.md` 已更新
- [ ] 受影响 `.agents/project/`（`context.md` / `todo.md` / `plan.md` / `core/<crate>.md` / 必要时 ADR）已更新
- [ ] commit 已创建且无无关 diff
- [ ] `git status` 干净，或 WIP 已明确标注

## 8. 协作模式

默认 **单开发者 + AI Agent**，开发者授予 Agent 在 §5 边界内的最高执行权限。

- Agent 回复语言跟随开发者提问语言；代码、命令、路径和专有名词保持原文。
- 结合上下文判断用户希望持续推进既有计划时（例如对多阶段任务说“持续开发”“全部处理好”“做完再告诉我”），或要求恢复/停止持续开发时，自动读取并使用 [agent-continuous-development-guard](.agents/skills/agent-continuous-development-guard/SKILL.md)，不要求用户再输入技能名。启动守护须核对实际任务身份与完整Goal；完成或手动停止后自动清理仅本任务的守护进程。中间提问/进度查询不视为停止。
- 日常决策（命名、文件组织、测试布局、小版本依赖）Agent 直接定。
- 只在以下情况停下询问：§5 授权边界、需求方向真正模糊、信息必须由开发者提供，或存在无法自行解除的外部阻塞。编码/测试失败由 Agent 按 §5 自主诊断、选择最优处理方案并继续；运行器故障按 §5 冷却监测。
- 多模型协作：用户授权优先使用 `gpt-6.1-sol` 子智能体做已定 contract 的实现/测试；架构由主控收敛，高风险改码由不同模型或 Provider 独立审查。总控按依赖图派发独立功能簇或互不冲突子任务，worker 不递归派发、不擅改 contract；后续默认同一main物理目录、多对话框按文件归属分工，不再默认新建worktree；既有worktree先保全交接再由总控串行集成main。Cargo manifest/lock、迁移、共享文档、构建及集成门禁由集成负责人统一调度；提前分配文件、target 和服务资源，审查可并行只读。分支交付不等于 main 已验收；创建、交接与合并后清理遵循[并行开发与集成](.agents/project/conventions.md#并行开发与集成)。模型不可用时如实记录并选成本合适的可用方案。
- **长期 Git 工作方式**：框架保持同一源码仓库与 main 集成主线；用户已持续授权总控将已授权框架范围的本地任务串行整合到 main，无需每个分支再次确认。先整合本批源码、解决冲突，再集中执行适用组合验收，通过后创建本地集成提交并安全退役任务分支/worktree；分支通过不等于 main 通过，保护 WIP、单一索引负责人和来源祖先或等价映射。当前默认共享main目录多对话框协作，既有任务分支/worktree先安全交接退役，后续不默认新增，不创建额外源码仓库；已通过有限簇由总控及时提交并按真人持续授权安全推送 main，核验远端对象；不 force push、不发布/tag。原V2版本计划已取消，需求转为[可选快速开发模块](.agents/project/plans/optional-rapid-development-modules.md#开发与使用边界)，暂不开发；R14通过或持续Goal“全部完成”均不授权启动。main只维护通用框架与唯一示例，不强制集成后台业务或UI；上述暂不开发业务模块的开工及分支/交付组织另由用户明确指定，不为它们预建分支或默认合并；已授权框架任务分支不受此冻结限制。AI产品仍未启动，既有单独授权边界保持。

## 9. 记忆维护

| 变化类型 | 更新位置 |
| --- | --- |
| 用户可见行为、配置、命令 | `docs/`、crate `README.md` |
| 阶段、约束、对外动作 | `context.md` |
| 待办增删 | `todo.md` |
| 里程碑进度、路由 | `plan.md`、`progress.md` |
| crate 当前事实 | `core/<crate>.md` |
| 方向决策 | `decisions/adr-001-framework-principles.md`；领域实现取舍见相应计划 / core |
| 依赖与版本 | `stack.md` |
| 测试入口、验收事实 | `memory/testing.md`、`memory/verification.md` |
| 可复用工作流 | `.agents/skills/<name>/SKILL.md` |

不在共享文件追加历史日志；审计走 Git。

## 10. 禁止事项

- 不删文件除非用户明确要求或走 `git rm`
- 不引入未在 `stack.md` 记录的新依赖
- 不写 `TODO` 注释；不写聊天恢复提示进正式文档
- 不在 `docs/` 写执行流水；不在 `.agents/project/` 写最终用户教程
- 不擅自发布、打 tag、创建远端仓库
- 不跳过 §6.3 验证直接 commit
- 不在未授权时写入项目目录之外的持久化路径
- 不把其他项目的规则、模块名或业务概念带入本仓库
