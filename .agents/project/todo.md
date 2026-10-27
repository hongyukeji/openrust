# 活跃待办

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：只列未完成工作；已完成事项不在此重复记载。

## 领取规则

领取功能前核对 Git/WIP 和 [执行校准](plan.md#执行校准)。当前实现范围以本表为准，v1 总体验收见 [底层补齐计划](plans/v1-foundation-completion.md)。

## M5：共享基础设施、通信与运行保障

| 功能簇 | 当前工作与边界 | 验收 |
| --- | --- | --- |
| 可观测性与生产资源 | ADR-060 密码哈希容量、ADR-061 readiness 和低基数指标/OTLP trace 已实现；真实 Collector、TCP scrape、双库登录突发与过载恢复均有记录。开发机 HTTP 基线、CPU/RSS 采样及 overload 指标见 [`tools/performance/README.md`](../../tools/performance/README.md)。剩余工作为独立审查及 v1 完整生产/集群门禁；不重做 readiness 探针 | metrics 默认关闭、显式开启后标签低基数；readiness 故障/恢复、请求体结束后的资源释放和过载恢复有真实验证；适用 workspace 门禁与压力基线记录完整，跨节点及生产门禁留在 v1 总体验收 |

## M6：开发体验与 v1 最终验收

按 [开发体验实施计划](plans/v1-developer-experience.md) DX1–DX8 的依赖顺序细化下列既有任务：默认 Run/Debug → TestApp → fixtures → 业务生成 → 插件/只读诊断 → 事务通知与上下文组合 → dev 编排 → 示例与升级。先闭合当前 M5 簇，每次只领取一个子簇；已实现部分核对后复用。IDE 同一配置 Command 仅为 `run` 的普通启动和断点调试必须实际验证。

- CLI：补 controller/scaffold/command 等剩余生成器；按 [底层补齐计划](plans/v1-foundation-completion.md) F9 提供模板清单/导出/预览/应用覆盖、变量版本与冲突诊断；补 routes、只读 doctor、`db status` 和显式 `db reset`；复用已有 config show/migrate/seed。status/doctor 不写数据；reset 另定目标环境、确认和数据清理边界，在隔离库测试，不由 doctor/dev 隐式触发。
- 插件开发体验：沿用 M5 公开契约完善插件模板/示例、只读依赖/配置诊断、兼容与升级说明；生成独立宿主后 check/test，验证默认隔离及配置/迁移升级、停用保留数据；并入现有 CLI、测试与源码升级验收，不创建第二套装配或插件管理器。
- 开发编排：dev 先探测并复用合适服务，缺失时才建本任务资源；按显式配置处理迁移、watch、信号、端口冲突和进程回收。当前 dev 只是代理 serve，不能宣传自动编排已完成。
- 测试数据 F10：生成显式 builder/fixtures，支持属性覆盖、批量上限、关系组合与固定种子；复用 TestDatabase，双库验证保存/约束/清理，不引入隐式 Factory trait。
- 测试体验：TestApp/HTTP assertions 复用 finalize/build_router/Tower oneshot 与 TestDatabase；fake 显式替换，补邮件/SMS/queue/HTTP 调用断言及 gRPC fixture。TTL/验证码/退避边界优先纯时间参数或 Tokio 可控时间，必要时局部时钟；真实数据库共享时钟另测，不能靠 sleep 或本机冻结替代集群验证。OpenAPI、错误/分页 schema、文档端点与授权范围一致。
- 交付：`apps/example` 综合应用、双库生成黄金路径、Docker/Compose、备份恢复/滚动升级/迁移说明、mdBook 与迁移对照；验证完整源码分发、workspace/path 依赖与 apps 生成/启动，不依赖已发布内部包或临时 registry patch。独立下游 API 与多进程拓扑验收工程统一放 `tests/consumers/`。
- 使用与升级验收：由未参与该功能实现的走查者仅凭中文文档完成首次启动、增加业务资源、更换驱动、测试、诊断、部署和一次源码升级；记录环境、步骤、耗时与阻塞点。独立 Agent 走查注明身份，不能冒充真实用户测试。用临时源码副本保留用户 apps/配置/迁移升级，验证冲突处理及回滚边界，详见 [升级与维护](plans/v1-performance-and-readiness.md#8-源码升级与维护约定)。
- 构建与维护约定：测量无数据库/单驱动/标准应用的依赖、干净及业务增量构建和产物体积；按证据优化，不仅为数字拆包。明确 OS/架构支持矩阵、框架支持与安全修复/弃用窗口、API/schema 与客户端兼容规则；现有 `/api` 版本策略继续有效。
- 最终门禁：默认/最小/关键/all-features、MSRV、依赖审计、双库真实所有数据路径、最低兼容版本；至少两个 API 加多 worker/scheduler 经负载均衡验证共享状态、实时与文件、故障/滚动恢复；代表性性能基线与回归阈值。
- 加密：按 ADR-034 复核选用实现的审计证据、依赖与目标平台性能；算法标准不能替代实现审计。外部供应商真实短信需要授权账号/目标，未验证如实标注，不能把 v1 最终门禁报告为全部通过。
- 生成器各原定里程碑交付，M6 只做全集复验，不借“最终统一做”遗漏 M3/M4 交付。发布/tag/推送由独立授权处理。

## v1 通过后的已授权下一阶段

按 [v2 后台计划](plans/v2-admin-platform.md) 执行 B1–B5：Admin 使用 pure-admin 精简国际化版 + Element Plus，App 使用 uni-app，PC 仅规划设计；B1 锁定版本与 Rust API 适配。候选功能仍先评估，不因列出即全部开发。[AI模型平台](plans/ai-model-platform.md) 单独规划，只有用户明确要求开发且成熟门禁满足后才启动；不进入当前 V1/V2 实现任务，不创建源码、分支或 worktree。

## 外部条件与证据缺口

- Twilio 实发缺授权账号、凭据与目标；可继续不依赖实发的工作，不能代称已投递。
- 完整 Redis/对象存储/跨节点实时、最低版本整体兼容、集群与性能门禁尚未完成。可运行基础服务或单项进程测试不能替代这些证据。
