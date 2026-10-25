# AGENTS.md — AI Agent 协作契约

> 适用：任何软件项目（不限编程语言和框架）。本文件是 AI 编程 Agent 在仓库工作的最高入口规则。
> 摘要：通用版协作契约。项目特定的模块清单、技术栈、命名约定、当前状态全部放 `.agents/project/` 下。

适用对象：Codex / Claude Code / Cursor / RooCode / OpenHands 等 AI 编程 Agent。

## 1. 角色与契约

| 角色 | 职责 |
| --- | --- |
| 开发者 | 架构方向确认、需求确认、代码审查、最终交付 |
| AI Agent | 规格起草、代码实现、测试验证、记忆维护、变更说明 |

Agent 不替代开发者做需求决策、最终发布判断。开发者不替代 Agent 维护 `.agents/project/` 记忆。

开发期 Agent 直接推进任务，不按代码风险类别反复评估暂停。除 §5 列出的授权边界与不可逆操作外，按工作循环 §6 连续执行。

## 2. 权威来源

信息冲突时按优先级判断：

1. 代码、Git 状态、构建配置（事实）
2. `AGENTS.md`（本文件）
3. `.agents/project/README.md`（目录索引）
4. `.agents/project/context.md`（项目当前上下文）
5. `.agents/project/todo.md`（活跃待办）
6. `.agents/project/plan.md`（当前阶段计划 + 模块负责人）
7. `.agents/project/architecture.md` / `.agents/project/conventions.md` / `.agents/project/stack.md`（项目规则）
8. `.agents/project/decisions/`（架构决策记录 ADR）
9. `.agents/project/modules/<x>.md` / `.agents/project/starters/<x>.md`（业务模块 / Starter 当前事实）
10. `docs/`（面向用户和开发者的正式文档）
11. 历史日志、聊天记录

不依赖聊天历史或口头记忆。执行前以仓库文件和 Git 状态为准。

## 3. 启动读取

每次会话先确认以下入口；已完整注入或本会话已读且未变化的内容不重复读取。长文件先用 `rg -n` 定位相关章节，不把历史验收叙述全量载入：

1. `AGENTS.md`（本文件）
2. `.agents/project/README.md`（目录索引）
3. `.agents/project/context.md` 的当前版本、阶段、模式与固定约束，以及本任务相关段落
4. `.agents/project/todo.md` 的当前状态与本任务相关待办
5. `.agents/project/plan.md` 的当前阶段与本任务相关路由；发布门禁仅在发布或全量审计时补读

按任务类型补读：

| 任务类型 | 补读 |
| --- | --- |
| 架构 / 模块边界 | `.agents/project/architecture.md`、相关 `.agents/project/decisions/` |
| 编码 / 命名 / 提交 / 验证 | `.agents/project/conventions.md` |
| 依赖 / 版本 | `.agents/project/stack.md` |
| 业务模块开发 | `.agents/project/modules/<module>.md` + 相关 `docs/<module>.md` + 模块代码 |
| Starter 开发 | `.agents/project/starters/<starter>.md` + 相关 `docs/<starter>.md` + starter 代码 |
| 候选能力 | `.agents/project/roadmap.md` + 相关 `.agents/project/modules/<module>.md` |
| 文档写作 | `.agents/project/style.md` |
| 最近进度 / 验证 | `.agents/project/progress.md` / `.agents/project/memory/testing.md` / `.agents/project/memory/verification.md` |

不全量读取 `docs/` 与各类构建产物 / 依赖目录 / 运行产物（如 `node_modules` / `target` / `build` / `dist` / `.gradle` / `venv` / `vendor` / `__pycache__` 等）。优先 `rg` 定位章节。

项目 skills 位于 `.agents/skills/<name>/SKILL.md`。Codex 从当前工作目录向 Git 根目录扫描各级 `.agents/skills`，先发现名称和描述，匹配任务后才读正文；其他 Agent 按同一路径按需读取。只加载当前任务需要的 skill 与参考文件，不遍历全部正文，不因加载 skill 扩展任务、重复启动读取或自动启用分支 / PR / 派工。

执行项目任务时发现相关 skill 有实际缺口或歧义，Agent 可在项目目录内自主更新 `.agents/skills/`，使工作流更准确、清晰且易维护；只处理本任务发现的问题，不全面重写或借此扩展任务范围。skill 改进按 §6 的 Spec → Implement → Verify → Commit 闭环执行并创建本地提交，不为每次改进重复请求许可；§5 的项目外写入与不可逆操作授权边界仍适用。

