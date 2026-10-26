# 当前进度

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：框架处于 v1 M5，尚未达到完整 v1。源码工作区、C1–C3 中文化、M3 认证流程扩展及 M4 共享锁、worker 生命周期、F1 日历调度、F2 国际化、F3 邮件模板、队列化通知、job 生成集成、通用插件公开契约、Db 与 Queue 外部扩展契约、跨协议验证/调用作用域和 HTTP 有界运行/就绪切换均有适用验收证据；M5 缓存驱动、命名实例、F4a tag generation、F4b 单进程加载合并和 F4c SWR 已通过适用验收，Redis Session 和 Redis Queue 均通过 Redis 8 真实服务验收；当前进入数据访问与查询授权 F5，完整剩余范围见待办。

## 源码 workspace 交付

[ADR-039](decisions/adr-039-source-workspace-distribution.md) 已实现并完成验收：内部包 `cli`、`macros`、`testing`、`server` 与所有本地生成 package 均禁用发布；workspace/path 依赖、宏门面、CI、应用生成与配置定位已迁移。干净源码副本生成默认、PostgreSQL、MySQL 及加密认证应用并 check/test；两个应用独立启动、配置隔离通过。PostgreSQL 18 与 MySQL 8.4 真实服务矩阵、workspace fmt/clippy/test/doc、cargo deny 均通过。细节见 `.agents/project/memory/testing.md` 与 ADR-039。

## 已有交付与边界

