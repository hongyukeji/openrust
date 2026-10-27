# 当前计划

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：底层目录收尾、F8 数据交换、gRPC G1–G3c 及 M5 实时跨节点在线路由均已有对应验收；当前按 `todo.md` 继续可观测性与生产资源簇。完整 v1 验收通过后进入 v2 后台。

## 当前里程碑

根 `openrust` package 已承载框架模块；原 M0–M2 基础范围及 M3 本地认证/安全运行时验收已有证据。M4/M5 已完成多项队列、调度、缓存、数据库访问、存储、安全、gRPC 与实时通信子簇，**完整 M4 和 v1 尚未完成**，生产 worker 负载、可观测性与全栈集群门禁仍在待办。实时能力已用两个真实 API 子进程和 Redis 8 验证 WebSocket/SSE 跨节点路由、Origin 拒绝、撤销、SIGTERM drain 及 Redis 不可达时 fail-closed。gRPC G3c 已以 Envoy 1.40.1 和两个独立 Linux 节点完成同 Channel HTTP/2 分流、健康摘流、长流中断/重连及节点恢复验收。F7 SSO 已在 PostgreSQL 18/MySQL 8.4、Keycloak 26.8.1、Redis 8 上完成双 client/双应用/每应用双 API 子进程真实验收，涵盖 callback/logout、MFA/logout、refresh/logout 竞态、跨节点凭据拒绝、节点重启重放与 JWT 窗口；隔离 Redis 停机期间返回 503，恢复服务并重启四个 API 副本后，同一真实 Logout Token 重放成功，本簇完整门禁通过。

工具 package 统一位于 `tools/` 下；`performance` 不进入默认成员。examples/tests 已按应用/能力分组；ADR-062 已将宿主迁入根 package。目录分组验收证据见最新架构与 ADR-021。

## 执行校准

**[ADR-039](decisions/adr-039-source-workspace-distribution.md) 的源码 workspace、包命名、生成器、应用启动/配置隔离和双库验收已完成并提交；C1–C3 中文化亦已完成。M3/M4 认证事务扩展、共享锁/幂等、worker 修正、日历/国际化/邮件通知及 M4 生成集成已按双库和 workspace 门禁验收；M5 插件/Db/Queue 扩展、HTTP 生命周期、缓存/Redis Queue、查询授权、迁移互斥、存储/上传下载、受限出站与依赖韧性及 OIDC/TOTP 子簇均按各自证据完成。F7 SSO 已在 Keycloak 26.8.1、PostgreSQL 18/MySQL 8.4 与 Redis 8 上完成双应用×双 API 子进程、TCP 负载均衡、callback/logout 与 MFA/refresh 竞态、真实 Redis 停机 503/恢复后副本重启及 Logout Token 重放；本簇完整门禁通过。F6-A/F6-B 租户上下文、查询隔离与共享资源传播均已完成双库及 workspace 验收；F8-A/F8-B 已完成，F8-B 通过真实 PostgreSQL 18/MySQL 8.4 任务验收及独立 consumer 验证，gRPC G1 基础与生成已完成并通过真实 TCP/consumer 验收，G2a TLS/mTLS、G2b 身份/错误映射和 G2c 调用预算/资源/健康停机均已通过对应真实 TCP 与 workspace 门禁；G3a 联合 HTTP/RPC 生命周期、G3b Proto/trace 兼容与 G3c 双节点 HTTP/2 路由均已完成。M5 WebSocket/SSE 已按 ADR-059 实现，并完成真实 Redis 双 API 路由及故障关闭验收；当前按 `todo.md` 继续可观测性与生产资源簇。

