# 当前计划

> 当前代码与目标：源码已提交 `app/example/` 过渡结构；最终业务位置为 `modules/<模块名>/`，唯一示例为 `modules/example/`。目录和模块能力尚未完成验收，按 [ADR-074](decisions/adr-074-laravel-modules-directory.md) 推进，不能将目标路径当作已实现。

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：执行 Laravel + laravel-modules 的 Rust 1:1 实现；R0 上游和逐章资产基线已记录，R1-S 已提交34dfb5e，R2-A/A2/A3 已验收。R2-B 统一 Provider 生产内核已通过本簇门禁，随后 R2-C/D、R2-M 模块闭环、R1-E 真实 CRUD/当前能力示例；R1 整体未完成。

## 当前里程碑

[ADR-070](decisions/adr-070-laravel-first-rebuild.md) 已接受全面重写方向；[总计划](plans/laravel-rebuild.md) 定义能力矩阵、Rust 适配、目标目录、R0–R16、验收和模型分工。[执行任务](plans/laravel-rebuild-execution.md) 可直接交给执行 Agent。当前领取项仅在 [todo.md](todo.md)。

## 执行校准

1. 当前代码为已提交的根 lib/server 与 app/example 过渡结构，R1-S 已以34dfb5e收口；最终根结构和 modules/ 边界依 ADR-074。结构子簇实际门禁与限定提交已完成，现在实现 R2 基础、R2-M 模块闭环/目录迁移、R1-E CRUD。不能将新目标路径或旧布局验证写成已实现。
2. 正式定位 Laravel + laravel-modules = OpenRust，以固定上游源码/测试和文档逐项1:1实现功能/可观察语义、默认/错误安全边界、Container/Provider与开发流程，并持续跟进；模块化不是只有示例目录规范。目录、概念、命名、默认行为和扩展流程以 Laravel 为准；Goravel 为实现参考。Rust必要适配、用户约定和授权增强须按[差异分类](plans/laravel-rebuild.md#逐项对应与差异分类)记录原行为/限制/选定行为/证据，未等价保持缺口；版本基线和后继更新见[长期上游跟进](plans/laravel-rebuild.md#长期上游跟进)。既有目录和 API 可以重写，无历史兼容义务。
3. 旧 ADR 中的已实现行为和验收记录作为迁移输入；统一返回格式等优秀机制须融合进 Laravel 对应职责，不能为追求上游外形而删除。新方向不自动使旧记录变成新代码已通过。冲突约束依 ADR-070 替代，功能簇落地时同步领域 ADR/事实。
4. 原 M5/M6、DX/F/G 顺序让位于 R0–R16；未完成生产、集群、SQLite、供应商条件等映射到新矩阵，不丢弃。只一个主功能簇 in_progress，内部允许隔离子任务。
5. 对已定 contract 的实现/测试优先 gpt-6.1-sol；主控定跨模块接口，高风险改码不同模型或 Provider 审查。所有 worker 直接在 main，明确非重叠文件可并行；manifest/lock/迁移/共享文档/提交由集成负责人串行协调，构建 target 与服务资源预先分配；不创建额外源码仓库、开发分支或隔离 worktree，不递归派发、不推送。
6. 根无参数 cargo run、唯一综合示例、生成应用和真实 HTTP 是验收项；根 package 同时包含框架库和默认 server。
7. 日常 PostgreSQL 18/MySQL 8.4 LTS/Redis 8；真实复制、多实例、网络和供应商证据按总计划。最低版本、SQLite、完整集群分别验证。
8. R14 底座完整通过再做 R15 V2：Vue Admin、uni-app App、PC 只规划。独立 AI模型平台仍未获启动授权。
9. WIP、他人资源、依赖安全、真实证据及对外授权边界继续有效。常规问题自主决策并记录；外部缺口继续独立工作，不能虚报完成。

当前交付依赖是 R1-S结构/挂载 → R2基础Container/Provider/配置/TestApp → R2-M模块契约/目录迁移 → R1-E真实CRUD与当前能力演示；R2后继和各领域逐簇实现并同步示例。结构已提交不等于R1整体/严格1:1完成，GET基础响应不等于真实业务CRUD。

## 路由

| 任务 | 入口 |
| --- | --- |
| 新方向与替代关系 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)、[ADR 索引](decisions/README.md) |
| 整体范围、阶段、验收 | [全面重构计划](plans/laravel-rebuild.md)、[模块正式矩阵](plans/laravel-modules-matrix.md) |
| 当前实现资产 | [architecture.md](architecture.md)、core、Cargo.toml、源码和 Git |
| 原能力/生产验收输入 | [原 v1 计划](plans/gen1-plan.md)、[底层补齐](plans/v1-foundation-completion.md)、[集群](plans/v1-cluster-deployment.md)、[原 DX](plans/v1-developer-experience.md) |
| 编码、提交、证据 | [conventions.md](conventions.md)、[testing.md](memory/testing.md)、[verification.md](memory/verification.md) |
| 执行任务文本 | [执行说明](plans/laravel-rebuild-execution.md) |

## 验证入口

每代码簇按 AGENTS.md 跑 fmt、workspace clippy/test/doc、cargo deny，及适用 feature/MSRV、双库、生成项目、真实服务门禁；记录命令与实际结果。文档-only 只做 diff、链接/锚点和旧口径检查。不能把本轮规划检查写成代码或重构已验收。

当前重构来源与 102 章/749 子节当前纳入范围盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