| 能力 | 当前实现与证据 | 尚未覆盖 |
| --- | --- | --- |
| 根模块与配置 | 框架合入根 `src/`；typed config、APP_ 可定制命名空间、`.env` 生产隔离与脱敏；默认/关键 features 和生成项目已有验证 | 无本簇剩余项 |
| 内核与 HTTP | Application/Plugin、显式服务、静态插件顺序、`try_insert`/`try_command` 冲突错误、`try_get` 缺依赖错误、配置 section 安全字段路径、应用路由冲突启动错误；显式服务、路由/resource、验证、统一响应、分页、request ID、CORS、压缩、超时、panic 处理；`RateLimitLayer` 显式限流支持可信 IP、已验证用户、匿名回退和应用 key；`SortParams` 按 ADR-008 语法验证并执行字段 allowlist；HTTP 文档提供严格 DTO 与显式过滤示例 | OpenAPI、完整 readiness/metrics/OTLP |
| 数据层 | PostgreSQL/MySQL、迁移/seed/事务、tb_ resolver、隔离测试、default 与具名数据库实例（ADR-045）、显式读写分离、数据库共享锁与请求幂等；PG18/MySQL8.4 真实复制暂停/追平已验证；model/migration 支持可选 UUIDv7 与显式 `not_deleted()` scope，生成项目 PG18/MySQL8.4 真实迁移/插入/删除/读取/恢复通过；ADR-041 提交前回滚与提交后 direct 通知已双库验证；自定义 Db plugin 可复用公开 `Db` 连接/事务与应用自有 SeaORM migration，独立 SQLite consumer 已实测 | 完整业务性能与集群；SQLite 不属于官方支持矩阵，框架 feature 专属能力仍需适配 |
| 认证与授权 | JWT/refresh、可选 Session/CSRF、PAT、VerificationCodes、AuthFlows、Policy/RBAC、共享限流；双库 auth_database/auth_runtime 与生成项目 TCP HTTP 流程已有通过记录；注册/登录 typed events 与账户/refresh 事务原子性按 ADR-041 双库通过 | 完整跨节点业务场景；access token 不逐请求查撤销 |
| HTTP client 与 i18n | 统一有界出站调用/fake；请求级 locale、中英资源、JSON/typed plural/select、精确小数及显式时区日期格式；workspace tests/docs/clippy/deny、PostgreSQL 生成源码 clean-copy check/test 通过 | 不覆盖 Fluent/货币格式/运行时 ICU 数据更新；日期时区由调用方决定；gRPC 在 G2 组合验收 |
| 邮件与短信 | Mailer/SmsSender、SMTP/log/fake、Twilio adapter、本地化模板与队列化多渠道 Notifier；真实 Mailpit SMTP 队列投递和 API 收件确认 | Twilio 真实账号投递仍待授权目标与凭据 |
| 加密、签名、审计 | 进程内加密/key ring、用途隔离签名链接/可选共享撤销、可替换审计 sink/同事务审计；已有单测、双库相关路径与生成验证 | 加密实现审计/平台性能复核；不承诺任意远程 sink 可靠送达或审计不可篡改 |
| 队列与 worker | 数据库任务、显式 handler、事务派发、有限并发/重试/失败保留/超时/lease/心跳；双库强杀恢复；活动唯一键、任务链/有限批次已提交；ADR-042 的跨实例锁与业务请求幂等已单独交付。Worker 在 PG18/MySQL8.4 真实 SIGTERM 下合作取消并确认、不合作超时并释放 lease、同队列 pending job 不再领取；连接池断开时 supervisor 返回错误且 lease 保留；新 worker 接管后旧 token 不能确认新结果；所有 worker join 后传播错误。F1 增加五字段 cron/IANA 时区、DST wall-slot 去重、计划版本隔离和每 tick 1–100 项补跑；PostgreSQL 18/MySQL 8.4 的独立进程竞争、计划变更和积压推进测试通过。M5 `JobRegistry::execute` 明确为无持久化/事务/自动重试/任务组的 await 内执行入口。ADR-046 新增可选 Redis driver，数据库仍为默认；真实 Redis 8.10.2 测试覆盖多连接 40 项竞争、唯一派发、失败重试、进度及子进程强杀后的租约接管；Redis-only `apps/server` 插件集成及真实 API/worker 分进程 smoke 通过。其事务/任务组/scheduler 边界和 Redis 集群/最低版本限制见 ADR-046。当前下一簇数据访问与查询授权 F5 | 生产多实例负载与完整负载集群仍待完成 |
| M4 事件扩展 | `openrust::events` typed ordered listener、fail-fast 与事务上下文；queue feature 显式映射至已注册 Job；事务 before/after 边界及 AuthFlows 注册/登录事件按 ADR-041 验收。真实 PostgreSQL 18.6 与 MySQL 8.4 隔离库各通过事务拒绝/回滚/提交后 listener 错误、注册拒绝无账户、登录失败通知及登录成功事件失败无 refresh 泄漏；既有 queued listener 双库提交/回滚 worker 验收仍有效 | 不代表跨节点事件广播、exactly-once 或完整集群 |
| 目录与测试入口收尾 | CLI/macros/test packages 已迁至 `cli/`、`macros/`、`testing/`；hello 位于 `examples/hello/main.rs`；根 HTTP 和 server integration targets 按能力分组并显式保留 target 名。迁移前后均为 5 个 package、265 个可列测试条目。PG18/MySQL8.4 server all-features 集成各 40 项通过；生成 PostgreSQL/MySQL `--auth` 项目各通过 fmt/check/真实 auth_flows HTTP test；hello TCP HTTP 200；workspace fmt/clippy/test/docs、deny 与 no-default check 通过。`apps/server` 保留；旧 `crates/` 源码和目录已移除。 | 无本簇剩余项 |
| Scheduler | UTC interval/daily slot、数据库时钟、slot 与 job 原子去重、run/once/list；双库独立进程竞争/异常退出/SIGTERM 已验证 | 不含 cron/IANA/DST；生产资源、滚动升级和完整拓扑验收未完成 |
| CLI 与测试工具 | new、model/migration/mailer/policy/job 生成、dev/运行命令代理、TestDatabase；job 生成同时启用 app/migration queue feature、登记 QueueMigration 与 JobsPlugin；生成项目 PG18/MySQL8.4 check/test、双库队列 targets、真实 PostgreSQL API/worker/scheduler 组合通过 | controller/scaffold/command 生成器、依赖编排/watch、doctor、TestApp/HTTP assertions、黄金路径/正式文档 |
| 缓存、存储、实时 | 统一 Cache 与 memory/file/Redis/custom drivers、named stores、tag generation 和有界单进程同键 loader 合并已实现；真实 Redis tag 与应用聚合 singleflight、workspace 门禁通过 | 完整分布式缓存生产负载仍待验证；local/S3 存储、上传扩展、WebSocket/SSE 及可观测性仍待完成 |

