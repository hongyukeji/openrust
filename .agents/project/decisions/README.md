# 架构决策索引

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：快速定位有效 contract 与替代关系。ADR 接受表示设计成立，不等于能力已实现或验证。

## 使用规则

- 当前代码、Git 与 manifest 是实现事实；`todo.md` 是唯一活跃队列，`core/` 与 `memory/` 记录实现/验证边界。
- `Accepted`：仍有效；`Partially Superseded`：只保留表中列明的部分；`Superseded`：仅供追溯，不执行其中旧方案。
- 改默认值或公共契约时同时更新原 ADR 的状态/去向与本索引；不把历史讨论再次复制成活跃任务。
- 数据库测试隔离原误用编号 011，现校准为 038；短信保留 011。编号不表示实施先后。

## 基础、布局与工程规则

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [001 自有内核与组件整合](adr-001-own-kernel-on-axum.md) | Accepted | 成熟组件优先；自有组装与公开扩展，Laravel 式开发体验，Rust 惯用 API |
| [002 SeaORM 与双库](adr-002-seaorm-dual-database.md) | Accepted | PostgreSQL/MySQL 正式矩阵；暂定最低与日常基线分开验证 |
| [005 许可证与 MSRV](adr-005-license-naming-msrv.md) | Partially Superseded | MIT OR Apache-2.0、Edition、MSRV 有效；包命名与分发由 039 替代 |
| [006 命名与命令](adr-006-rust-native-naming.md) | Partially Superseded | 通用命名有效；命名空间见 016，目录见 021，具体能力 contract 见各 ADR |
| [007 聚合宿主](adr-007-aggregate-server-entry.md) | Partially Superseded | 集成职责仍有效；永久保留及旧 crate 拓扑由 021 替代 |
| [008 HTTP API](adr-008-api-conventions.md) | Accepted | 响应、状态模式、错误与协议例外 |
| [009 可扩展驱动](adr-009-extensible-capability-drivers.md) | Accepted | 统一入口、公开扩展、后端能力与真实验收 |
| [010 Seed](adr-010-database-seeding.md) | Accepted | 应用提供 seed 函数，不做平行 ORM 工厂 |
| [016 中性命名空间](adr-016-neutral-application-namespaces.md) | Accepted | 默认 APP_、tb_ 与可定制解析，替代 006 的旧品牌前缀 |
| [019 中性 crate 目录](adr-019-neutral-crate-directories-and-app-package.md) | Superseded | 原逐能力 crates 取消；目录见 021，当前 server 中性包名见 039 |
| [020 旧 workspace 方案](adr-020-workspace-layout-and-loco-practices.md) | Superseded | 旧多能力 package 方案取消；有效配置/示例原则已纳入 021/030 |
| [021 模块与目录职责](adr-021-loco-style-module-layout.md) | Partially Superseded | 根模块、工具边界和宿主职责有效；发布身份与独立生成项目由 039 替代 |
| [023 应用扩展与体验](adr-023-loco-application-experience-evaluation.md) | Partially Superseded | doctor/TestApp 有效；停机未决项由 037 命令 supervisor 定稿 |
| [029 读写路由](adr-029-database-read-routing.md) | Accepted | 显式副本、主库 scope；不实现复制/选主 |
| [030 模块化配置](adr-030-modular-configuration.md) | Accepted | Rust 配置代码与运行时 config 分离；单一 ConfigLoader/.env 规则 |
| [038 测试数据库隔离](adr-038-isolated-test-databases.md) | Accepted | 独立测试库与显式清理；原重复编号 011 校准 |
| [039 源码交付与内部命名](adr-039-source-workspace-distribution.md) | Accepted | 整套 workspace、apps 业务、内部中性包名与禁用发布；当前交付证据见 progress.md |

## 认证、安全与通信

| ADR | 状态 | 当前归属与边界 |
| --- | --- | --- |
| [004 旧认证默认](adr-004-session-and-token-auth.md) | Partially Superseded | 账户/密码/通用流程原则保留；Session/PAT 默认及排除 JWT 由 018 替代 |
| [011 邮件与短信范围](adr-011-sms-and-communication-baseline.md) | Accepted | v1 通信需求；具体接口见 033，供应商实发另需授权 |
| [012 身份与路由](adr-012-auth-current-user-contract.md) | Accepted | 认证身份/路由边界，与 018/022 配合 |
| [017 通用 Session](adr-017-generic-session-runtime.md) | Accepted | 显式可选 Cookie Session，数据库默认共享 |
| [018 JWT-first](adr-018-jwt-primary-authentication.md) | Accepted | API 主要认证路径、refresh、可选 Session/PAT |
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
