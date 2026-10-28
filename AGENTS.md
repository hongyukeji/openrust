# AGENTS.md — AI Agent 协作契约

> 适用：OpenRust 框架仓库。本文件是 AI 编程 Agent 在本仓库工作的最高入口规则。项目事实、技术栈、规范与当前状态在 `.agents/project/`，本文件不重复。

适用对象：Codex / Claude Code / Cursor / 其他 AI 编程 Agent。

## 当前设计授权

长期产品定位为 **用 Rust 实现 Laravel，并持续跟进 Laravel 框架功能演进**。目录、概念、命名、默认行为、工作流和扩展方式以 Laravel 为准；仅在 Rust 语言、运行时或工具约束确有必要时适配，并记录原因与等价行为证据。上游跟进与版本范围规则见 [总计划](.agents/project/plans/laravel-rebuild.md#长期上游跟进)。

保留并融合本框架已有优秀设计，例如统一返回格式，放入 Laravel 对应职责与公开扩展位置；这是已授权的通用增强，不限于语言适配。不得因上游没有相同内置能力就删除；具体边界见 [ADR-070](.agents/project/decisions/adr-070-laravel-first-rebuild.md#用户意图与验收原则)。

[ADR-070](.agents/project/decisions/adr-070-laravel-first-rebuild.md) 与 [全面重构计划](.agents/project/plans/laravel-rebuild.md) 是当前目标：Laravel 优先、参考 Goravel，允许全面重写目录/API/默认值/实现，无历史兼容义务，保留 Rust 语言与工具优势。旧规定不能阻止已批准的重构；已实现事实和真实验收记录仍须保留。工程质量、WIP 保护和 §5 对外授权边界继续适用。当前任务与后续顺序只在 `todo.md` 领取。

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

领取或切换功能簇、上下文压缩后恢复、收到新需求时，先核对 Git/WIP、`todo.md` 与 `plan.md`「执行校准」。已完成的根 `src/` 迁移是旧架构事实；新的目录/API 目标按 ADR-070 重新设计，不沿用旧 M0–M6 排序。相关计划有变化时读取最新差异；不得沿用旧提示词中的范围或阶段替代仓库最新要求。

| 任务类型 | 补读 |
| --- | --- |
| 架构 / crate 边界 / 公共 API 设计 | `architecture.md`、`plans/framework-design.md`、相关 ADR |
| 编码 / 命名 / 测试 / 提交 | `conventions.md` |
| 依赖 / 版本 / feature | `stack.md` |
| 某个 crate 的实现 | `core/<crate>.md` + 该 crate 的 `README.md` 与源码 |
| 里程碑范围与验收 | `plans/gen1-plan.md` |
| 文档写作 | `style.md` |
| 最近验证 | `memory/testing.md` / `memory/verification.md` |

`docs/` 于 M6 以 mdBook 建立；在此之前，用户文档写在各 crate 的 `README.md`。不全量读取 `docs/`、`target/`、`Cargo.lock`。项目 skills 位于 `.agents/skills/<name>/SKILL.md`，按任务名称与描述匹配后再读正文。

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
├── decisions/        # ADR，每决策一文件 adr-NNN-<topic>.md
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

同类失败连续 3 次：停下说明现象、已尝试方案、推测原因，请开发者判断。拿不准方向时先用 5-10 行 Spec 说明意图再动手。

## 6. 工作循环

每个功能簇采用 **Spec → Implement → Verify → Commit**。

### 6.1 Spec

复杂或多步任务先写 5-15 行：要改什么、为什么、怎么验证、影响哪些 crate 与公共 API。Spec 是理清思路的工具，不是请求确认。轻量任务可省略。

### 6.2 Implement

- 一个功能簇 = 一个工程闭环，不夹带无关需求。
- 设计与重写遵循 ADR-070：以 Laravel 能力和体验为首要依据，参考 Goravel 的跨语言实现；Spec 说明 Rust 适配、复用/重写选择和验收。允许重写已验证实现，但保留并重新验证其有效行为证据。
- 公共 API 变更先评估下游（其他 crate、模板、`examples/example/`、`tests/consumers/`、`docs/`）兼容性；0.x 允许破坏性变更，但必须同步更新全部调用方、文档与 changelog 条目。
- 新增依赖必须有理由并写入 `stack.md`；主版本升级写 ADR 或 changelog。
- 遵守 `conventions.md`：`#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`、库 crate 禁止 `unwrap` / `expect` / `panic` / `todo` / `unimplemented`、不写 `TODO` 注释。
- 顺手修边界清楚、可当场验证的小问题并纳入说明；需要跨 crate 设计或改变方向的问题写入 `todo.md`。
- 优先编辑既有文件；不做未需要的抽象，不为假设的未来需求预留扩展点。
- 第一方文档、Rustdoc、代码/配置注释及生成模板说明以简体中文为主、英文为辅；新增改动立即执行，既有英文按待办整改。术语、技术标识符和来源例外见 [语言规范](.agents/project/style.md#语言规范)。
- 注释只解释非显然的原因、触发边界、失败语义或安全约束。

### 6.3 Verify

提交前自查并自纠，不把半成品上报为完成：

1. 重读修改过的每个文件；`git diff` 与声明一致；链接与路径可解析；重命名后 `rg` 扫旧名称。
2. 按范围验证：

| 改动范围 | 验证 |
| --- | --- |
| 任意 crate 源码 | `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo doc --workspace --no-deps` |
| `openrust::db` / `openrust::queue` / `openrust::auth` 等数据相关模块 | 另在 PostgreSQL 与 MySQL 各跑集成测试 |
| 模板与生成器 | 生成项目后 `cargo check` + `cargo test` |
| 文档 / skill-only | `git diff --check` + 链接检查 + `rg` 旧引用扫描，不跑全量构建 |

3. 框架是底层通用机制，每个公共行为都应有测试；新建测试文件前必须能一句话说清它能发现哪个缺陷。测试失败先自行修复。
4. 整组验证结束后按 [构建产物与临时资源](.agents/project/conventions.md#构建产物与临时资源) 检查并清理本任务产物；每个功能簇结束后默认完整清理本仓库 Cargo target。共享目录被占用时延后清理并说明，不中断其他构建。

### 6.4 Commit

验证通过后直接创建本地 commit，格式与脚本以 `conventions.md`「Git 提交」为唯一来源。WIP 不 commit，但在 `todo.md` 标注状态与阻塞点；不把已完成工作长期搁置不 commit。

## 7. WIP 与完成定义

同一时刻只有一个功能簇 in_progress。功能簇完成必须满足：

- [ ] §6.3 匹配范围的验证实际执行并通过
- [ ] 公共行为变化有测试
- [ ] 新能力已接入当前获准架构的可运行应用，并有公开 API、生成项目及集成测试；迁移前根 `server` 的验收责任在新结构中保留
- [ ] 受影响 `docs/`、crate `README.md` 已更新
- [ ] 受影响 `.agents/project/`（`context.md` / `todo.md` / `plan.md` / `core/<crate>.md` / 必要时 ADR）已更新
- [ ] commit 已创建且无无关 diff
- [ ] `git status` 干净，或 WIP 已明确标注

## 8. 协作模式

默认 **单开发者 + AI Agent**，开发者授予 Agent 在 §5 边界内的最高执行权限。

- Agent 回复语言跟随开发者提问语言；代码、命令、路径和专有名词保持原文。
- 日常决策（命名、文件组织、测试布局、小版本依赖）Agent 直接定。
- 只在以下情况停下询问：§5 授权边界、需求方向真正模糊、信息必须由开发者提供、连续 3 次同类失败。
- 多模型协作：用户授权优先使用 `gpt-6.1-sol` 子智能体做已定 contract 的实现/测试；架构由主控收敛，高风险改码由不同模型或 Provider 独立审查。一个主功能簇可拆互不冲突子任务，worker 不递归派发、不擅改 contract；所有 Agent 直接在本仓库 `main` 工作；一个写入者串行实现，审查可并行只读。不得自行创建开发分支或隔离 worktree。模型不可用时如实记录并选成本合适的可用方案。
- **长期 Git 工作方式**：开发者已明确要求当前及以后直接在 `main` 开发和创建本地提交，不使用隔离开发分支/worktree；2026-10-28 授权将现有隔离分支完整转入 main 后删除。后续只有开发者另行明确改变要求才新建分支/worktree；不推送、不发布、不打 tag。独立 AI 产品仍未启动，业务隔离边界保留，不能以此提前创建产品分支。

## 9. 记忆维护

| 变化类型 | 更新位置 |
| --- | --- |
| 用户可见行为、配置、命令 | `docs/`、crate `README.md` |
| 阶段、约束、对外动作 | `context.md` |
| 待办增删 | `todo.md` |
| 里程碑进度、路由 | `plan.md`、`progress.md` |
| crate 当前事实 | `core/<crate>.md` |
| 技术决策 | `decisions/adr-NNN-<topic>.md` |
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
