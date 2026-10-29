# 当前计划

> 当前代码与目标：R1-E、R2-N、R3-A/B/C/D/E/F 均已按范围交付（R3-E：dab80d4；R3-F 本簇限定提交见 Git）。R3-F 已完成独立复审、适用门禁、生成应用、根真实 TCP 与资源收尾；C04-A RequestInfo/内容协商与staging并发回收修复已通过适用验收，随后继续todo中的完整C04剩余行为。完整模块管理仍按 [ADR-074](decisions/adr-074-laravel-modules-directory.md) 推进。

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：执行 Laravel + laravel-modules 的 Rust 1:1 实现；R0 上游和逐章资产基线已记录，R1-S、R2-A/A2/A3、R2-B/C/D/M/N、R1-E 与 R3-A/B/C/D/E 已按簇完成适用验收；后续继续 R3 HTTP/Web 与模块行为矩阵。R0–R16 全体及严格 1:1 仍未完成。

## 当前里程碑

[ADR-070](decisions/adr-070-laravel-first-rebuild.md) 已接受全面重写方向；[总计划](plans/laravel-rebuild.md) 定义能力矩阵、Rust 适配、目标目录、R0–R16、验收和模型分工。[执行任务](plans/laravel-rebuild-execution.md) 可直接交给执行 Agent。当前领取项仅在 [todo.md](todo.md)。

## 运行时配置与动态调度依赖

