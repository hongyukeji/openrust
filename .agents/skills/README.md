# 项目技能

本目录保存 OpenRust 可复用工作流。项目事实与授权边界以 [AGENTS.md](../../AGENTS.md) 和 [项目索引](../project/README.md) 为准，技能不维护第二套里程碑、版本矩阵或执行状态。

| 任务 | 技能 | 使用边界 |
| --- | --- | --- |
| 设计 API、模块职责与公开扩展 | [codebase-design](codebase-design/SKILL.md) | 接口有待确定或已有耦合证据时使用 |
| 按已定 contract 完成功能簇 | [implement-feature](implement-feature/SKILL.md) | 覆盖根模块、driver、CLI、生成器及集成收口 |
| 复杂故障与性能回归 | [diagnosing-bugs](diagnosing-bugs/SKILL.md) | 以证据收敛原因，按环境选择复现方式 |
| 测试先行与行为回归 | [tdd](tdd/SKILL.md) | red → green → refactor，按契约选择验证层次 |
| Vue Web 组件与工程 | [vue-best-practices](vue-best-practices/SKILL.md) | 路由、状态和测试细节按需读取，复用已选后台骨架 |
| uni-app App 开发 | [uniapp-development](uniapp-development/SKILL.md) | 页面生命周期、导航与跨端差异，App 单独验收 |
| 应用界面设计与打磨 | [impeccable](impeccable/SKILL.md) | 面向后台与移动操作流程的精简适配版 |
| Web 界面审查 | [web-design-guidelines](web-design-guidelines/SKILL.md) | 固定版本规则，可访问性、表单与响应式检查 |
| 浏览器功能验收 | [webapp-testing](webapp-testing/SKILL.md) | 真实 Rust API 与页面断言，区分 mock、H5 和 App |

每个技能使用 `<name>/SKILL.md` 的名称和描述供 Agent 按任务匹配；正文按需读取。`agents/openai.yaml` 提供 Codex 展示信息，未关闭隐式调用。是否发现新文件取决于客户端刷新机制；被发现不代表每次请求都必须使用，也不代表效果已经过模型对照实验验证。

三个通用技能改编自 [mattpocock/skills](https://github.com/mattpocock/skills/tree/5beb35b040e3de97e54fa9e386d7e42d15c295c2/skills/engineering)。各自元数据记录本次改编参考的固定快照和原始路径，目录内保留上游 MIT 许可；该快照不追认最初安装时未记录的版本。正文按 OpenRust 契约重写，更新上游时应逐项比对，不能覆盖本地适配。

## 前端技能边界

[V2 计划](../project/plans/v2-admin-platform.md) 是端侧范围和启动顺序的权威入口：Admin 使用 Vue / pure-admin 精简国际化版 + Element Plus，App 使用 uni-app，PC 端只规划设计。技能就绪不表示 V2 已启动，具体兼容版本与接入验证仍在 B1 完成。

- Vue 技能合并 `vuejs-ai/skills` 的基础、Router、Pinia、测试四个来源，保留必要规则与按需参考，避免重复触发。
- Impeccable 保留应用设计方法，不导入其自动下载程序、浏览器扩展、代理配置和根目录产品文档流程；不同时安装另一套重叠设计技能。
- Web 审查直接改编有 MIT 许可的 `vercel-labs/web-interface-guidelines/command.md`；不复制未附许可的 `agent-skills` 入口，不在每次审查时拉取动态规则。
- 浏览器验收改编 Anthropic `webapp-testing`，使用项目测试工具和状态断言，不附带另一套进程管理脚本。
- uni-app 技能由项目编写并链接 DCloud 文档；不是官方技能，不默认继承 Vue Web 的路由、DOM 和浏览器测试保证。

四个外部适配技能的固定来源、版本与改动说明在各自入口，许可在各自目录。上游更新需重新审查依赖、授权和规则冲突，不能覆盖本地适配。
