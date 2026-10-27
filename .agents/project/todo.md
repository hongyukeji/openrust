# 活跃待办

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：只列未完成工作；完成移出本表，事实与证据进入 `core/`、`progress.md`、`memory/` 和 Git。一次只领取一个实现功能簇。F6-B 租户资源传播及 F8-A/B 数据交换已完成并记录验收证据。

## 领取规则

先核对 Git/WIP 和 [执行校准](plan.md#执行校准)。数据库队列、interval/UTC daily 调度、活动唯一键及有限任务链/批次已提交，不重复实现。共享锁与请求幂等已按 ADR-042 完成双库验收；Worker 停机修正和 F1 日历调度均已按 PostgreSQL/MySQL 专项验收；F2 国际化、F3 安全邮件模板、M4 队列化通信与薄通知层均已完成适用验收。M4 生成、M5 通用插件公开契约、Db/Queue 外部扩展契约、跨协议验证/调用作用域及 HTTP 有界运行/就绪切换均已完成对应专项和 workspace 验收。F4b 单进程同键加载合并与 F4c SWR 已完成并提交；命名数据库连接实例已按 ADR-045 完成 PostgreSQL 18/MySQL 8.4 实际验收；Redis Session 驱动已通过真实 Redis 验收；显式同步任务执行已按 ADR-043 收口；ADR-046 Redis Queue 已通过真实 Redis 8.10.2 多连接竞争、失败重试、强杀恢复、server Redis-only 接入和 workspace 门禁。ADR-047 查询授权与 ADR-048 稳定键分批均已完成双库与独立 consumer 验收；ADR-049 迁移互斥已通过双库多进程验收；ADR-050 存储与上传下载、ADR-051 出站目标策略及 ADR-052 命名依赖韧性已完成适用的真实服务和 workspace 验收；gRPC G1 已完成真实 TCP、生成项目及独立 consumer 验收。完整集群仍属于后续门禁。

当前工作区另有 `.agents/project/memory/verification.md` 与 F7 MFA 验收相关的未提交 WIP；本功能簇保留且不纳入提交。

V1 参考 Loco、Laravel、Goravel、OpenSpring；若依/芋道仅 V2。新增必交范围与契约见 [底层补齐计划](plans/v1-foundation-completion.md)，当前进行中功能簇保持不变；可选启用的能力仍须开发验收。

## M4：异步与流程扩展

按下表顺序拆成可独立提交的功能簇；已经存在的基础 API 先核查，不重复开发。

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| 共享锁与请求幂等（ADR-042） | 已交付 `SharedLocks`、fencing/事务内检查、显式 `RequestIdempotency` 与有界 JSON replay/清理；无 Redis 依赖 | PostgreSQL 18/MySQL 8.4 真实隔离库、多 Db pool 并发竞争、接管/旧 owner 防护、scope/指纹冲突、业务事务回滚与结果回放通过 |
| Worker 租约与停机修正 | 按 ADR-037 修正 `work_once` 合作取消等待、shutdown deadline、token 条件重试；supervisor 收到信号后 drain、join 全部 worker 并传播错误。PG18/MySQL8.4 真实 SIGTERM 验证合作任务确认、不合作任务有界超时并释放 lease、同队列 pending job 不再领取；关闭 worker 连接池模拟数据库不可用时 supervisor 返回错误且原 lease 保留；双库旧 token 接管后 stale handler 不能确认新 worker 结果；强杀恢复测试复验 | 本地双库功能验收完成；生产多实例负载及完整集群留在 M4/M6 门禁 |
| 国际化表达与地区格式化 F2 | ICU4X 2.3.x 支持 CLDR 复数、有限 select、精确小数与显式时区日期；普通文本和 `{name}` 兼容 | 中文/英文/波兰语、零/小数/缺参/回退、并发隔离、跨时区、干净源码生成 check/test 和 workspace 门禁通过；不改变协议字段及运行时 locale 契约 |

## M5：共享基础设施、通信与运行保障

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| 通用插件公开契约 | 按 [ADR-009](decisions/adr-009-extensible-capability-drivers.md#通用插件交付阶段) 补齐应用配置、服务、命令与路由组合、缺依赖/冲突错误语义及资源归属；复用静态 `Plugin::build/finish`，不另建插件生命周期系统 | 独立 consumer 仅用公开 API 完成注册、配置、执行和错误验证；未启用功能隔离；不依赖 AI 项目；workspace 门禁通过 |
| Db 外部扩展契约 | 在通用插件 contract 后复用 SeaORM/SeaQuery 建立公开最小 driver 边界；区分可用 backend 的连接/事务与不支持能力，官方数据库矩阵不自动扩大 | 独立 consumer 不改核心即可配置和连接；事务/错误双库验证；既有 PostgreSQL/MySQL 行为无回归 |
| 有界运行与就绪切换 | 已完成 HTTP listener/task 所有权、就绪状态、启动失败清理和限时退出；gRPC/实时后续复用，按 [生产约定](plans/v1-performance-and-readiness.md#6-生产默认值与开发编排) | 定向 HTTP TCP 生命周期测试、真实 `server` 子进程 SIGTERM、默认/all-features workspace 与其余门禁通过；不重开通用 Plugin shutdown |
| Redis Queue 驱动 | 已按 [ADR-046](decisions/adr-046-redis-queue-driver.md) 实现可选 `queue-redis`；数据库默认保持不变，Redis/数据库不承诺跨资源原子性 | Redis 8.10.2 真实服务、多连接竞争与强杀恢复已完成；Redis Cluster 分片、最低版本和完整集群留在发布/集群门禁 |
| 数据访问与生产迁移：稳定键有界分批 | 已按 [ADR-048](decisions/adr-048-stable-batch-reading.md) 交付 `BatchReader` 与授权查询接入 | PostgreSQL 18/MySQL 8.4 已真实验证单列/复合主键、升降序、多页、取消续读、callback 失败、scope 过滤及连接可继续使用；转入迁移互斥 |
| 数据访问与生产迁移：迁移互斥 | 已按 [ADR-049](decisions/adr-049-migration-execution-lock.md) 以 resolver-aware 独立锁表和事务行锁保护 MigrationPlugin 的 up/down/status | PostgreSQL 18/MySQL 8.4 各 4 项真实 `db_migration` 集成通过，包含多进程互斥、等待超时、owner 强杀恢复、具名 DB 隔离及 up/down/status；fmt/clippy/default 与 all-features tests/docs/no-default/cargo deny 均通过；显式双库 targets 通过 |
| 可选外部身份与 MFA F7 | OIDC 子簇按 ADR-053 已提交 `d0b0c74`；TOTP/恢复码按 ADR-054 完成并提交，复用 AuthFlows、本地密钥、限流与审计，不建设授权服务器 | PostgreSQL 18/MySQL 8.4 实际 TCP/隔离库各 1 项通过，覆盖 RFC 向量、密码/OIDC challenge、Session 阻断、来源/过期/尝试边界、TOTP replay、恢复码单次消费、密钥轮换、审计与撤销；干净源码副本双库生成 check/test 通过；SSO 独立子簇仍待完成 |
| SSO 登录与统一退出 F7 **completed** | 已实现 ADR-055 的关联撤销、MFA/refresh/Session 绑定、RP logout state 与 Back-Channel Logout 验证/重放；provider fake 双库、Keycloak 26.8.1 双 client 真实 SSO 与退出均有验收。PostgreSQL 18/MySQL 8.4 各以两个隔离应用、每应用两个独立 API 子进程、真实 TCP round-robin、Redis 8 共享 Session 完成 callback、跨节点 MFA/TOTP、RP logout、真实 logout handler 与 callback/logout、MFA/logout、refresh/logout 竞态；退出后两个子节点都实际拒绝读取 Session/轮换 refresh。节点重启后重放幂等；15 秒 access JWT 在退出后仍可验签并于 exp 后拒绝。双库 OIDC retry 用例在真实 Redis store 注入一次删除错误后重放成功。 | 隔离 Redis 容器真实停机时通知返回 503；恢复服务、重启 PostgreSQL/MySQL 各自的四个 API 子进程后，同一 Keycloak Logout Token 重放成功，两个数据库上的真实拓扑验收通过。fmt、default/all-features clippy/test、warnings-denied Rustdoc、no-default check 与 cargo deny 均通过；双库真实 Keycloak/Redis 故障验收通过。M6 仍复验完整产品集群与滚动升级 |
| 可选租户隔离 F6-A | 已完成 `TenantId`/`TenantContext`、可信 HTTP `TenantLayer` 和 `TenantQueryRule`；应用显式解析可信身份，默认关闭，不改 schema、不拦截原生 SQL/裸 ORM | PostgreSQL 18/MySQL 8.4 各两个真实 TCP API 节点；验证伪造 header 不改变解析、用户 scope 与 tenant 相交、关联及批量 update/delete 不串租、缺上下文拒绝；workspace 门禁通过并本地提交 |
| gRPC G2 安全与运行 | 复用验证/身份/授权、TLS/mTLS、Status、deadline/取消、全局与流限额、背压、health/reflection、退出 | 错证书/凭据拒绝、调用预算实际传播、慢消费与异常断连、SIGTERM/资源释放；有数据库认证的路径双库验证 |
| gRPC G3 集群与组合 | HTTP/RPC 显式联合运行与生成项目、双节点 HTTP/2 负载分配、摘流/滚动/重连、proto 新旧互通与追踪关联 | 联合启动失败清理；真实多节点四类调用和流终止语义，反射生产保护；M6 只做整体复验，不能用 unary hello 关闭范围 |
| WebSocket/SSE | 复用 auth/config/HTTP；握手与频道授权、Origin、凭据过期/撤销、心跳/背压/连接与消息大小边界；共享在线跨节点投递 | 两 API 节点真实通信、断线/慢消费/停机、权限变更、资源回收；不承诺离线可靠消息 |
| 可观测性与生产资源 | readiness、指标与可选 OTLP、HTTP/gRPC/worker/DB/cache/实时有界并发与超时；密码哈希阻塞任务增加明确容量与排队/拒绝策略，单次参数限制不等于总资源有界；安全脱敏和 HTTP→RPC/HTTP→queue/worker 的 trace 关联 | 依赖失效 readiness、低基数指标、登录突发与共享数据库竞争下的过载/恢复；p95/p99、错误率、CPU/内存及资源释放证据，不能只写指标接口 |

代表性业务路径从 M4 起随能力推进：沿用 `examples/blog` 目标逐步串联账号、权限、CRUD、异步通知，再接缓存、上传与实时；不预建未实现能力的空壳，不把全部组合问题留到 M6。测量与体验验收见 [补强计划](plans/v1-performance-and-readiness.md)。

## M6：开发体验与 v1 最终验收

- CLI：补 controller/scaffold/command 等剩余生成器；按 [底层补齐计划](plans/v1-foundation-completion.md) F9 提供模板清单/导出/预览/应用覆盖、变量版本与冲突诊断；补 routes、只读 doctor、`db status` 和显式 `db reset`；复用已有 config show/migrate/seed。status/doctor 不写数据；reset 另定目标环境、确认和数据清理边界，在隔离库测试，不由 doctor/dev 隐式触发。
- 插件开发体验：沿用 M5 公开契约完善插件模板/示例、只读依赖/配置诊断、兼容与升级说明；生成独立宿主后 check/test，验证默认隔离及配置/迁移升级、停用保留数据；并入现有 CLI、测试与源码升级验收，不创建第二套装配或插件管理器。
- 开发编排：dev 先探测并复用合适服务，缺失时才建本任务资源；按显式配置处理迁移、watch、信号、端口冲突和进程回收。当前 dev 只是代理 serve，不能宣传自动编排已完成。
- 测试数据 F10：生成显式 builder/fixtures，支持属性覆盖、批量上限、关系组合与固定种子；复用 TestDatabase，双库验证保存/约束/清理，不引入隐式 Factory trait。
- 测试体验：TestApp/HTTP assertions 复用 finalize/build_router/Tower oneshot 与 TestDatabase；fake 显式替换，补邮件/SMS/queue/HTTP 调用断言及 gRPC fixture。TTL/验证码/退避边界优先纯时间参数或 Tokio 可控时间，必要时局部时钟；真实数据库共享时钟另测，不能靠 sleep 或本机冻结替代集群验证。OpenAPI、错误/分页 schema、文档端点与授权范围一致。
- 交付：`examples/blog`、双库生成黄金路径、Docker/Compose、备份恢复/滚动升级/迁移说明、mdBook 与迁移对照；验证完整源码分发、workspace/path 依赖与 apps 生成/启动，不依赖已发布内部包或临时 registry patch。
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
