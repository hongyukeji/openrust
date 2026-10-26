# ADR-025 跨节点共享限流契约

- 状态：Accepted（通用 API 与 PostgreSQL 18 / MySQL 8.4 store 已实现并通过 AuthFlows 与生成项目 HTTP 总验收）
- 日期：2026-10-26
- 依据：[v1 能力覆盖](../plans/v1-capability-coverage.md)、[ADR-013](adr-013-deployment-and-redis-cache-default.md)、[ADR-024](adr-024-authentication-flow-service.md)

## 决策

- 通用限流入口为 `openrust-core::RateLimiter` 与公开 `RateLimitStore` 契约；认证、HTTP middleware、短信/邮件和业务服务复用同一服务，不各建专用计数器。
- `RateLimitStore` 每次原子消费返回窗口计数与重置等待时间；`RateLimiter` 校验 key/limit/window 并统一返回稳定 429 与 `Retry-After`。key 原文不得写日志或持久化。
- v1 官方共享实现复用既有 PostgreSQL/MySQL，使用数据库服务端时钟和每个 key 一行的固定窗口；多个 API/worker 副本对同一 key 竞争同一数据库原子记录，不依赖粘性会话或节点内存。
- 数据表由可组合的 `DatabaseRateLimitMigration` 显式迁移建立；DatabasePlugin 只注册 service，不在启动时自动改 schema。未运行迁移导致的访问错误必须保留为故障，不能当成未限流/放行。已过期记录由数据库 store 每 256 次消费最多批量清理 512 行；窗口长度随记录保存以支持清理。
- 自定义 store 可实现 `RateLimitStore` 并通过 `RateLimiter::new` 注册；实现须说明共享范围、窗口语义、并发原子性和故障行为。进程内 store 不作为官方多副本默认。
- key 由调用方按场景构造（如 endpoint + IP/email），并包含稳定的策略身份；相同 key 必须始终使用同一窗口。数据库 store 只保存 SHA-256 摘要；这是避免直接持久化原文的伪名化，不抵抗对邮箱等低熵输入的猜测，也不是加密。各认证端点分别选择适当 key，避免单一邮箱 key 允许攻击者锁死受害者账户。
- 当前 contract 使用 fixed window，允许窗口边界出现突发，不承诺滑动窗口；`limit`、窗口秒数必须为正，失败关闭，不把存储故障转换成允许。
- PostgreSQL/MySQL 分别验证并发竞争、窗口重置、拒绝与 Retry-After、过期记录清理、数据库故障传播和独立 limiter 实例共享；M3 认证流程再证明所有敏感端点实际消费同一入口。

## 影响

限流不等于幂等、验证码尝试计数或防重点击。统一入口及数据库实现先交付；验证码仍按独立用途绑定/一次消费契约实现并复用此限流服务。