## 4. `.agents/project/` 目录约定

`.agents/project/` 是 AI Agent 长期记忆目录。任何软件项目均遵循同一结构：

```text
.agents/project/
├── README.md           # 目录索引
├── style.md            # 文档写作与命名规范
├── context.md          # 项目当前上下文（版本、阶段、模式、约束）
├── todo.md             # 活跃待办（按优先级分组）
├── plan.md             # 当前阶段计划（优先级 + 模块负责人 + 路由）
├── progress.md         # 当前交付与验证摘要（历史由 Git 追溯）
├── architecture.md     # 项目模块结构
├── conventions.md      # 项目编码 / Git / 验证规范
├── stack.md            # 技术栈与版本基线
├── roadmap.md          # 版本目标
├── decisions/          # 架构决策（每 ADR 一文件 adr-NNN-*.md）
├── modules/            # 业务模块、部署、前端当前事实（每模块一文件）
├── starters/           # Starter 当前事实（每 starter 一文件）
├── core/               # Core 技术能力当前事实
├── plugins/            # 可安装 Plugin 能力边界当前事实
├── plans/              # 明确要求持久化、尚未进入执行的详细方案
├── memory/             # 当前测试与验证基线
├── developers/         # 角色开发约束、模板与临时记录
└── scripts/            # 检查、提交与协作锁脚本
```

`docs/` 面向用户和开发者，`.agents/project/` 保存 Agent 项目记忆，`.agents/skills/` 保存可发现的专项工作流，不混用。`.agents/` 下的其他子目录名留给 Agent 工具约定，项目内容只放 `project/`，不在 `.agents/` 根目录新增文件或目录。

`.agents/project/` 由 Agent 自维护。开发者只读查看，不手动改。

新项目首次接入：Agent 按本节结构创建 `.agents/project/`，最少生成 `README.md` / `context.md` / `todo.md` / `plan.md` / `architecture.md` / `conventions.md` / `stack.md`。

## 5. 授权边界与不可逆操作

这一节是关于**作业范围 / 不可逆 / 影响他人**操作的安全规则。代码改动本身（删 API / 重构 / 改 DB schema / 安全 / 支付逻辑）开发期按目标推进，**不为代码风险类别暂停**。

### 5.1 项目目录边界

Agent 默认作业范围 = **当前项目根目录及其所有子目录**（包含 `AGENTS.md` 的目录及递归子目录）。该范围内的文件和文件夹**增删改无限制**，按 §6 工作循环自主执行。

**项目目录之外的写操作必须先请求开发者明确授权**：

- 修改、删除、新增项目目录之外的任何持久化文件或文件夹
- 包括但不限于：系统目录（`/etc` / `/var` / `/usr` 等）、用户 home（`~/.config` / `~/.gitconfig` / `~/.ssh` / `~/.bashrc` / `~/.zshrc` 等）、其他项目目录、外部仓库、共享磁盘
- 全局 npm / pip / cargo / brew 等包管理器的全局安装也算外部写操作

**项目目录之外的只读访问不受限制**：

- 读取系统命令输出（`uname` / `git config --get user.email` / `which node` 等）
- 读取版本管理器全局配置（仅查看，不修改）
- 临时文件（`/tmp/*` / `$TMPDIR/*`）用于工具执行后立即清理可不授权

不确定边界时按授权处理：先询问，不擅自越界。

### 5.2 Git 与不可逆操作

执行前必须确认的操作：

- **force push**：禁止，除非用户明确要求
- **删除远程分支**：禁止，除非用户明确要求
- **推送 `main` / `master` / 受保护分支**：先确认当前分支正确、工作区干净、验证通过、commit 不含无关改动
- **`drop table` / 清空数据库 / 大批量 DELETE**：确认目标，区分开发库 vs 生产库
- **`rm -rf <目录>`**：确认目标，检查是否包含未提交工作
- **删除核心配置文件**：通过 `git rm` 或确认后再删，便于回滚
- **删除 `.git`**：禁止
- **修改 Git 钩子 / CI 配置以跳过验证**：禁止，除非用户明确要求

### 5.3 失败与拿不准

**同类失败连续 3 次**：停下来说明现象、已尝试方案、推测原因，请求开发者判断。这是质量自检，不是风险分类。

