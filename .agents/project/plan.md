# 当前计划

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：先完成底层目录收尾，再按依赖补齐 v1；v1 完整验收通过后继续 v2 后台。当前交付见 [progress.md](progress.md)，唯一执行队列见 [todo.md](todo.md)。

## 当前里程碑

根 `openrust` package 已承载框架模块；原 M0–M2 基础范围及 M3 本地认证/安全运行时验收已有证据，Mailer/Policy 生成器和公开 HTTP 路由限流已实现并验证。M4 已交付数据库队列/worker、UTC 调度、活动任务唯一键与有限任务链/批次。**这不代表完整数据输出/查询边界、完整 M4 或 v1 完成**。状态只按源码、提交和相应验收判断。

目录收尾已完成：工具包位于 `cli/`、`macros/`、`testing/`，examples/tests 按应用/能力分组；迁移前后 workspace package 数与 265 个可列测试条目一致，server 保留为真实进程/双库宿主。完整验证与提交见 [progress.md](progress.md) 和 [目录收尾 Spec](plans/module-layout-migration-spec.md)。

## 执行校准

1. 目录收尾、M3 Mailer/Policy 生成器与公开 HTTP 路由限流已完成；当前按 `todo.md` 审计数据输出/查询边界，之后恢复 M4。
2. 目录中性化与 Cargo package 名是不同决定。框架在 `src/`；CLI、proc-macro、测试工具保留独立 package 的技术边界，迁出顶层 `crates/`。应用约定 `apps/` 保留；`apps/server` 逐项审计启动、feature、双库、命令与进程测试职责，替代覆盖通过前保留。见 [ADR-021](decisions/adr-021-loco-style-module-layout.md)。
3. `examples/` 和 `tests/` 按示例/能力分目录；不得只移动文件导致 Cargo 不再发现测试。迁移必须比较 targets 与用例数量，验证包路径、宏展开、模板、CI、文档和全部测试入口。
4. 只保留一个实现功能簇 in_progress。每簇先定公开 contract，再实现、匹配验证、本地提交、更新事实。已有实现不重做；未提交 WIP、编译通过、fake、本地双库、完整集群分别报告。
5. M3 Mailer/Policy 生成器与通用路由限流已补交；数据输出/查询边界与跨事务业务流程扩展点仍按 `todo.md` 补齐，不能用“阶段已关闭”取消范围。
6. 生命周期复用 `Plugin::build/finish`、Tower 与显式命令 supervisor；worker/scheduler 的取消、drain、join 按 [ADR-037](decisions/adr-037-queue-worker-lifecycle.md)。不建立平行 Hooks/DI/ORM；业务前置校验、提交后事件仍需公开类型化入口。
7. 数据库事务内派发是可靠提交边界；after-commit 内存回调不等于可靠投递，活动任务唯一键也不等于永久请求幂等。任务、通知和业务扩展须分别定义重试、短路、失败及敏感载荷边界。
8. 数据服务日常基线为 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8；暂定最低为 PostgreSQL 14+ / MySQL 8.0+ / Redis 6.2+。最低完整能力、Redis driver 和发布时生命周期仍需验收，不在每簇重复全版本矩阵。先检查复用合适现有服务，规则见 [context.md](context.md#固定约束)。
9. 标准应用默认 Redis，memory/file 显式选用，库默认 features 保持最小；不得静默回退本地状态。数据库/Redis 已部署不等于集群已通过。读写分离、worker/scheduler 单项竞争测试也不替代 [完整集群门禁](plans/v1-cluster-deployment.md)。
10. 每簇对照 [能力覆盖](plans/v1-capability-coverage.md)、[通用能力](plans/v1-common-capabilities.md)、[性能与开箱即用](plans/v1-performance-and-readiness.md)、[集群](plans/v1-cluster-deployment.md) 的相关条目，不只沿最初 M0–M6 简表开发；按 [扩展性自主判断](plans/framework-design.md#开发-agent-的自主扩展性判断) 记录最小合理设计。
11. **v1 全部门禁通过后，已获授权继续 [v2 后台](plans/v2-admin-platform.md)**。当前不能提前展开后台，也不需要把旧提示词“本轮永不进入 v2”当作永久限制。发布、tag、push 等仍遵守 `AGENTS.md` 的独立授权边界。
12. AI 平台仍仅规划：框架 v1/v2 成熟后另行明确启动，产品只在 `projects/*` 分支与独立 worktree 开发；P0–P8 和上线门禁通过并获上线批准后才开放。详见 [产品隔离规则](plans/ai-platform-product.md#分支与工作区硬性约束)。

## 里程碑顺序

| 顺序 | 工作 | 完成门槛 |
| --- | --- | --- |
| 优先 | 目录与验收入口收尾 | ADR-021 / 迁移 Spec；没有遗失 tests/targets，生成项目和双库入口仍可运行 |
| 补齐 M3 | 通用限流/查询/输出边界（Mailer/Policy 生成器已完成） | 公开 API 和双库验证；跨事务/事件部分随 M4 补齐 |
| M4 | 事件、事务/认证扩展、共享锁/请求幂等、任务恢复补强、队列通知 | 数据库真实竞争与故障测试，job 生成器，启用 queue 的双库 CI |
| M5 | 缓存、多实例、存储上传、WebSocket/SSE、可观测性 | Redis/file/memory 契约、对象存储、跨节点与资源边界 |
| M6 | CLI/dev/doctor、TestApp、OpenAPI、完整生成器、示例与正式文档 | 双库黄金路径、默认/最小 features/MSRV、兼容/集群/性能/发布准备全部通过 |
| v2 | B1–B5 现成后台 | 复用成熟 v1，组织/菜单/后台 UI 留在应用层，双库与集群验收 |

专题计划定义范围，`todo.md` 定义当前顺序和领取状态；不得建立第二套活跃任务队列。

## 路由

| 任务类型 | 入口 |
| --- | --- |
| 架构与公开 API | [architecture.md](architecture.md)、[框架设计](plans/framework-design.md)、[ADR 索引](decisions/README.md) |
| 编码、Git 与验证 | [conventions.md](conventions.md)、[testing.md](memory/testing.md) |
| 依赖与版本 | [stack.md](stack.md) |
| 范围与验收 | [v1 计划](plans/gen1-plan.md)、[verification.md](memory/verification.md) |
| 后续版本 | [roadmap.md](roadmap.md) |

## 验证入口

代码功能簇按 `AGENTS.md` 执行 fmt、workspace clippy/test/doc、cargo deny；数据能力另有 PostgreSQL/MySQL 真实测试，模板变更生成项目 check/test。完整命令只在 [testing.md](memory/testing.md) 维护。文档功能簇只执行 diff、链接和旧引用检查，不把历史代码验收算成本轮运行。
