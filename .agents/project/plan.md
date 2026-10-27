# 当前计划

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：执行 Laravel 式全面重构；计划已记录，实现尚未开始。

## 当前里程碑

[ADR-070](decisions/adr-070-laravel-first-rebuild.md) 已接受全面重写方向；[总计划](plans/laravel-rebuild.md) 定义能力矩阵、Rust 适配、目标目录、R0–R16、验收和模型分工。[执行任务](plans/laravel-rebuild-execution.md) 可直接交给执行 Agent。当前领取项仅在 [todo.md](todo.md)。

## 执行校准

1. 当前代码事实仍是根 `src/` 框架、根 server、tools 和 apps/example；规划起点 `7f964b8`，实施前重新检查 Git/其他代理。目标结构尚未实现。
2. 定位为用 Rust 忠实实现 Laravel，并持续跟进上游功能和行为更新。目录、概念、命名、默认行为和扩展流程以 Laravel 为准；Goravel 为实现参考。Rust 必要适配须记录具体原因与等价证据；版本基线和后继更新见[长期上游跟进](plans/laravel-rebuild.md#长期上游跟进)。既有目录和 API 可以重写，无历史兼容义务。
3. 旧 ADR 中的已实现行为和验收记录作为迁移输入；新方向不自动使旧记录变成新代码已通过。冲突约束依 ADR-070 替代，功能簇落地时同步领域 ADR/事实。
4. 原 M5/M6、DX/F/G 顺序让位于 R0–R16；未完成生产、集群、SQLite、供应商条件等映射到新矩阵，不丢弃。只一个主功能簇 in_progress，内部允许隔离子任务。
5. 对已定 contract 的实现/测试优先 gpt-6.1-sol；主控定跨模块接口，高风险改码不同模型或 Provider 审查。worker 默认独立 worktree，不递归派发、不自动合并、不推送。
6. 默认运行和 RustRover 调试同一 Command=`run` 是验收项；R1 实际验证，不能以 --bin/--package 或别名代替。
7. 日常 PostgreSQL 18/MySQL 8.4 LTS/Redis 8；真实复制、多实例、网络和供应商证据按总计划。最低版本、SQLite、完整集群分别验证。
8. R14 底座完整通过再做 R15 V2：Vue Admin、uni-app App、PC 只规划。独立 AI模型平台仍未获启动授权。
9. WIP、他人资源、依赖安全、真实证据及对外授权边界继续有效。常规问题自主决策并记录；外部缺口继续独立工作，不能虚报完成。

## 路由

| 任务 | 入口 |
| --- | --- |
| 新方向与替代关系 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)、[ADR 索引](decisions/README.md) |
| 整体范围、阶段、验收 | [全面重构计划](plans/laravel-rebuild.md) |
| 当前实现资产 | [architecture.md](architecture.md)、core、Cargo.toml、源码和 Git |
| 原能力/生产验收输入 | [原 v1 计划](plans/gen1-plan.md)、[底层补齐](plans/v1-foundation-completion.md)、[集群](plans/v1-cluster-deployment.md)、[原 DX](plans/v1-developer-experience.md) |
| 编码、提交、证据 | [conventions.md](conventions.md)、[testing.md](memory/testing.md)、[verification.md](memory/verification.md) |
| 执行任务文本 | [执行说明](plans/laravel-rebuild-execution.md) |

## 验证入口

每代码簇按 AGENTS.md 跑 fmt、workspace clippy/test/doc、cargo deny，及适用 feature/MSRV、双库、生成项目、真实服务门禁；记录命令与实际结果。文档-only 只做 diff、链接/锚点和旧口径检查。不能把本轮规划检查写成代码、IDE 或重构已验收。
