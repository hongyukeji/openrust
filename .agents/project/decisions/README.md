# 架构决策索引

当前目标设计由 [070 Rust 版 Laravel 的产品定位、全面重构与持续跟进](adr-070-laravel-first-rebuild.md)（Accepted，尚未实现）接管；表中旧 contract 的实现事实保留，新目标按 070 的替代范围读取。

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：快速定位有效 contract 与替代关系。ADR 接受表示设计成立，不等于能力已实现或验证。

## 使用规则

- 当前代码、Git 与 manifest 是实现事实；`todo.md` 是唯一活跃队列，`core/` 与 `memory/` 记录实现/验证边界。
- `Accepted`：仍有效；`Partially Superseded`：只保留表中列明的部分；`Superseded`：仅供追溯，不执行其中旧方案。
- 改默认值或公共契约时同时更新原 ADR 的状态/去向与本索引；不把历史讨论再次复制成活跃任务。
- 数据库测试隔离原误用编号 011，现校准为 038；短信保留 011。编号不表示实施先后。

## 当前请求路由契约

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [083 真实路由上下文与受管理方法声明](adr-083-matched-route-context.md) | Accepted（限定子范围已验收） | D7公开Route/参数/resolver、QUERY/受管理方法及fallback/OPTIONS纠偏；原生Axum逃生口边界明确；不关闭完整Routing/模型绑定 |
| [084 Session previous 与请求级 Precognition](adr-084-session-previous-and-precognition.md) | 实现中，尚未验收 | D8真实Route后的previous保存、可信确认状态、受管理参数提取/分派与统一规则选择；完整FormRequest/URI对象/官方前端包仍开放 |

## 运行时配置与异步目标

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [080 Laravel配置扩展与数据库动态调度](adr-080-runtime-configuration-scheduling.md) | Accepted（未实现、未验收） | R2必交typed分组DB设置/受权API（宿主启用可选）、R10默认DB队列/动态计划、R14集群与迁移；Laravel简洁读取，V2界面后置 |

