# ADR-026 通用验证码服务

- 状态：Accepted（通用验证码实现及 PostgreSQL 18 / MySQL 8.4 持久化并发、AuthFlows 与生成项目 HTTP 测试已验证）
- 日期：2026-10-26
- 依据：[ADR-011](adr-011-sms-and-communication-baseline.md)、[ADR-024](adr-024-authentication-flow-service.md)、[ADR-025](adr-025-shared-rate-limiter.md)、[v1 能力覆盖](../plans/v1-capability-coverage.md)

## 最小 Spec

- 在 `openrust-auth` 提供 `VerificationCodes` service 与公开 `VerificationCodeStore` 原子存储契约，由 AuthPlugin 在 DatabasePlugin 已注册时提供 PostgreSQL/MySQL 默认 store。
- 服务只生成、持久化和验证一次性验证码，不发送邮件/SMS，不自动创建账户、不触发登录/MFA；业务调用方把返回的 code 交给 Mailer/SMS 自行发送。
- 初始格式为 6 位数字；调用方提供稳定 purpose 与规范化身份值，校验时必须同时匹配两者。每个 purpose + identity 同时仅有一个有效 challenge；允许重发时旧 challenge 立即失效。
- `VerificationCodePolicy` 要求有效期、重发冷却、最大错误次数均显式且有边界；同一 purpose/identity 的所有节点共享冷却、尝试计数、有效期与一次消费状态。
- 使用现有 `Passwords` Argon2id PHC 哈希存 code，不持久化明文；身份索引保存 SHA-256 purpose/identity 摘要。低熵身份摘要仅为伪名化，文档不得称为匿名或加密。
- 数据库使用自身 UTC 时间；提交错误 code 时在 row lock/原子更新中增加次数，超过次数失效；成功校验只能有一个并发调用消费成功。过期、缺失、已消费与不匹配对调用方统一返回 false，存储错误仍作为服务故障传播。
- issue 在冷却窗口内返回 429/Retry-After，不签发新 code；认证/发送端点仍须先用 RateLimiter 做包含端点与请求来源的共享限流，避免仅按身份 key 锁定受害者。
- `AuthVerificationCodeMigration` 由应用现有 SeaORM migrator 显式注册；不得启动自动改 schema。双库契约测试覆盖并发 issue/verify、尝试耗尽、过期、冷却/重发、摘要存储、表名 resolver、故障和生成项目。

## 决策理由与边界

验证码属于可复用的认证证明机制，使用现有 `openrust-auth`、Plugin、DatabasePlugin、Passwords 与 RateLimiter，不新增平行 crate、发信入口或短信登录模式。Argon2id 对短数字码提供离线猜测成本；公开服务不暴露 code hash 的 Debug/日志，且敏感 endpoint 必须组合共享 RateLimiter。MFA、TOTP、完整 MFA recovery 与通用签名链接仍按各自 v1 能力 contract 决策，不由本 ADR 顺带实现。
