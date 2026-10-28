# `openrust::signing` 当前事实

> 更新：2026-10-26
> 摘要：可选 `signing` feature 提供用途绑定的限时 signed-link JWT、独立 key ring 与可选共享撤销。

- `SignedLinks` 固定 HS256、严格 token type、issuer 和 `kid`；每个 link 带 purpose、subject、iat、exp、随机 256-bit JTI。有效期需大于零且不超过配置上限（默认一天、最大 30 天）；subject 由 token holder 可读，不可放秘密。
- `[security.signed_links]` key ring 与 `[auth.jwt]`、`[security.encryption]` 分离；新 token 仅用 active key 签发，保留旧 key 只用于验证。无默认 key。
- 默认无状态验证；逐 link 撤销需配置共享 `SignedLinkRevocationStore`。内置 PostgreSQL/MySQL 实现只存 JTI SHA-256 与 expiry；`SignedLinkRevocationMigration` 加入应用既有 migrator。验证遇到 store 错误时失败关闭。
- `DatabaseSignedLinkRevocationStore::purge_expired(limit)` 删除 1–1000 行；由应用 scheduler/maintenance command 调用。业务负责链接传输、主体授权和 URL 构造；认证验证码仍按 `VerificationCodes` 原子单次消费。
- CLI `--auth` 模板注册 `SignedLinksServiceProvider`、数据库撤销 store 与迁移，配置写入独立 link key section；`.env.example` 只提供空 secret 占位。

验收：4 项 signing unit tests 通过；PostgreSQL 18 与 MySQL 8.4 的完整 `auth_database` 各 6 项通过；生成两种 backend 的 `--auth` 项目均通过 fmt、`cargo check` 和真实 HTTP 流程。workspace 全量门禁结果更新于 `memory/testing.md`。