用户授权目标见 [ADR-080](decisions/adr-080-runtime-configuration-scheduling.md) 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)：Laravel配置体验、V1必交typed分组DB业务设置与受权管理API（宿主启用可选）、默认DB队列与动态计划；业务secret加密及driver受控激活/待重启，DB保存和节点应用分别可见。当前仅规划，不切换当前HTTP簇；R2契约/持久基础（依赖R4/R7及适用R8）→R10队列事务/每任务策略/动态管理→R14真实多节点与迁移验收；[可选管理界面暂不开发](plans/optional-rapid-development-modules.md#开发与使用边界)，不得自动进入。公共服务/CLI和示例随各簇交付，内部版本/快照不增加普通读取负担；唯一执行状态仍见todo。

## 执行校准

1. 当前代码的根 lib/server 和旧 app/example 过渡结构由 R1-S 以34dfb5e收口；R2-M 已将业务代码迁入 modules/example，根模块边界依 ADR-074。模块清单、运行激活、资源与统一迁移的真实验收见本轮 testing；完整模块管理和 R1-E CRUD 不因目录迁移自动完成。
2. 正式定位 Laravel + laravel-modules = OpenRust，以固定上游源码/测试和文档逐项1:1实现功能/可观察语义、默认/错误安全边界、Container/Provider与开发流程，并持续跟进；模块化不是只有示例目录规范。目录、概念、命名、默认行为和扩展流程以 Laravel 为准；Goravel 为实现参考。Rust必要适配、用户约定和授权增强须按[差异分类](plans/laravel-rebuild.md#逐项对应与差异分类)记录原行为/限制/选定行为/证据，未等价保持缺口；版本基线和后继更新见[长期上游跟进](plans/laravel-rebuild.md#长期上游跟进)。既有目录和 API 可以重写，无历史兼容义务。
3. 旧 ADR 中的已实现行为和验收记录作为迁移输入；统一返回格式等优秀机制须融合进 Laravel 对应职责，不能为追求上游外形而删除。新方向不自动使旧记录变成新代码已通过。冲突约束依 ADR-070 替代，功能簇落地时同步领域 ADR/事实。
4. 原 M5/M6、DX/F/G 顺序让位于 R0–R16；未完成生产、集群、SQLite、供应商条件等映射到新矩阵，不丢弃。只一个主功能簇 in_progress，内部允许隔离子任务。
5. 对已定 contract 的实现/测试优先 gpt-6.1-sol；主控定跨模块接口，高风险改码不同模型或 Provider 审查。所有 worker 直接在 main，明确非重叠文件可并行；manifest/lock/迁移/共享文档/提交由集成负责人串行协调，构建 target 与服务资源预先分配；不创建额外源码仓库、开发分支或隔离 worktree，不递归派发、不推送。
6. 根无参数 cargo run、唯一综合示例、生成应用和真实 HTTP 是验收项；根 package 同时包含框架库和默认 server。
7. 日常 PostgreSQL 18/MySQL 8.4 LTS/Redis 8；真实复制、多实例、网络和供应商证据按总计划。最低版本、SQLite、完整集群分别验证。
8. [R15/C32 可选快速开发模块暂不开发](plans/optional-rapid-development-modules.md#开发与使用边界)，R14通过仅技术前置；须用户明确开工，当前框架main和非UI扩展继续。独立 AI模型平台仍未获启动授权。
9. WIP、他人资源、依赖安全、真实证据及对外授权边界继续有效。常规问题自主决策并记录；外部缺口继续独立工作，不能虚报完成。

当前交付依赖是 R1-S结构/挂载 → R2基础Container/Provider/配置/TestApp（A–D 已验收）→ R2-M模块契约/目录迁移 → R1-E真实CRUD与当前能力演示（已由 bc3fa01 提交）→ R2-N模块硬依赖与启动顺序（本簇已验收）→ R3–R16逐簇实现并同步示例。R2-D 测试应用由生产 Provider/Router 同路径构建，但不代表完整模块管理已完成；R1-E 的 Article、租户、附件、通信、观测等生产路径与 R2-N 证据分别记录于 [testing.md](memory/testing.md)。模块完整管理与 Laravel 严格 1:1 范围继续保留。

C04-A/B1/B2与C04-C均按各自子簇验收；C04-C按ADR-081完成Cookie/响应闭环，最终默认668/全特性1241、根双库各48、生成plain3/认证双库各2、真实TLS及适用fmt/Clippy/docs/feature/MSRV/deny通过，产物已清理，本簇限定提交见Git。后继从todo领取C04-D Session/flash与Web栈契约；完整C04仍开放。详细范围与历史证据见[todo](todo.md)及[testing](memory/testing.md)。

## 路由

| 任务 | 入口 |
| --- | --- |
| 新方向与替代关系 | [ADR-070](decisions/adr-070-laravel-first-rebuild.md)、[ADR 索引](decisions/README.md) |
| 宏观执行顺序 | [两阶段框架主线](plans/laravel-rebuild.md#两阶段框架主线)：阶段一固定Laravel+modules基线→阶段二已纳入通用增强，均属框架main；R0–R16为技术分解，原R15版本阶段取消，可选模块不自动开工 |
| 整体范围、阶段、验收 | [全面重构计划](plans/laravel-rebuild.md)、[模块正式矩阵](plans/laravel-modules-matrix.md) |
| Provider包发现后继 | [Provider编译期发现Spec](plans/laravel-rebuild.md#r2r13-provider-编译期发现-spec)；R2契约→R13工具，依赖R2-M/N；仅规划不切换当前HTTP |
| 当前实现资产 | [architecture.md](architecture.md)、core、Cargo.toml、源码和 Git |
| 原能力/生产验收输入 | [原 v1 计划](plans/gen1-plan.md)、[底层补齐](plans/v1-foundation-completion.md)、[集群](plans/v1-cluster-deployment.md)、[原 DX](plans/v1-developer-experience.md) |
| 编码、提交、证据 | [conventions.md](conventions.md)、[testing.md](memory/testing.md)、[verification.md](memory/verification.md) |
| 执行任务文本 | [执行说明](plans/laravel-rebuild-execution.md) |

## 验证入口

每代码簇按 AGENTS.md 跑 fmt、workspace clippy/test/doc、cargo deny，及适用 feature/MSRV、双库、生成项目、真实服务门禁；记录命令与实际结果。文档-only 只做 diff、链接/锚点和旧口径检查。不能把本轮规划检查写成代码或重构已验收。

当前重构来源与 102 章/749 子节当前纳入范围盘点见 [固定基线](plans/laravel-rebuild-baseline.md)；未将旧证据继承为新 contract 通过。
