# ADR-004 认证默认为 Cookie 会话与个人访问令牌

- 状态：Partially Superseded（默认认证及排除 JWT 由 [ADR-018](adr-018-jwt-primary-authentication.md) 替代；原多租户排除由 [V1 补齐计划](../plans/v1-foundation-completion.md) 调整；账户、密码与通用流程原则仍有效，具体运行时见 ADR-022/024/026/027）
- 日期：2026-10-25

## 背景

框架同时面向浏览器端应用与 API 服务。无状态 JWT 难以即时撤销与审计，不适合作为唯一默认。

## 决策

- 提供两种认证方式：通用 Cookie Session（共享数据库默认；凡通过 Cookie Session 认证的 unsafe method 均校验 CSRF）与个人访问令牌（存储 SHA-256 哈希，支持前缀、能力列表、过期时间）。Session 的公开入口、共用存储和轮换规则见 ADR-017；身份与路由 API 见 ADR-012。
- 认证账户与 PAT 由 `openrust::auth::AuthMigration` 加入应用现有 SeaORM migrator；不另建迁移运行系统。账户保存 `email`、`password_hash`、`email_verified_at`、`disabled_at` 与 UTC 时间字段，领域资料由应用自己的表按 `id` 关联。Session 不再采用仅含 `user_id` 和认证摘要的独立 schema，改用 ADR-017 定义的通用 Session 记录；PAT 表保存前缀、token 哈希、JSON abilities、可选过期/撤销/最近使用时间。原始认证凭据不落库；撤销和禁用按共享数据库语义传播。
- 密码哈希使用 RustCrypto `argon2` 0.6 的 Argon2id 与 PHC 格式；默认参数为 memory=19456 KiB、iterations=2、parallelism=1，可显式调整但受框架上/下界约束。新密码哈希使用随机 salt；PHC 中解析出的算法、版本和资源参数也须满足同一安全边界，避免异常存储数据触发无界计算。哈希和验证放入 Tokio blocking pool；不实现或记录密码明文与 PHC 哈希的 `Debug`。认证流程复用同一 `Passwords` 服务，不引入第二种口令哈希实现。登录、注册、密码重置端点默认限流。
- 内置流程：注册、登录、登出、密码重置、邮箱验证；授权用 `Policy` + 最小角色权限模型。
- （已由 ADR-018 替代）JWT 不在第一代；曾作为 0.2 候选 `auth-jwt` feature。
- V1 必须交付可选租户隔离机制，复用现有身份、Policy、查询与共享状态契约，详见 [补齐计划](../plans/v1-foundation-completion.md)；当前基础认证实现不等于已完成租户机制。部门/组织业务模型及其 DataScope 管理界面仍归 V2，不将其写入通用认证核心。

## 影响

- 认证流程依赖邮件能力，因此邮件提前到 M3；当前模块为 `openrust::auth` 与 `openrust::mail`。
- 用户表结构由迁移定义，业务通过追加迁移扩展字段，不继承框架结构体。
- 框架逻辑表名、默认 `tb_` 前缀与自定义映射遵循 [ADR-016](adr-016-neutral-application-namespaces.md)；AuthMigration 与认证 Session/PAT/refresh 查询均已使用统一表名 resolver。