**Agent 自己拿不准方向时**：在小规模实现前用 5-10 行说明意图。这是 Spec 阶段（§6.1），不是确认许可。

## 6. 工作循环

每个功能簇冲刺采用 **Spec → Implement → Verify → Commit** 循环。

### 6.1 Spec（规格）

复杂或多步任务先写 5-15 行说明：

- **要改什么**：一句话
- **为什么**：业务或技术理由
- **怎么验证**：测试命令或观察点
- **影响范围**：受影响的文件、模块、API

Spec 是 Agent 自己理清思路的工具，**不是请求开发者确认**。轻量任务（typo / 单行修复 / 文档补全）可省略。

### 6.2 Implement（实现）

- 一个功能簇 = 一个业务或工程闭环，不夹带无关需求
- 开发新功能或重构既有能力时，必须评估新旧功能、接口及调用方的兼容性，默认保持既有功能正常使用及调用方式、参数、权限、状态、返回结构、字段含义和数据语义稳定。确需不兼容变更时，先在 Spec 中说明原因、新旧行为差异、影响范围和验证方式，由开发者确认取舍后实施；已有明确授权的范围直接执行，不重复请示。实施时同步更新受影响调用方、文档和必要验证，不引入授权范围之外的行为回归
- 仅适用于特定任务或阶段的兼容性取舍，记录在对应任务计划或 `.agents/project/context.md`，注明适用范围和失效条件，任务或阶段结束后清理；不将临时授权写成长期通用规则
- 实现或验证中发现边界清楚、影响可控、能当场验证的小问题时，允许顺手修掉并纳入本轮说明；不要因“非原始任务”把明显问题留到遗忘
- 顺手修一旦需要跨模块设计、扩大本轮已接管范围、接手他人 WIP、改变产品方向或无法随本轮验证，就停止扩散，改写入 `.agents/project/todo.md` / 相关模块记忆作为后续功能簇
- 优先编辑既有文件，避免新文件
- 不写无意义注释；只在 WHY 非显然时写一行（不写「设置字段值」这种注释）
- 前端新增或修改按钮、标签、菜单、状态、提示等用户可见文案时，必须按项目 i18n 机制接入国际化，并同步补齐对应国际化资源；缺失时至少补齐 `zh-CN` / `zh-TW` / `en-US` 三语翻译，不能只写硬编码中文或单语 key
- 新增或重构 admin 页面时，优先按 pure-admin 风格设计和实现；从易用性、信息密度和空间合理性出发组织筛选、表格、表单、详情和操作区，合理使用表格、工具栏、抽屉 / 弹窗、分组表单、标签页、折叠面板等前端组件，不把所有能力堆在同一块区域里。
- 新增或修改菜单同步脚本时，菜单图标编码必须先核对 admin 前端已安装图标包和实际可用图标编码；只有确认图标包中存在对应图标后才能填写，不能凭名称猜测或写未验证编码
- 后端新增或修改关键方法时，只在方法名和局部代码不能清楚表达 WHY、触发边界、失败语义或安全约束时补充简洁注释 / Javadoc；不要为了“看起来完整”给直白流程、普通 getter / setter 或赋值代码补重复注释
- 不为不可能场景做错误处理 / fallback；只在系统边界（用户输入、外部 API）验证
- 不做未需要的抽象；三行相似代码胜过过早抽象
- 不为假设的未来需求设计扩展点

### 6.3 Verify（验证 + 自查 + 自纠）

提交前 Agent 自己完成以下检查，**不把半成品上报为完成**：

**1. 自查**（每次必做）：

- 重读修改过的每个文件，捕捉 typo / 格式错乱 / 编号断号 / 残留占位符
- `git diff` 与声明的变更一致，无意外文件、无无关改动
- 内部链接、相对路径、跨文件引用全部存在且指向正确目标
- 重命名 / 移动 / 删除文件后用 `rg` 扫一遍旧路径，确认无悬空引用

**2. 范围验证**：

