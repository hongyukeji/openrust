# 活跃待办

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：只列未完成工作；完成移出本表，事实与证据进入 `core/`、`progress.md`、`memory/` 和 Git。一次只领取一个实现功能簇。

## 领取规则

先核对 Git/WIP 和 [执行校准](plan.md#执行校准)。数据库队列、调度、活动唯一键及有限任务链/批次已提交，不重复实现。若存在并行 Agent 的 WIP，先保留并闭合其当前最小簇，再按以下顺序继续。共享锁与请求幂等已按 ADR-042 完成双库实现和验收；Worker 停机修正已完成进程内/双库 SIGTERM 基础验收，后续唯一 in_progress 为日历调度 F1。

V1 参考 Loco、Laravel、Goravel、OpenSpring；若依/芋道仅 V2。新增必交范围与契约见 [底层补齐计划](plans/v1-foundation-completion.md)，当前进行中功能簇保持不变；可选启用的能力仍须开发验收。

## M4：异步与流程扩展

按下表顺序拆成可独立提交的功能簇；已经存在的基础 API 先核查，不重复开发。

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| 共享锁与请求幂等（ADR-042） | 已交付 `SharedLocks`、fencing/事务内检查、显式 `RequestIdempotency` 与有界 JSON replay/清理；无 Redis 依赖 | PostgreSQL 18/MySQL 8.4 真实隔离库、多 Db pool 并发竞争、接管/旧 owner 防护、scope/指纹冲突、业务事务回滚与结果回放通过 |
| Worker 租约与停机修正 | 按 ADR-037 修正 `work_once` 合作取消等待、shutdown deadline、token 条件重试；supervisor 收到信号后 drain、join 全部 worker 并传播错误。PG18/MySQL8.4 真实 SIGTERM 验证合作任务确认、不合作任务有界超时并释放 lease、同队列 pending job 不再领取；关闭 worker 连接池模拟数据库不可用时 supervisor 返回错误且原 lease 保留；双库旧 token 接管后 stale handler 不能确认新 worker 结果；强杀恢复测试复验 | 本地双库功能验收完成；生产多实例负载及完整集群留在 M4/M6 门禁 |
| 日历调度 F1 **in_progress** | 按 [底层补齐计划](plans/v1-foundation-completion.md) 交付 cron/IANA/DST 与有界补跑；先定语法/时区/错过触发/计划变更语义，复用 ADR-037 数据库时钟与 slot 派发 | 双库 DST 重复/缺失、跨日停机、补跑上限、计划变更及双节点去重；不能用原 UTC daily 关闭任务 |
| 国际化与邮件模板 F2/F3 | 分两簇：同一 catalog 增加复数/选择/地区格式化；安全邮件模板/布局复用 Mailer、lettre 与生成器 | 多语言/数字日期与并发隔离；转义/缺参、multipart、固定重试内容、生成项目及本机 SMTP 实收 |
| 队列化通信与薄通知层 | 序列化邮件/SMS 作业与多渠道编排；复用 Mailer/SmsSender、locale、队列 retry；不新增独立重试系统 | 同事务派发、失败/重复投递边界、fake 与真实本机 SMTP；Twilio 实发仍需授权目标与凭据 |
| M4 生成与集成收口 | `generate job`、可运行 API/worker/scheduler 组合示例；双库 CI 显式启用 queue 并运行进程测试 | `.github/workflows/ci.yml` 原数据库作业只启用 auth/db；当前结构簇已补 queue，验证时证明 queue/schedule/崩溃测试确实执行，缺 URL 不得作为通过；完成 M4 全部门禁 |

## M5：共享基础设施、通信与运行保障

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| 数据库与队列外部扩展契约 | 在可选 driver 实现前补齐 ADR-009 的公开接入。Db 初始化被 `DatabaseTableNames::validate_backend` 限制为双库，复用 SeaORM/SeaQuery 形成最小扩展边界；Queue 的 JobContext/registry/进度绑定数据库，补可复用 typed Job 的装配与执行契约。分别成簇，保留现有双库实现，不创建第二套 ORM/队列系统，不直接暴露内部租约令牌绕过安全规则 | 独立 consumer 不改核心、仅用公开 API 完成注册/配置/执行/错误测试；其他 SeaORM backend 的基础连接/事务与不支持能力明确区分，官方矩阵不自动扩大；非数据库队列 fixture 可执行同一 Job 并验证取消/进度，不支持事务派发时显式拒绝；原双库行为无回归 |
| 跨协议验证与调用作用域 | [补齐计划](plans/v1-common-capabilities.md#5-协议无关验证与调用作用域)：复用 validator/Error、AppState 与现有上下文，补公开转换、剩余预算/取消/trace/locale 规则；不持久化请求凭据 | HTTP/命令/Job 同规则、并发身份/语言隔离与任务独立生命周期；gRPC 在 G2 接入并复验 |
| 有界运行与就绪切换 | 先在现有 HTTP 命令补清楚的 listener/task 所有权、就绪状态、启动失败清理和限时退出；gRPC/实时后续复用，按 [生产约定](plans/v1-performance-and-readiness.md#6-生产默认值与开发编排) | HTTP 长请求/后台任务、SIGTERM、drain 超时、取消/join 和失败退出码；不用通用 Plugin shutdown 重开已有决策 |
| 缓存驱动与命名实例 | ADR-014/015：标准应用 Redis 默认，memory/file 显式选择；default/named stores、custom driver、TTL/序列化/原子边界；库 Redis feature 可选 | 三驱动契约、真实 Redis、file 安全路径/过期/原子替换/并发/错误、独立扩展示例；Redis 故障不隐式回退 |
| 高级缓存 F4 | 基础 Cache 与共享锁后交付 tag、SWR、显式同键加载合并；按 [底层补齐计划](plans/v1-foundation-completion.md) 明确能力与作用域 | Redis 双节点失效/回填竞争、最大陈旧窗口、失败/取消及停机回收；不伪称跨节点恰好一次 |
| 多实例与可选驱动 | 明确默认/named database、cache、storage 等实例与生命周期；数据库已完成的主从路由不等于多个独立连接；可选 Redis Session/queue 及 sync 测试执行复用既有契约，先定与数据库 driver 不同的能力边界 | 实例命名/重复注册/未知名称/配置错误；会话过期/撤销、Redis 队列恢复与真实服务测试；sync 显式使用，不伪装持久化或跨后端事务派发；不引入全局 service locator |
| 数据访问、查询授权 F5 与生产迁移 | 在既有组合任务补公开类型化授权规则，覆盖列表/详情/修改/删除/批量/关联；稳定键分批及迁移互斥分别成簇 | 双库权限与用户过滤 AND 合并、失败拒绝、原生 SQL 边界；有界数据处理和双进程迁移故障恢复 |
| 存储与上传下载 | local/S3-compatible、流式 I/O、限额/MIME/路径与签名下载；上传前、内容可读发布前、完成/失败扩展 | MinIO 实集成、受控临时对象、检测拒绝/超时/取消清理、版本绑定与直传绕过防护；集群文件可达 |
| 出站目标安全策略 | [补齐计划](plans/v1-common-capabilities.md#6-出站目标安全与调用韧性)：显式受限 URL 策略、DNS/实际连接/重定向/代理边界及可信内部端点白名单，先落现有 HTTP client | 可控解析与本地网络测试拒绝绕过、凭据跨目标传播；策略失败关闭；默认内部服务不被误阻断 |
| 命名依赖隔离与可选熔断 | 复用 Tower/现有 HTTP 重试与资源约定，补有界并发/排队、熔断/半开、总预算、抖动及 Retry-After；gRPC G2 复用适用策略 | 故障依赖不耗尽另一依赖容量；取消释放、半开恢复、超时预算与幂等/流不可重放约束；不叠加重试循环 |
| 可选外部身份与 MFA F7 | 出站策略/认证扩展后分簇交付 OAuth2/OIDC/SSO 客户端与 TOTP/恢复码；复用 AuthFlows/密钥/审计，不建设授权服务器 | 真实兼容 IdP、回调重放/issuer/PKCE/JWKS、绑定安全、挑战/恢复码竞争及双库状态；MFA 前不授完整权限 |
| 可选租户隔离 F6 | 调用作用域、F5、缓存/队列/存储齐备后交付可信租户解析、公开隔离接入与审计；普通应用默认关闭 | 双库和两 API 节点下查询、关联/批量、缓存/锁、文件/任务不串租；缺上下文拒绝，RPC/实时随后复验 |
| 通用 CSV/XLSX F8 | F5/F6 与存储/队列后，分别实现解析转换与任务化集成；不建业务页面 | 列映射/逐行错误、分批事务、压缩展开/公式注入、取消与内存边界；双库导入、授权导出与报告清理 |
| gRPC G1 基础与生成 | [ADR-040](decisions/adr-040-grpc-integration.md)：可选 tonic/prost、配置/注册、默认/命名客户端、四种 RPC 与应用 proto 生成 | 无库真实 TCP 四种调用、独立 consumer；生成项目 check/test，默认构建无需 gRPC/protoc |
| gRPC G2 安全与运行 | 复用验证/身份/授权、TLS/mTLS、Status、deadline/取消、全局与流限额、背压、health/reflection、退出 | 错证书/凭据拒绝、调用预算实际传播、慢消费与异常断连、SIGTERM/资源释放；有数据库认证的路径双库验证 |
| gRPC G3 集群与组合 | HTTP/RPC 显式联合运行与生成项目、双节点 HTTP/2 负载分配、摘流/滚动/重连、proto 新旧互通与追踪关联 | 联合启动失败清理；真实多节点四类调用和流终止语义，反射生产保护；M6 只做整体复验，不能用 unary hello 关闭范围 |
| WebSocket/SSE | 复用 auth/config/HTTP；握手与频道授权、Origin、凭据过期/撤销、心跳/背压/连接与消息大小边界；共享在线跨节点投递 | 两 API 节点真实通信、断线/慢消费/停机、权限变更、资源回收；不承诺离线可靠消息 |
| 可观测性与生产资源 | readiness、指标与可选 OTLP、HTTP/gRPC/worker/DB/cache/实时有界并发与超时；密码哈希阻塞任务增加明确容量与排队/拒绝策略，单次参数限制不等于总资源有界；安全脱敏和 HTTP→RPC/HTTP→queue/worker 的 trace 关联 | 依赖失效 readiness、低基数指标、登录突发与共享数据库竞争下的过载/恢复；p95/p99、错误率、CPU/内存及资源释放证据，不能只写指标接口 |

代表性业务路径从 M4 起随能力推进：沿用 `examples/blog` 目标逐步串联账号、权限、CRUD、异步通知，再接缓存、上传与实时；不预建未实现能力的空壳，不把全部组合问题留到 M6。测量与体验验收见 [补强计划](plans/v1-performance-and-readiness.md)。

## M6：开发体验与 v1 最终验收

- CLI：补 controller/scaffold/command 等剩余生成器；按 [底层补齐计划](plans/v1-foundation-completion.md) F9 提供模板清单/导出/预览/应用覆盖、变量版本与冲突诊断；补 routes、只读 doctor、`db status` 和显式 `db reset`；复用已有 config show/migrate/seed。status/doctor 不写数据；reset 另定目标环境、确认和数据清理边界，在隔离库测试，不由 doctor/dev 隐式触发。
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

按 [v2 后台计划](plans/v2-admin-platform.md) 执行 B1–B5：Admin 使用 pure-admin 精简国际化版 + Element Plus，App 使用 uni-app，PC 仅规划设计；B1 锁定版本与 Rust API 适配。候选功能仍先评估，不因列出即全部开发。[AI 插件套件与完整平台应用](plans/ai-platform-product.md) 已确认交付方向但未立项，不进入当前 V1/V2 实现任务，也不创建源码、分支或 worktree；后续按独立门禁启动。

## 外部条件与证据缺口

- Twilio 实发缺授权账号、凭据与目标；可继续不依赖实发的工作，不能代称已投递。
- 完整 Redis/对象存储/跨节点实时、最低版本整体兼容、集群与性能门禁尚未完成。可运行基础服务或单项进程测试不能替代这些证据。
