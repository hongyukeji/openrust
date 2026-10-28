# AI Agent 记忆索引

当前设计和开发顺序由 [ADR-070](decisions/adr-070-laravel-first-rebuild.md) 与 [全面重构计划](plans/laravel-rebuild.md) 接管；[执行任务](plans/laravel-rebuild-execution.md) 提供可直接使用的任务说明。旧计划只作能力/证据输入。

> 受众：AI Agent
> 摘要：`.agents/project/` 只维护 OpenRust 当前基线。执行历史、已解决问题和完成任务由 Git 追溯。

## 启动读取

清单与按任务补读表以 [`AGENTS.md` §3](../../AGENTS.md) 为唯一维护点，本文件不复制。

## 当前结构

| 路径 | 职责 |
| --- | --- |
| `context.md` | 当前版本、阶段、协作模式和关键约束 |
| `todo.md` | 唯一活跃任务队列，只放未完成且可领取的功能簇 |
| `plan.md` | 当前里程碑、优先级、crate 路由和验证入口 |
| `progress.md` | 当前交付与验证摘要，不保存逐日流水 |
| `roadmap.md` | v1 之后的版本方向 |
| `plans/` | 框架版本计划与设计方案，以及独立排期的可选领域套件与完整应用规划；仅框架活跃任务进入 `todo.md` |
| [`plans/ai-model-platform.md`](plans/ai-model-platform.md) | AI模型平台唯一计划：完整范围、内含网关源码、工程验收及同仓 main 的模块/目录隔离；用户明确要求开发后才启动 |
| `architecture.md` / `stack.md` / `conventions.md` | 架构、技术栈和工程规范 |
| `style.md` | 文档写作、文件命名和清理规范 |
| [decisions/README.md](decisions/README.md) | ADR 唯一编号、有效决定与替代关系索引 |
| `memory/testing.md` / `memory/verification.md` | 当前可复验测试命令和验收边界 |
| `scripts/` | 提交脚本 |
| `guards/` | 本地守护看板、心跳及审计状态，忽略Git提交 |
| [`../skills/`](../skills/README.md) | 专项工作流；按任务读取 |

代码落地后按框架模块或保留工具 package 在 core/ 维护当前事实。根 openrust package 同时提供 src/lib.rs 框架库与 src/main.rs 默认 server；Laravel 应用职责目录直接位于根，当前 app/example 仍为迁移中的代码；最终唯一综合示例为 modules/example，由统一模块入口装配。最终布局、迁移和验收边界见 [ADR-074](decisions/adr-074-laravel-modules-directory.md)。CLI 模板与生成器保留用户应用生成功能，不要求另建长期源码仓库。

## 维护规则

- 不保存聊天记录、执行流水、已解决问题报告、完成任务副本或按日期归档。
- `todo.md` 完成一项就删除；完成证据写入 crate 事实、正式文档和 commit。
- `progress.md`、`testing.md`、`verification.md` 只保留当前基线；新结果替换旧结果。
- 远期方向写入 `roadmap.md`；进入开发前再拆入 `todo.md`。
- `plans/` 中的方案在对应里程碑完成后，把稳定事实迁入 `architecture.md` / `core/*.md` / `docs/`，删除已完成部分，不保留“已完成”占位。