具体 API 只在 [core/](core/) 维护，验收入口见 [testing.md](memory/testing.md)，声明边界见 [verification.md](memory/verification.md)。Git 中的未提交改动只算 WIP，不因文档先写“实现”而升为交付。

## 阶段判定

- M5 HTTP 有界运行与就绪切换：新增应用级 `/ready`（listener 绑定后 200，停机/未启动时 503），保留 `/health` 存活语义；SIGINT/SIGTERM 先切换 not-ready 并停止接入，按 `shutdown_timeout_secs` drain，超时合作取消请求、join/终止 server task 并返回 `http_shutdown_timeout`。覆盖 listener bind 失败、请求 drain/超时和真实 server 子进程 SIGTERM。定向生命周期测试与真实 TCP SIGTERM 通过；`cargo fmt --all --check`、`git diff --check`、all-target/all-features clippy、默认与 all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check`、no-default-features check 全部通过。`cargo deny` 有既有重复版本/MPL allowance 警告，advisories/bans/licenses/sources 均通过。测试仅验证单进程 HTTP 生命周期，不代表集群 readiness/依赖探测。

- M5 Queue 外部扩展契约已提交 `d7d2132`：公开 `JobRegistry` 注册/执行 typed Job，`JobContext` 提供取消、进度 reporter 与应用状态；独立 consumer 经 `Queue::with_driver` 实际派发执行。PG18/MySQL8.4 队列及其相关 16 项双库真实服务回归通过；尚未验收完整集群与生产负载。
- M5 跨协议调用作用域已提交 `19e56f4`：新增公共 `validation::validate`、`ExecutionContext` / `ExecutionMetadata` / `Cancellation`；HTTP request/timeout/cancel、命令 locale scope、数据库队列 metadata envelope、worker attempt deadline/cancel 与 request-id span 均已实现。PostgreSQL 18 与 MySQL 8.4 的真实 queue/migration/crash-recovery targets 各 7 项通过，包含持久化 worker 与任务链 metadata round-trip。后续 HTTP 有界运行/就绪切换另见本节；缓存、SWR、命名数据库实例、Redis Session、显式同步任务和 Redis Queue 均已完成适用验收；当前转入数据访问与查询授权 F5。

- 原 M0–M2 基础范围和 M3 本地认证/安全运行时已验收；Mailer/Policy 生成器、公开 HTTP 路由限流与数据输出/查询边界均已提交并完成适用验证。
- M4 仅部分完成；单项 scheduler/worker 多进程测试不等于完整集群验收。
- M4 类型化事件、queued listener、事务提交边界与 AuthFlows 注册/登录扩展已在 PostgreSQL 18/MySQL 8.4 完成双库验收；源码 workspace 交付和 C1–C3 中文化现已验收，共享锁与请求幂等已于 ADR-042 完成。Worker 生命周期修正与 F1 cron/IANA/DST、版本隔离、每 tick 有界补跑经双库专项验收。F2 通过中/英/波兰语、select/plural fallback、精确小数、跨时区日期、HTTP 并发 locale 测试及 clean-copy 生成项目 check/test。F3 已提交可选 MiniJinja 邮件模板、严格变量/HTML 转义/布局与 MIME、CLI 中英模板生成器；Mailpit 实际 SMTP 收件及 API 的主题、文本、HTML 已确认。队列化通知与薄多渠道编排已完成双库与 SMTP 验收；M4 job 生成器、组合示例、双库 CI targets 和 workspace 门禁已完成，当前进入 M5 通用插件公开契约。
- M4 队列化通知：`openrust::notifications::Notifier` 分别派发固定邮件/SMS job，通过 `JobContext::service<T>` 取 worker 应用服务；事务提交/回滚、分渠道活动唯一键、独立执行、失败重试及“供应商接受但确认丢失会重复发送”均有真实数据库测试。PostgreSQL 18 与 MySQL 8.4 的 `queue_notifications` 各 1 项通过；Mailpit v1.32.0 通过 SMTP 接受队列 worker 邮件，API 确认收件人、主题、文本与 HTML。Twilio 协议已有测试，真实账户发送仍无授权目标/凭据。
- C2 Rustdoc 与源码/配置注释中文化：覆盖根公开模块、`cli` 内部实现、`macros`、`testing`、`apps/server`、`examples` 与 `tests`；未改 API、运行时文案或行为。fmt、workspace clippy/test/doc、cargo deny、diff check 通过；PostgreSQL 18 和 MySQL 8.4 各通过 18 项 auth/db/queue/recovery/schedule 集成测试。生成模板入口注释快照按实际生成结果更新。
- C3 生成说明与 CLI 帮助中文化：CLI 分组标题、帮助/版本选项、子命令说明、生成的应用指南、`.env.example` 注释及生成 Rustdoc 已中文化；Cargo 描述原已中文。CLI 15 项测试、workspace fmt/clippy/test/warnings-denied doc/cargo deny 通过；干净源码副本生成无数据库、PostgreSQL、MySQL 应用各自直接 check/test 通过。
- C1 默认文档入口中文化：根 README、贡献/安全/行为准则/变更日志及框架模块 README 已按简体中文优先更新，保留代码、技术标识符、链接和许可证原文。13 个变更 Markdown 的本地链接/锚点、`git diff --check`、warnings-denied Rustdoc 与框架 doctest 通过；详见 `.agents/project/memory/verification.md`。
- V1 底层补齐 F1–F10 的范围与依赖见 [专题计划](plans/v1-foundation-completion.md)；F1–F3 已落地，F4–F10 仍按待办顺序交付；若依/芋道仅 V2 参考。
- gRPC 已按 [ADR-040](decisions/adr-040-grpc-integration.md) 纳入 v1/M5，当前仅完成范围和规划；跨协议验证/上下文、出站安全/韧性及生产运行/迁移补强见 `todo.md`，没有新增代码或运行验收证据。
- M5/M6 尚有成组能力未交付；没有可宣称已完成的 v1 性能基线、全部最低版本兼容性或完整集群验收。
- v1 完整验收通过后按已授权顺序进入 v2；Admin 已确定 pure-admin 精简国际化版 + Element Plus，App 使用 uni-app，PC 仅规划设计。当前只完成选型与文档校准，未创建端侧应用或安装前端依赖；[AI模型平台](plans/ai-model-platform.md) 仅有规划，尚未开发，须等待用户明确要求开发并满足成熟门禁。

## 下一步

按 [todo.md](todo.md) 推进显式同步任务执行及其后续 M5/M6 工作；F4c SWR 已通过专项验收，再执行完整 v1 性能、生产就绪与集群门禁。Worker 与 scheduler 专项双库证据不覆盖生产多实例负载与完整集群门禁。组件整合与 Laravel 式体验按 ADR-001；不得重做已提交能力，也不得因旧阶段标签跳过补交和最终验收。

## 本次文档核对范围

核对当前源码、Cargo/Git、Agent 文档、ADR 与本机可定位项目记录：Codex/Claude 原生 JSONL 快照 74 份（45 份关联 OpenRust，207 条去重人类候选文本）；Paseo 26 份相关日志（24 份项目/已确认 worktree，2 份明确关联的起源会话），与原生会话按来源交叉去重，不相加计算独立对话。OpenCode 找到 7 个项目会话元数据但无消息正文；其他客户端未发现额外可限定到本项目的本地正文。

覆盖可读取的本地项目资料，不声称覆盖全部云端聊天。原始聊天和凭据不入库；只将有效要求写入相应决策与计划，历史交付说法仍以当前代码及验收证据核实。