## 基础、布局与工程规则

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [070 Rust 版 Laravel 与持续跟进](adr-070-laravel-first-rebuild.md) | Accepted（计划已记录，未实现） | 用户原意、原汁原味、已有优秀机制融合、长期上游同步与必要 Rust 适配；实施见 R0–R16 |
| [001 自有内核与组件整合](adr-001-own-kernel-on-axum.md) | Partially Superseded | 目标约束按 ADR-070 重开；成熟组件优先；自有组装与公开扩展，Laravel 式开发体验，Rust 惯用 API |
| [002 SeaORM 与数据库基线](adr-002-seaorm-dual-database.md) | Partially Superseded | PostgreSQL/MySQL 生产与集群基线；SQLite 轻量后端范围见 066 |
| [005 许可证与 MSRV](adr-005-license-naming-msrv.md) | Partially Superseded | MIT OR Apache-2.0、Edition、MSRV 有效；包命名与分发由 039 替代 |
| [006 命名与命令](adr-006-rust-native-naming.md) | Partially Superseded | 目标约束按 ADR-070 重开；通用命名有效；命名空间见 016，目录见 021，具体能力 contract 见各 ADR |
| [007 聚合宿主](adr-007-aggregate-server-entry.md) | Partially Superseded | 目标约束按 ADR-070 重开；集成验收职责有效；宿主迁入根 package 由 062 更新 |
| [008 HTTP API](adr-008-api-conventions.md) | Partially Superseded | 目标约束按 ADR-070 重开；响应、状态模式、错误与协议例外 |
| [076 Laravel 语义 HTTP 状态默认值](adr-076-laravel-http-status-default.md) | Accepted | JSON API 默认保留真实状态码；`always_ok` 只作为显式覆盖，统一 envelope 与 request ID 一致 |
| [077 Laravel 式域路由](adr-077-domain-routing.md) | Accepted | `RouteGroup::domain`、单标签参数、domain-first、Host 安全与具名 URL；R3-F 已提交163bb49 |
| [078 请求检查与内容协商](adr-078-request-inspection.md) | Accepted | RequestInfo 不消费body，复用authority与类型化提取器；固定Laravel/Symfony协商行为和有界headers |
| [079 统一请求输入](adr-079-request-input.md) | Accepted | body-owning Request复用输入bags；Json/Query共享规范化和有界解析，原文保留 |
| [081 请求Cookie与响应队列](adr-081-request-response-cookies.md) | Accepted（C04-C 子簇已验收） | 请求隔离queue、wire与bag分层、复用现Session/Encryptor；名称绑定与响应加密顺序 |
| [082 统一Session状态与flash生命周期](adr-082-session-store-and-flash.md) | Accepted（C04-D1限定子范围已验收） | 唯一Session/公开store、40位ID与属性/flash、请求保存生命周期；保留防复活安全增强，完整Web栈仍开放 |
| [009 可扩展驱动](adr-009-extensible-capability-drivers.md) | Accepted | 统一入口、公开扩展、后端能力与真实验收 |
| [010 Seed](adr-010-database-seeding.md) | Partially Superseded | 目标约束按 ADR-070 重开；应用提供 seed 函数，不做平行 ORM 工厂 |
| [016 中性命名空间](adr-016-neutral-application-namespaces.md) | Partially Superseded | 目标约束按 ADR-070 重开；默认 APP_、tb_ 与可定制解析，替代 006 的旧品牌前缀 |
| [019 中性 crate 目录](adr-019-neutral-crate-directories-and-app-package.md) | Superseded | 原逐能力 crates 取消；目录见 021，当前 server 中性包名见 039 |
| [020 旧 workspace 方案](adr-020-workspace-layout-and-loco-practices.md) | Superseded | 旧多能力 package 方案取消；有效配置/示例原则已纳入 021/030 |
| [021 模块与目录职责](adr-021-loco-style-module-layout.md) | Partially Superseded | 目标约束按 ADR-070 重开；根模块、工具边界和宿主职责有效；发布身份与独立生成项目由 039 替代 |
| [062 根目录统一启动入口](adr-062-root-server-entry.md) | Partially Superseded | 目标约束按 ADR-070 重开；根唯一 `server` bin 与 Cargo 入口有效；应用模块注册由 063 补充 |
| [063 显式业务模块注册](adr-063-explicit-app-module-registry.md) | Partially Superseded | 目标约束按 ADR-070 重开；根 `src/main.rs` 单一启动；根宿主在 `apps/mod.rs` 静态声明并注册业务模块，不扫描或动态加载 |
| [064 统一示例应用与验收 consumer 收纳](adr-064-canonical-example-application.md) | Partially Superseded | 目标约束按 ADR-070 重开；用户示例统一到 `apps/example/`；独立 consumer/拓扑验收收纳到 `tests/consumers/`；保留编译期显式注册 |
| [065 框架路由入口与数据模块分层](adr-065-root-routing-and-data-module-layout.md) | Partially Superseded | 目标约束按 ADR-070 重开；路由装配实现位于 `src/routes.rs` 并沿用 RouteRegistry/Tower 扩展；数据交换源码位于 `src/data/exchange/`，旧 API 路径兼容 |
| [066 SQLite 轻量数据库后端完整支持](adr-066-optional-sqlite-backend.md) | Accepted（计划中，未实现） | v1 完整支持适用于单机语义的通用能力；“可选”指应用可选择该后端；SQLite 不承担集群共享语义；验收见专项计划 |
| [068 API 命名空间与静态站点根路由](adr-068-api-namespace-and-static-site-root.md) | Partially Superseded | 目标约束按 ADR-070 重开；业务 API `/api/*`、WebSocket `/ws/*`；配置化公开目录与根路由，默认 `public/index.html` |
| [069 服务端视图模板](adr-069-server-rendered-views.md) | Partially Superseded | 目标约束按 ADR-070 重开；可选 MiniJinja `view` feature、`views/` 模板与 `view:` 首页；`public/` 仍提供静态资源 |
| [023 应用扩展与体验](adr-023-loco-application-experience-evaluation.md) | Partially Superseded | 目标约束按 ADR-070 重开；doctor/TestApp 有效；停机未决项由 037 命令 supervisor 定稿 |
| [029 读写路由](adr-029-database-read-routing.md) | Accepted | 显式副本、主库 scope；不实现复制/选主 |
| [030 模块化配置](adr-030-modular-configuration.md) | Partially Superseded | 目标约束按 ADR-070 重开；Rust 配置代码与运行时 config 分离；单一 ConfigLoader/.env 规则 |
| [038 测试数据库隔离](adr-038-isolated-test-databases.md) | Accepted | 独立测试库与显式清理；原重复编号 011 校准 |
| [039 源码交付与内部命名](adr-039-source-workspace-distribution.md) | Partially Superseded | 目标约束按 ADR-070 重开；整套 workspace、apps 业务、内部中性包名与禁用发布；宿主位置由 ADR-062 替代 |