1. 目录收尾、M3 Mailer/Policy 生成器、公开 HTTP 路由限流及数据输出/查询边界、M4 类型化事件/queued listener、ADR-039 源码交付、ADR-047 查询授权和 ADR-048 稳定键分批已完成；后续工作按 `todo.md` 当前优先级推进。
2. 目录和内部 package 统一使用中性职责名，ADR-039 已完成；已完成目录迁移不重做。框架在 `src/`；CLI、proc-macro、测试工具保留独立 package 的技术边界，应用约定 `apps/` 保留；根 package 的 `src/main.rs` 承担集成宿主职责；`apps/` 留给用户业务应用。见 [ADR-021](decisions/adr-021-loco-style-module-layout.md)。
3. `examples/` 和 `tests/` 按示例/能力分目录；不得只移动文件导致 Cargo 不再发现测试。迁移必须比较 targets 与用例数量，验证包路径、宏展开、模板、CI、文档和全部测试入口。
4. 只保留一个实现功能簇 in_progress。每簇先定公开 contract，再实现、匹配验证、本地提交、更新事实。已有实现不重做；未提交 WIP、编译通过、fake、本地双库、完整集群分别报告。
5. M3 Mailer/Policy 生成器、通用路由限流及数据输出/查询边界已补交；事务与 AuthFlows 扩展已按 ADR-041 落地。Worker 租约/停机专项已双库验收；通知、生产 worker 负载和完整集群继续按 `todo.md` 补齐，不能用“阶段已关闭”取消范围。
6. 生命周期复用 `Plugin::build/finish`、Tower 与显式命令 supervisor；worker/scheduler 的取消、drain、join 按 [ADR-037](decisions/adr-037-queue-worker-lifecycle.md)。不建立平行 Hooks/DI/ORM；业务前置校验、提交后事件仍需公开类型化入口。
7. 数据库事务内派发是可靠提交边界；after-commit 内存回调不等于可靠投递，活动任务唯一键也不等于永久请求幂等。任务、通知和业务扩展须分别定义重试、短路、失败及敏感载荷边界。
8. 数据服务日常基线为 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8；暂定最低为 PostgreSQL 14+ / MySQL 8.0+ / Redis 6.2+。最低完整能力、Redis driver 和发布时生命周期仍需验收，不在每簇重复全版本矩阵。先检查复用合适现有服务，规则见 [context.md](context.md#固定约束)。
9. 标准应用默认 Redis，memory/file 显式选用，库默认 features 保持最小；不得静默回退本地状态。数据库/Redis 已部署不等于集群已通过。读写分离、worker/scheduler 单项竞争测试也不替代 [完整集群门禁](plans/v1-cluster-deployment.md)。
10. 每簇对照 [能力覆盖](plans/v1-capability-coverage.md)、[通用能力](plans/v1-common-capabilities.md)、[性能与开箱即用](plans/v1-performance-and-readiness.md)、[集群](plans/v1-cluster-deployment.md) 的相关条目，不只沿最初 M0–M6 简表开发；按 [扩展性自主判断](plans/framework-design.md#开发-agent-的自主扩展性判断) 记录最小合理设计。
11. **v1 全部门禁通过后，已获授权继续 [v2 后台](plans/v2-admin-platform.md)**。Admin 采用 pure-admin 精简国际化版 + Element Plus，App 采用 uni-app，PC 仅规划设计；版本与接入要求见 v2 计划。当前不能提前展开后台，也不需要把旧提示词“本轮永不进入 v2”当作永久限制。发布、tag、push 等仍遵守 `AGENTS.md` 的独立授权边界。
12. [AI模型平台](plans/ai-model-platform.md) 单独维护完整规划，只有用户明确要求开发且成熟门禁满足后才启动；平台开发使用 `projects/ai-model-platform` 与一个独立 worktree，分支直接包含 `ai-model-gateway` 源码；独立插件分支仅在后续实际需要时使用。不将 AI 专用模块加入当前 V1/V2 执行队列。
13. 文档、Rustdoc、代码/配置注释与开发者帮助立即按 [语言规范](style.md#语言规范) 使用简体中文。ADR-039 后按 `todo.md` 的 C1–C3 顺序整改既有英文说明，再继续后续能力；C1–C3 已完成；不新建完整英文文档线，不改变运行时 i18n。
14. 按 [ADR-001](decisions/adr-001-own-kernel-on-axum.md#成熟组件优先与-laravel-式开发体验) 优先复用成熟组件，提供 Laravel 式约定与完整体验；框架维护整合价值和真实可用的公开扩展。已有内核/队列不因偏好重写，参考范围按 V1 四框架、V2 再参考若依/芋道执行；既有公共命名保持有效。按 ADR-001 的 V1 优先原则减少重复底层工作，以常见路径、公开扩展、一致组合和生产证据验收。
15. 审查建议已归入 `todo.md` 对应簇：M4 修 worker 合作停机与内层错误传播；M5 开头先按 [ADR-009](decisions/adr-009-extensible-capability-drivers.md#通用插件交付阶段) 补齐插件公开契约及独立接入，再分别闭合 Db/Queue 外部执行边界，之后交付可选驱动与资源控制；M6 完善插件模板/诊断/升级体验及整体性能验收。静态发现不算新增测试或修复通过。

16. **gRPC 纳入 v1 必交付**，范围以 [ADR-040](decisions/adr-040-grpc-integration.md) 为准；在 M5 按 G1–G3 完成，M6 组合复验。V1 主攻通用底层，V2 现成后台；先闭合 `todo.md` 当前进行中功能簇，再按依赖推进。

17. **底层缺口按 [底层补齐计划](plans/v1-foundation-completion.md) F1–F10 全部交付**。cron、缓存 tag、高级 i18n、轻量 fixtures、租户通用隔离和外部身份接入等旧后置/排除口径已校准；具体 API/默认语义先定 Spec，不再重复评估是否需要交付。仅后续适配清单保持评估状态。

18. **F7 包含 [SSO 登录与统一退出](plans/v1-foundation-completion.md#sso-登录与统一退出)**：OIDC、TOTP/恢复码及双应用 SSO/退出的真实双库多进程专项、Redis 故障恢复和本簇门禁均已通过；F8-A/F8-B、gRPC G1–G3c 和 ADR-059 实时路由均已完成对应验收，M6 按完整集群计划复验。JWT 撤销窗口与共享状态边界按 ADR-055 报告。

## 里程碑顺序

| 顺序 | 工作 | 完成门槛 |
| --- | --- | --- |
| 当前 | M5 可观测性与生产资源 | readiness、低基数 metrics/otel、依赖健康和并发资源边界、性能基线 |
| M4 | 流程扩展、共享锁/幂等、worker 修正、日历调度、国际化/邮件模板与队列通知 | 双库并发/故障、DST/补跑、模板/通知、job 生成与组合运行 |
| M5 | 插件公开契约与外部扩展、运行/迁移保障、缓存/存储、查询/租户、外部身份/MFA、数据交换、gRPC/实时/观测 | 独立扩展、真实驱动、跨协议/租户隔离、身份协议、双库与多节点资源边界 |
| M6 | 插件模板/诊断/升级、CLI/dev/doctor、应用模板覆盖、fixtures/TestApp、OpenAPI、示例与正式文档 | 双库黄金路径、源码升级、构建成本及兼容/集群/性能门禁 |
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
