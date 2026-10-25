# AI Agent 记忆索引

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
| `roadmap.md` | 第一代之后的版本方向 |
| `plans/` | 第一代开发计划与框架设计方案；进入开发后按功能簇拆入 `todo.md` |
| `architecture.md` / `stack.md` / `conventions.md` | 架构、技术栈和工程规范 |
| `style.md` | 文档写作、文件命名和清理规范 |
| `decisions/` | 仍有效或明确标记为 Superseded 的架构决策 |
| `memory/testing.md` / `memory/verification.md` | 当前可复验测试命令和验收边界 |
| `scripts/` | 提交脚本 |
| [`../skills/`](../skills/README.md) | 专项工作流；按任务读取 |

代码落地后按 crate 新增 `core/<crate>.md` 保存各 crate 当前事实；第一代只有框架 crate，没有业务模块目录。

## 维护规则

- 不保存聊天记录、执行流水、已解决问题报告、完成任务副本或按日期归档。
- `todo.md` 完成一项就删除；完成证据写入 crate 事实、正式文档和 commit。
- `progress.md`、`testing.md`、`verification.md` 只保留当前基线；新结果替换旧结果。
- 远期方向写入 `roadmap.md`；进入开发前再拆入 `todo.md`。
- `plans/` 中的方案在对应里程碑完成后，把稳定事实迁入 `architecture.md` / `core/*.md` / `docs/`，删除已完成部分，不保留“已完成”占位。