- 业务层面默认不新增单元测试；普通业务 CRUD、流程编排、页面联动等用编译 / 构建 / 应用级 smoke / 集成观察验证，避免堆积低价值测试。只有涉及底层架构、starter / core 通用机制、关键算法、跨模块 contract 或可复用基础能力时，才新增或调整单元测试。
- 后端测试不设置固定文件数量上限。新增、合并或删除测试以风险覆盖、重复程度、执行成本和维护价值为依据。资金、安全、权限、迁移、并发、幂等、跨模块 contract、Starter / Core 通用机制及历史缺陷回归测试，没有等价覆盖不得删除
- 新建测试文件前必须能一句话说清它能发现哪个缺陷，说不清就不建。镜像常量 / 默认值 / 文案 / 源码文本的断言、只验证框架自身行为或 Mock 自身返回值的用例、与既有测试重复或恒真的断言都不单独成文件；确需补充的用例并入同能力既有测试类，一次性验证脚本不落进测试源目录。判据见 `.agents/project/conventions.md`「验证」

| 改动范围 | 验证 |
| --- | --- |
| 业务层源码 | 编译 / 构建 / 应用级 smoke / 集成观察；默认不写单元测试 |
| 底层架构 / starter / core / 通用机制源码 | 相关模块 unit test |
| 应用装配或依赖 | 应用聚合 test |
| 数据库迁移 | 启动验证 + 迁移测试 |
| 文档-only | `git diff --check` + 相对链接检查 + `rg` 残留引用扫描 |
| 配置 / 部署 | 按需 package / smoke |

具体验证命令在 `.agents/project/conventions.md` 或 `.agents/project/plan.md` 中定义。

**3. 自纠**：发现问题在当前轮内修复，不留半成品给开发者。测试失败先自行修复，连续 3 次同类失败按 §5 处理。

不可机器验证的项（视觉效果、产品 UX 判断）在交付报告中显式标注「需开发者确认」，但代码本身仍按 §6.4 完成 commit。

### 6.4 Commit（提交）

**自查 + 验证通过后 Agent 直接创建本地 commit，不等待开发者额外确认**。这是工作循环的闭环，不是可选步骤。

- 功能簇提交 = 完整业务或工程闭环，commit diff 不含无关改动
- commit 前最后一次 `git diff --staged`，确认变更与声明一致
- commit 消息格式、type 范围、`AI-Agent` 来源署名和共享 index 下的 pathspec commit 写法以 `.agents/project/conventions.md` 的「Git 提交」章节为唯一维护来源；本文件只约束“验证通过后必须创建本地 commit”这一闭环要求，避免提交格式规则双写漂移

**例外**：若当前为 WIP（功能簇未闭环），不 commit，但在 `.agents/project/todo.md` 标注 WIP 状态和阻塞点，**绝不把已完成工作长期搁置不 commit**。

工作区干净时允许普通推送当前分支。

推送 `main` 或受保护分支按 §5 确认。

## 7. WIP 与完成定义

**WIP = 1**：同一时刻只有一个功能簇处于 in_progress。

阻塞时：写下阻塞原因到 `.agents/project/todo.md`，把功能簇移回 backlog，不并行新任务。

每个功能簇完成必须满足（Agent 主动验证每一项，**不能凭直觉假设满足**）：

- [ ] 完成 §6.3 对应范围验证；代码改动实际执行匹配的编译 / 构建，文档或 skill-only 不运行无关全量构建
- [ ] 行为变化有匹配验证；业务层默认不写单元测试，底层架构 / 通用机制变化才新增或更新 unit test 并实际跑通
- [ ] 受影响 `docs/` 文档已更新（grep 验证旧引用清理干净）
- [ ] 受影响 `.agents/project/` 记忆已更新（`context.md` / `todo.md` / `plan.md` / 模块 / 必要时新增 ADR）
- [ ] 功能簇 commit 已创建，无无关 diff（`git diff --staged` 实际看过）
- [ ] `git status` 干净，或已知 WIP 明确说明
- [ ] 内部链接 / 路径引用全部 resolve（`rg` 扫旧路径）

## 8. 协作模式

默认 **单开发者 + AI Agent** 模式（**开发者授予 Agent 在 §5 边界内的最高执行权限**）：

- 开发者确认需求方向和最终交付
- Agent 按工作循环自主执行：规格 → 实现 → 自查 → 自纠 → 提交 → 记忆维护
- **Agent 回复语言跟随开发者提问语言**：开发者用什么语言提问，就用什么语言回复；代码、命令、路径和专有名词保持原文
- **日常决策** Agent 直接定，不主动请示（命名、文件组织、注释、refactor 细节、记忆位置、依赖小版本选择、模块文档放哪、是否新增 ADR 等）
- **验证发现问题** Agent 在当前轮内自纠，不把半成品上报为完成
- **只在以下情况停下询问**：
  - §5.1 项目目录之外的写操作（系统目录 / 用户 home / 其他项目 / 全局包管理器等）
  - §5.2 Git 与不可逆操作（force push / 删远程分支 / drop table / 推送受保护分支等）
  - 需求真正模糊（不是细节，是方向）
  - 信息真正缺失（必须开发者提供才能继续）
  - 连续 3 次同类失败仍无法修复