## 认证、安全与通信

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [004 旧认证默认](adr-004-session-and-token-auth.md) | Partially Superseded | 账户/密码/通用流程原则保留；Session/PAT 默认及排除 JWT 由 018 替代 |
| [011 邮件与短信范围](adr-011-sms-and-communication-baseline.md) | Accepted | v1 通信需求；具体接口见 033，供应商实发另需授权 |
| [012 身份与路由](adr-012-auth-current-user-contract.md) | Accepted | 认证身份/路由边界，与 018/022 配合 |
| [017 通用 Session](adr-017-generic-session-runtime.md) | Accepted | 显式可选 Cookie Session，数据库默认共享 |
| [018 JWT-first](adr-018-jwt-primary-authentication.md) | Partially Superseded | 目标约束按 ADR-070 重开；API 主要认证路径、refresh、可选 Session/PAT |
| [022 JWT runtime](adr-022-jwt-runtime-contract.md) | Accepted | claims、key ring 与失效语义 |
| [024 AuthFlows](adr-024-authentication-flow-service.md) | Accepted | 流程 service 与应用 HTTP 路由分工 |
| [025 共享限流](adr-025-shared-rate-limiter.md) | Accepted | 固定窗口与双库共享 store；HTTP 接入事实见 core/http.md |
| [026 验证码](adr-026-generic-verification-codes.md) | Accepted | 用途/身份隔离、原子消费、尝试和重发约束 |
| [027 Policy/RBAC](adr-027-policy-and-basic-rbac.md) | Accepted | 最小角色权限 service，组织/DataScope 归 v2 |
| [028 可信代理](adr-028-trusted-proxy-client-ip.md) | Accepted | 显式可信来源与 ClientIp；历史验证不等于全部集群验收 |
| [031 出站 HTTP](adr-031-outbound-http-client.md) | Accepted | 有界 body、超时/取消、显式受控重试与 transport fake |
| [032 国际化](adr-032-request-internationalization.md) | Accepted | 请求 locale、资源和显式后台语言 |
| [033 通信 driver](adr-033-sms-driver-and-template-contract.md) | Accepted | Mail/SMS 模板、SMTP/Twilio；供应商受理不等于送达 |
| [034 本地加密](adr-034-application-encryption-and-key-rotation.md) | Accepted | 本地认证加密与轮换；实现审计/性能仍有发布复核项 |
| [035 审计](adr-035-audit-recording.md) | Accepted | 脱敏、可替换 sink 与事务边界 |
| [036 签名链接](adr-036-signed-links.md) | Accepted | 用途隔离、到期与可选共享撤销，独立于验证码 |
| [040 gRPC 集成](adr-040-grpc-integration.md) | Accepted | v1 可选 server/client 与四类调用、协议/安全/运行/集群契约；尚未实现，替代旧 v2 候选范围 |
| [041 事务与认证流程事件](adr-041-transaction-and-auth-flow-events.md) | Accepted | 标准 Db 提交边界与 AuthFlows typed events；direct after-commit 尽力通知、可靠副作用使用事务队列 |
| [042 共享锁与请求幂等](adr-042-shared-locks-and-request-idempotency.md) | Accepted | PostgreSQL/MySQL 数据库租约/fencing 与显式请求结果快照；不等同外部副作用 exactly-once |
| [044 协议无关验证与调用作用域](adr-044-cross-protocol-execution-context.md) | Accepted | 复用 validator/Error；单次调用 metadata/deadline/cancel/locale 与队列安全传递边界 |
| [047 类型化资源查询授权](adr-047-typed-query-authorization.md) | Accepted | 资源行范围与 RBAC 分离；规则条件和业务筛选固定 AND，受保护读写隐藏 SeaORM builder |
| [048 稳定键有界分批读取](adr-048-stable-batch-reading.md) | Accepted | 复用 SeaORM keyset cursor，限定稳定主键、批次上限、取消与并发/事务边界 |
| [049 数据库迁移执行互斥](adr-049-migration-execution-lock.md) | Accepted（实现中） | 迁移命令以数据库行锁序列化，超时和进程退出可恢复 |
| [050 统一文件存储与驱动扩展](adr-050-storage-drivers.md) | Accepted | 根 `openrust::storage` 复用 OpenDAL，提供 local/S3-compatible、具名实例、流式对象契约及公开 driver/inspection 扩展；MinIO 实测和 workspace 门禁见 memory/testing.md |
| [051 出站 HTTP 受限目标策略](adr-051-http-target-security.md) | Accepted | opt-in 受限 HttpClient、DNS 全结果校验并固定连接地址、显式内网主机/CIDR/端口例外；不覆盖任意 transport 或第三方网络调用 |
| [052 命名出站依赖韧性](adr-052-named-dependency-resilience.md) | Accepted | 显式按名称共享有界并发/队列及可选进程内熔断；幂等重试加入 jitter 与受界 Retry-After |
| [053 外部 OAuth2/OIDC 身份客户端](adr-053-external-identity-client.md) | Accepted | 受限授权码/PKCE relying-party、发现/JWKS/ID Token 验证与显式身份绑定；真实 Keycloak 及双库验收见 `memory/testing.md` |
| [054 TOTP 多因素认证](adr-054-totp-mfa.md) | Accepted | 可选 TOTP/恢复码、一次性登录挑战、密钥轮换及不可绕过的认证门禁；PostgreSQL/MySQL 双库与生成项目验收通过，见 `memory/testing.md` |
| [059 WebSocket 与 SSE 在线实时传输](adr-059-realtime-transport.md) | Accepted | 有界在线连接、本地/公开 transport、Redis Pub/Sub 跨节点路由、认证授权、到期撤销与有界停机；无离线回放或 exactly-once |
| [060 密码哈希的有界并发](adr-060-password-hash-capacity.md) | Accepted | 每个服务实例限制 Argon2 并发，过载立即拒绝；clone 共享许可，阻塞操作完成前不释放 |
| [061 运行时依赖就绪探针](adr-061-runtime-dependency-readiness.md) | Accepted | HTTP `/ready` 聚合有界只读数据库/缓存探针；不改变 liveness 与 gRPC listener 语义 |

