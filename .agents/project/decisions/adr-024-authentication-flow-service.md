# ADR-024 认证流程服务与应用路由边界

- 状态：Accepted（AuthFlows service/plugin 与生成 HTTP endpoints 已实现；PostgreSQL 18 / MySQL 8.4 双库集成和生成项目真实 TCP HTTP e2e 通过）
- 日期：2026-10-26
- 依据：[ADR-004](adr-004-session-and-token-auth.md)、[ADR-012](adr-012-auth-current-user-contract.md)、[ADR-016](adr-016-neutral-application-namespaces.md)、[ADR-018](adr-018-jwt-primary-authentication.md)、[ADR-022](adr-022-jwt-runtime-contract.md)、[ADR-025](adr-025-shared-rate-limiter.md)、[ADR-026](adr-026-generic-verification-codes.md)

## 最小 Spec

- `openrust-auth` 提供可复用 `AuthFlows` service；HTTP 请求/响应 DTO、路由路径和业务页面仍由应用或生成项目组合，不新建第二套全局认证 router。
- 服务显式复用 `Db`、`Passwords`、`JwtTokens`、`RefreshTokens`；仅在所需服务均已注册时可构造，缺依赖启动/构建失败，不默认为 fake 或跳过验证。
- 注册创建未验证账户且不签发凭据；登录只允许已验证且未禁用账户。未知邮箱、错误密码、未验证及禁用账户对外统一拒绝，避免账户枚举。
- 登录成功返回短期 JWT access token 与不透明 refresh token；refresh 原子轮换并返回一对新凭据，重放撤销整个 family；登出撤销 refresh family，已签发 JWT 按其剩余 TTL 自然失效。
- access-token 签发失败不能消费 refresh token；凭据响应必须在数据库事务成功后生成/返回，错误路径不得泄露 bearer 或哈希。
- 注册验证、密码找回/重置与邮件/SMS 传输解耦；验证码机制按 ADR-026 提供，调用方只负责把返回 code 交给 Mail/SMS，不在 handler 自制计数、过期或一次消费。
- 登录、注册、验证和找回必须使用跨节点共享限流，优先复用 ADR-025 的 RateLimiter；请求策略要组合端点与来源，不能只按身份锁定账户。进程内计数不能作为默认保护；未接入共享限流时流程不得宣称可用于生产或静默放行。
- 默认数据库表名经 ADR-016 resolver 解析；PG/MySQL 均验证事务边界、并发刷新/撤销、未验证账户拒绝和生成项目端到端流程。

## 影响与边界

该服务只编排账户凭据生命周期，不提供 Policy/RBAC、不代替应用配置授权路由、不发送邮件或短信，也不决定业务资料字段。验证码和共享限流是登录流程的前置共同能力；其公共 contract 与实现须在启用相应端点前闭环。

## 当前实现补充

- `AuthFlowsPlugin` 必须在 `DatabasePlugin`、`AuthPlugin`、`JwtPlugin` 后注册；缺依赖时启动构建失败。`--auth` 生成注册、邮箱验证、登录、refresh、logout、password-reset HTTP handler 与 Mailer 调用；真实 TCP/fake-mail/`ClientIp` 集成已通过 MySQL 8.4.11 完整流程。
- 每个敏感服务操作调用共享固定窗口 `RateLimiter`，key 包含 endpoint、调用方可信 request-source 和 identity 摘要；当前策略为每个组合 key 每 15 分钟最多 5 次。生成 handler 使用 ADR-028 `ClientIp` extractor：默认取 TCP peer IP，仅当 peer 命中配置 CIDR 时解析可信 XFF 链。
- Service 统一邮箱规范化为 trim + ASCII lowercase。`register` 只在邮箱唯一约束冲突时返回 `None`，其他数据库错误必须传播；找回/邮箱验证对不适用账户返回 `None`。HTTP 层必须对相应 `None` 结果保持相同公开响应。`login` 对未知、未验证、禁用和密码错误都返回 `Unauthorized`。
- 未知/未验证/禁用账号登录仍执行一次当前 `PasswordConfig` 成本的 Argon2id hash，减少基于响应时长的邮箱枚举；HTTP 状态/错误码保持统一。代码 resend cooldown 对应用流程同样折叠成无 code 的 accepted 结果；RateLimiter/store 的真实故障仍传播。
- Refresh 前先校验旧 refresh token 所属账户并签好 access token，再执行原子 rotation，确保签名失败不会消费旧 token。reset 在一个数据库事务内改密并撤销账户 refresh families；验证码消费先于该事务，因此数据库写入失败后需请求新 code。logout 只撤销 refresh family，现存 JWT 在剩余短 TTL 内有效。
- 注册的账户插入与验证码签发是两个操作；若验证码 store 故障，账户保留为未验证状态，可通过独立的 `request_email_verification` 再次申请。消息发送由调用方负责。
- PG 与 MySQL 注册均使用普通 INSERT 并按 SeaORM `SqlErr::UniqueConstraintViolation` 识别重复邮箱；不能用 MySQL `INSERT IGNORE` 隐藏 CHECK、截断或其他数据库错误。
- PostgreSQL/MySQL 真实隔离库测试覆盖注册、重复注册、未验证拒绝、邮箱 code、登录、refresh rotation/replay、密码重置/旧密码拒绝和登出撤销。生成项目 fmt/check/test 基线已通过；HTTP/fake-mail/`ClientIp` 测试覆盖统一响应、JWT 与默认数据库 PAT 认证、限流错误、验证码、refresh 重放、密码重置及登出撤销。MySQL 8.4.11 全流程本轮通过；PostgreSQL 先前基础流程通过，但本轮新增代理与限流断言的复验受本地 PostgreSQL 测试容器只读数据目录阻塞。可信代理解析已单测及 MySQL HTTP 验收，PostgreSQL 的生成 HTTP 全流程仍待可写测试库复验。