- 不为「这件事好像有点风险」找借口暂停。代码风险类别不是暂停条件。

**功能簇冲刺 + 集中收口**：Agent 连续完成同一功能簇内强相关的后端 contract、前端视图、测试、文档和记忆维护。冲刺过程中持续处理编译、类型、关键单测和 `git diff --check` 反馈；冲刺结束集中验证、集中收口并提交。按 §9 只更新事实发生变化的文档和记忆，不为每次提交机械改写全部入口。

**目标优先、允许可控顺手修、防止无界扩散**：先完成既定目标和当前功能簇闭环；过程中遇到明显、局部、可验证的问题可以当场修掉，避免遗忘；不主动追加兼容包装、冗余检查或无关衍生能力，超出本轮边界的问题进入 `.agents/project/todo.md` 或模块记忆。

多分支 / PR 流程仅在开发者明确要求时启用：

1. 同步最新主分支
2. 解决冲突
3. 验证改动范围
4. 本地提交并推送
5. 创建或更新面向主分支的 PR
6. 报告 PR 状态和缺口

## 9. 记忆维护

每个任务完成后按变化类型更新：

| 变化类型 | 更新位置 |
| --- | --- |
| 开发者使用说明、API 行为、配置示例、运维入口 | `docs/<相关>.md` |
| 项目当前上下文（版本、阶段、约束变化） | `.agents/project/context.md` |
| 活跃待办（新增、调整、移除完成项） | `.agents/project/todo.md` |
| 当前阶段计划、优先级、模块负责人 | `.agents/project/plan.md` |
| 业务模块当前事实 | `.agents/project/modules/<module>.md` |
| Starter 当前事实 | `.agents/project/starters/<starter>.md` |
| 新增或修改技术决策 | `.agents/project/decisions/adr-NNN-<title>.md`（编号递增） |
| 关键验证结果 | `.agents/project/memory/testing.md` 或 `verification.md` |
| 路线图变化 | `.agents/project/roadmap.md` |
| 重要进度（不堆历史） | `.agents/project/progress.md` |
| 可复用工作流 | `.agents/skills/<name>/SKILL.md`，较长专项步骤放同目录 `references/` 按需读取 |

不在共享文件追加长篇历史日志；审计走 Git 历史。

文档写作规范见 `.agents/project/style.md`。

## 10. 禁止事项

- 不删文件除非用户明确要求或经过 Git 受控（`git rm`）
- 不引入新依赖除非任务需要且在 `.agents/project/decisions/` 记录
- 不写 `TODO` 注释（活跃任务进 `.agents/project/todo.md`）
- 不写聊天恢复提示进正式文档
- 不在 `docs/` 写执行流水
- 不在 `.agents/project/` 写最终用户教程
- 不擅自做发布、tag、生产部署
- 不通过修改产品源码掩盖截图 / 验证问题
- 不跳过 §6.3 验证直接 commit
- 不在自查通过且任务闭环后还把已完成工作长期搁置不 commit（除非明确 WIP 并在 `.agents/project/todo.md` 标注）
- 不在未经开发者明确授权时操作项目目录以外的持久化路径（写入 / 修改 / 删除）

## 11. 项目特定补充

项目特定的模块清单、技术栈版本、命名约定、生态选型、当前状态全部在 `.agents/project/` 下，本文件不重复：

- 模块结构：`.agents/project/architecture.md`
- 技术栈：`.agents/project/stack.md`
- 编码 / Git / 验证：`.agents/project/conventions.md`
- 当前上下文：`.agents/project/context.md`
- 活跃待办：`.agents/project/todo.md`
- 当前计划：`.agents/project/plan.md`
- 不提供 WIP 协作锁脚本：通过 `git status`、目标文件 diff 和 pathspec 提交避让。§7 的 WIP=1 表示单个 Agent 同时只推进一个子功能簇。
- 项目可复用工作流：`.agents/skills/`；发现与按需加载见 §3

新项目首次接入或规则被遗忘时，Agent 主动读取上述文件恢复项目上下文。
