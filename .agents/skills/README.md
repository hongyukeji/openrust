# 项目技能

本目录保存 OpenRust 可复用工作流。项目事实与授权边界以 [AGENTS.md](../../AGENTS.md) 和 [项目索引](../project/README.md) 为准，技能不维护第二套里程碑、版本矩阵或执行状态。

| 任务 | 技能 | 使用边界 |
| --- | --- | --- |
| 设计 API、模块职责与公开扩展 | [codebase-design](codebase-design/SKILL.md) | 接口有待确定或已有耦合证据时使用 |
| 按已定 contract 完成功能簇 | [implement-feature](implement-feature/SKILL.md) | 覆盖根模块、driver、CLI、生成器及集成收口 |
| 复杂故障与性能回归 | [diagnosing-bugs](diagnosing-bugs/SKILL.md) | 以证据收敛原因，按环境选择复现方式 |
| 测试先行与行为回归 | [tdd](tdd/SKILL.md) | red → green → refactor，按契约选择验证层次 |

每个技能使用 `<name>/SKILL.md` 的名称和描述供 Agent 按任务匹配；正文按需读取。`agents/openai.yaml` 提供 Codex 展示信息，未关闭隐式调用。是否发现新文件取决于客户端刷新机制；被发现不代表每次请求都必须使用，也不代表效果已经过模型对照实验验证。

三个通用技能改编自 [mattpocock/skills](https://github.com/mattpocock/skills/tree/5beb35b040e3de97e54fa9e386d7e42d15c295c2/skills/engineering)。各自元数据记录本次改编参考的固定快照和原始路径，目录内保留上游 MIT 许可；该快照不追认最初安装时未记录的版本。正文按 OpenRust 契约重写，更新上游时应逐项比对，不能覆盖本地适配。