## 队列、缓存与部署

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [003 数据库队列默认](adr-003-database-queue-default.md) | Partially Superseded | 数据库队列方向有效；缓存默认按 015，worker/scheduler 运行语义按 037 |
| [013 单机/集群](adr-013-deployment-and-redis-cache-default.md) | Accepted | 两种拓扑、共享状态与基础服务责任边界 |
| [014 缓存 driver/store](adr-014-cache-driver-and-store-contract.md) | Partially Superseded | 公共缓存契约有效；旧 memory 默认由 015 替代，实施待 M5 |
| [015 Redis 默认](adr-015-redis-default-calibration.md) | Accepted | 标准应用 Redis，memory/file 显式；库 feature 可组合 |
| [037 队列/调度生命周期](adr-037-queue-worker-lifecycle.md) | Accepted | 命令持有 supervisor、lease/token、任务编排/事件契约；逐项验收，非 M4 完成声明 |
| [046 Redis Queue driver](adr-046-redis-queue-driver.md) | Accepted（实现及 Redis 8 真实服务验收完成） | 可选 Redis 至少一次队列、lease/token、事务/任务组限制；默认数据库队列保持不变；最低版本及集群门禁待 v1 总验收 |

版本分工、分版本框架参考和独立 AI 产品边界见 [当前计划](../plan.md)、[版本路线](../roadmap.md) 与 [框架设计](../plans/framework-design.md)，不在 ADR 索引复制另一份任务队列。

- [ADR-071：应用职责目录与独立框架库](adr-071-application-framework-layout.md)：Superseded；原根应用布局由 ADR-072 替代；保留旧验收事实。

- [ADR-072：同仓框架与应用 workspace](adr-072-framework-and-application-projects.md)：Superseded；旧 demo 成员布局与已通过门禁作为历史输入，由 ADR-073 替代。

- [ADR-073：根应用与唯一综合示例模块](adr-073-root-application-and-example-module.md)：Partially Superseded；根结构继续有效，app/example 目标位置由 ADR-074 替代。

- [ADR-074：Laravel 与 laravel-modules 的统一框架及模块目录](adr-074-laravel-modules-directory.md)：Accepted；业务模块统一 modules/、唯一 modules/example，同一 Provider/Container；迁移与能力待验收。
