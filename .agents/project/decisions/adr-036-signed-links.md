# ADR-036：用途隔离的限时签名链接

- 状态：Accepted
- 日期：2026-10-26
- 依据：[v1 能力覆盖](../plans/v1-capability-coverage.md)、[ADR-022 JWT 运行时 contract](adr-022-jwt-runtime-contract.md)、[ADR-034 应用数据加密](adr-034-application-encryption-and-key-rotation.md)

## 决策

1. 在可选 `signing` feature 下提供 `openrust::signing::{SignedLinks, SignedLinksPlugin}`，配置使用 `[security.signed_links]`；签名密钥必须与 `[auth.jwt]`、`[security.encryption]` 独立配置。固定 HS256，使用既有 `jsonwebtoken`，token type 为 `openrust-link+jwt`。
2. 每个 token 绑定 issuer、purpose、subject、签发与到期时间、随机 256-bit JTI 和 key ID。调用方必须在验证时提供预期 purpose；subject 是公开 bearer token 的可读内容，调用方不得放入秘密。有效期由调用方选择并受配置最大值（默认一天、最大 30 天）约束。
3. 默认无状态验证只保证签名、用途、签发者和到期时间；链接在过期或移除其验证密钥前不能逐条撤销。需要逐条撤销的应用必须启用共享 `SignedLinkRevocationStore`；PostgreSQL/MySQL 驱动只存 JTI 的 SHA-256 摘要和过期时间，迁移由应用加入既有 migrator。撤销存储故障使验证失败关闭。
4. key rotation 复用独立 key ring：新 token 只由 active key 签发，保留的旧 key 仅用于验证；移除旧 key 会提前使其签发的有效链接失效。不得回退到 JWT access 或加密密钥。
5. 邮箱验证码继续使用既有 `VerificationCodes` 原子单次消费；签名链接服务为可组合机制，不取代验证码、自动发信、账号枚举策略或业务授权。

## 验收

- 验证有效签名、错误用途/签发者/算法/类型、篡改、未知 key、过期、未来签发时间、最大 TTL、非法输入和 key rotation；bearer 与配置 Debug 脱敏。
- fake/custom revocation store 覆盖撤销及存储故障；数据库 store 在 PostgreSQL 与 MySQL 验证跨实例撤销和表名映射。
- `apps/server` 接入，生成项目/文档说明配置、密钥隔离、撤销迁移和应用责任；执行 workspace fmt/clippy/tests/docs、`cargo deny check`。

## 验收记录

2026-10-26：PostgreSQL 18、MySQL 8.4 的 `auth_database` 各 6 项通过；两种后端新生成 `--auth` 项目通过 fmt、check 和真实 HTTP auth flow；4 项 signing unit tests、workspace default/all-features clippy/test、warnings-denied docs、no-default-features check、`cargo deny check` 通过。deny 仅有既有 duplicate dependency 与 unused MPL allowance 警告。
