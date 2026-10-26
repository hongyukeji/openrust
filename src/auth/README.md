# 认证与授权

认证配置定义于 `openrust::config::auth`，并从 `openrust::auth` 重新导出以保持兼容。运行时 TOML 放在应用根目录的 `config/`；本地签名密钥通过 `.env` 或进程环境变量提供，切勿将密钥材料写入受版本控制的配置文件。生产环境启动时忽略 `.env`，仅使用进程环境变量。

OpenRust 的认证与授权能力。

## 当前能力

- `AuthPlugin::new()` 会注册数据库认证器和共享 refresh token 服务。该插件依赖 `DatabasePlugin`、PostgreSQL 或 MySQL，以及 `AuthMigration` 创建的表；使用 refresh token 时还需要 `AuthRefreshTokenMigration`。
- `SessionPlugin::new()` 会安装共享数据库 Session middleware。请在 `AuthPlugin` 之前注册 `DatabasePlugin` 和 `SessionPlugin`；自定义 store 可实现 `tower_sessions::SessionStore`，并传给 `SessionPlugin::with_store`。
- 如果 `DatabasePlugin` 已先注册 `Db`，`AuthPlugin` 会注册 `RefreshTokens`。它生成高熵不透明凭据，只持久化 SHA-256 摘要，以原子方式轮换，在检测到重放时撤销整个 token family，并支持按账户整体撤销。调用方提供 family 的初始过期时间；轮换不会延长该时间。
- `openrust new --db postgres|mysql --auth` 会生成 JWT/Auth/AuthFlows plugins、认证迁移、JWT 保护的 `/api/me`、数据库支持的 PAT 保护 `/api/me/pat`，以及注册、邮箱验证、登录、刷新、登出、密码重置路由和 `[auth.jwt]` 配置。签名密钥必须通过 `APP_AUTH__JWT__KEYS__LOCAL` 提供，不能写入生成的配置。
- `AuthPlugin::with_authenticator` 通过现有 `Plugin` 生命周期注册应用自定义或第三方 `Authenticator` 实现。
- `AuthPlugin` 还会在 `AppState` 注册 Argon2id `Passwords` 服务。它使用随机 salt 生成 PHC 格式哈希，并在 Tokio blocking pool 中验证。可通过 `AuthPlugin::new().with_password_config(PasswordConfig { memory_kib: 19456, iterations: 2, parallelism: 1 })` 配置；默认值为 19456 KiB、2 次迭代和 1 个 lane，并限制可接受范围。验证前会检查编码中的算法、版本和成本参数，以拒绝不安全的存储值。`EncodedPasswordHash::as_str()` 是持久化边界；该封装有意不实现 `Debug`。空密码和超过 1024 字节的密码会被拒绝；密码不匹配时返回 `Ok(false)`；格式错误的 PHC 数据会返回内部错误。
- 配置 `DatabasePlugin` 后，`AuthPlugin` 会使用共享数据库 store 和同一个 Argon2id 密码哈希器注册 `VerificationCodes`。签发验证码前，将 `AuthVerificationCodeMigration` 加入应用 migrator。默认策略生成六位数字，有效期十分钟，最多允许五次错误尝试，并设置 60 秒重发冷却时间。返回的 `IssuedVerificationCode` 会在 `Debug` 中隐藏验证码；请自行通过 Mail/SMS 发送，且不要记录日志。签发/验证路由前应应用共享 `RateLimiter`，key 至少包括 endpoint 和请求来源，并可加入身份专属策略。
- `AuthFlowsPlugin` 在 `DatabasePlugin`、`AuthPlugin` 和 `JwtPlugin` 之后注册 `AuthFlows`。它组合账户注册、邮箱验证、登录、刷新、登出、密码重置请求与密码替换，但不会定义 HTTP 路由或发送邮件/SMS。默认 refresh family 有效期为 30 天；共享固定窗口限流默认为每个 endpoint/source/identity 每 15 分钟最多五次请求。请传入可信请求来源值（例如 socket peer 地址，或仅根据显式可信代理得出的值）；不要信任任意转发头。
- JWT 登录仍是默认方式。选择 Cookie Session 登录的应用可调用 `AuthFlows::login_session`；该方法会在共享登录限流下验证凭据、轮换 Session ID、存储账户 ID 和 CSRF token 的 SHA-256 摘要，并返回经过脱敏的 `SessionCredentials`。应用响应中应将 CSRF token 返回给客户端。退出路由使用 `AuthMode::Session` 保护，并调用 `AuthFlows::logout_session`；它会删除认证 key、轮换 ID，同时保留应用自己的 Session 数据。两种方式都依赖 `SessionPlugin`。
- `AuthFlows::register`、`request_email_verification` 和 `request_password_reset` 会返回可选的 `IssuedVerificationCode`；该封装的 `Debug` 输出会隐藏验证码。只有电子邮件唯一约束冲突才会让重复注册返回 `None`；其他数据库错误会继续传播。遇到无效或过期挑战时，`verify_email` 和 `reset_password` 返回布尔值。Handler 可保持公开响应一致，只在有验证码时发送。对于不存在、未验证、已禁用账户或密码错误，`login` 返回相同的 unauthorized 错误。登录/刷新成功时返回 `AuthCredentials`，其 `Debug` 输出会隐藏两种 bearer 凭据。DTO、响应格式、路由授权和消息发送均由 handler 负责。

`--auth` 生成路由接受以下 JSON 字段：`/auth/register` 和 `/auth/login` 使用 `email` + `password`；`/auth/request-email-verification` 和 `/auth/password-reset` 使用 `email`；`/auth/verify-email` 使用 `email` + `code`；`/auth/refresh` 和 `/auth/logout` 使用 `refresh_token`；`/auth/password-reset/confirm` 使用 `email`、`code` 和 `new_password`。凭据响应会在 JSON 响应封装中各返回一次 access token 和 refresh token。限流来源 key 使用公开的 HTTP `ClientIp` extractor：默认以 TCP peer 为准；仅当 peer 匹配 `[server].trusted_proxy_ranges` 时才信任 `X-Forwarded-For`。
- `JwtPlugin` 会在启动时校验显式提供的 key ring；`JwtTokens` 用于签发和验证短时 HS256 access token。`AuthLayer` 支持不依赖 Session middleware 的 `AuthMode::Jwt`；`SessionOrToken` 仍将 Bearer 凭据识别为 PAT。
- `CurrentUser` 和 `OptionalUser` 用于提取已验证的账户身份，并暴露账户 ID、凭据来源和 PAT abilities；应用 handler 根据 ID 加载自己的业务用户模型。
- PAT 数据库认证会在每个请求中检查撤销、过期和账户禁用状态，并更新 PAT 的 `last_used_at`。变更提交后，使用同一数据库的其他节点会在下一次请求中看到新状态。Cookie Session 认证使用 [ADR-017](../../.agents/project/decisions/adr-017-generic-session-runtime.md) 定义的共享 Session 运行时。
- `PersonalAccessTokens` 提供账户范围内的签发、列举和撤销操作。256 位 bearer 值仅返回一次，数据库只存储其 SHA-256 摘要和短前缀。列举操作不返回秘密；撤销限定于账户，并会立即共享给使用数据库的认证器。
- `Policy` 提供精确匹配、基于数据库的角色权限。`ensure_role`、`assign_role`、`grant_permission` 及相应撤销方法管理全局角色分配。`allows` / `authorize` 每次都会读取共享数据库；JWT 和 Session 需要已授予的角色权限，PAT 还必须拥有完全匹配的 token ability。未知权限默认拒绝；权限撤销提交后，各节点在下一次检查时都会看到。

```rust,ignore
use openrust::auth::{AuthLayer, AuthMode, AuthPlugin, CurrentUser, SessionPlugin};

app.plugin(SessionPlugin::new())
    .plugin(AuthPlugin::new())
    .routes(|routes| {
        routes.get("/api/me", me).layer(AuthLayer::required(AuthMode::SessionOrToken))
    });

async fn me(user: CurrentUser) -> openrust::Result<openrust::http::Json<i64>> {
    Ok(openrust::http::Json(user.id()))
}
```

应用 migrator 必须且只包含一次 `AuthMigration`；使用 refresh token 时加入 `AuthRefreshTokenMigration`，使用验证码时加入 `AuthVerificationCodeMigration`，使用 Policy 时加入 `AuthPolicyMigration`。`--auth` 项目模板会注册这些认证迁移，以及 `DatabaseRateLimitMigration` 和 `DatabaseAuditMigration`。默认迁移会创建中性的 `tb_users`、`tb_sessions`、`tb_personal_access_tokens`、`tb_auth_refresh_tokens`、`tb_verification_codes`、`tb_roles`、`tb_permissions`、`tb_user_roles`、`tb_role_permissions` 和 `tb_audit_records` 表；`DatabaseConfig.table_prefix` 与 `table_names` 可选择其他经过校验的物理表名，认证/授权查询使用相同 resolver。应用专属用户列和业务数据由应用管理。PAT、refresh 凭据和 Cookie Session ID 以 SHA-256 摘要形式存储；通用 Session 数据存储在共享 Session 记录中。验证码行包含 purpose/identity 摘要与 Argon2id 验证码哈希，不保存原始 identity 或验证码。摘要属于假名化处理，并非加密，也无法阻止对低熵 identity 的猜测。修改状态的 Session 请求必须提供匹配的 `x-csrf-token`；已选凭据无效时绝不会降级为匿名访问。

若需以原子方式审计权限变更，请在调用方的 `Db::transaction` 中调用 `Policy::grant_permission_in_transaction`，并在同一事务提交前通过 `DatabaseAuditSink::record_in_transaction` 写入已准备好的 `AuditRecord`。`--auth` 模板会注册 `AuditPlugin::database()` 并加入 `DatabaseAuditMigration`；审计事件仍需显式记录，`AuthFlows` 不会默默审计所有操作。数据策略与 sink 故障语义见[审计模块指南](../audit/README.md)。

`VerificationCodes::issue(purpose, normalized_identity, policy)` 只返回数字验证码，不负责发送。验证时 purpose 和 identity 必须完全匹配；调用方负责保持规范化方式一致。每个 purpose/identity 同时只允许一个有效验证码；验证成功后会在各节点间原子消费。格式错误或验证码错误会消耗有限的尝试次数；挑战不存在、过期、已消费、次数耗尽、过时或不匹配时均返回 `Ok(false)`。冷却时间尚未结束时重发会返回带 `Retry-After` 的 429。自定义存储可实现 `VerificationCodeStore`，但必须提供原子的冷却、尝试计数和单次消费语义。详见 [ADR-026](../../.agents/project/decisions/adr-026-generic-verification-codes.md)。

服务层不会把验证码消费和账户更新放进同一数据库事务：账户变更前验证码已经被消费。如果之后的写入失败，调用方应允许用户重新申请验证码。密码替换与 refresh family 撤销共用一个数据库事务。登出会撤销 refresh 凭据；已签发的 access token 仍会在短暂 TTL 到期前有效。生成路由使用公开的 `ClientIp` extractor：默认以 socket peer 为准；只有 peer 匹配已配置的 `[server].trusted_proxy_ranges` 时才信任 `X-Forwarded-For`。请按实际反向代理链配置可信范围。

`AuthCredential` 有意不实现 `Debug` 或序列化。自定义认证器不得记录其中的秘密。凭据无效时返回 `Error::Unauthorized`；CSRF 校验拒绝时返回 `Error::Forbidden`；后端故障时返回内部错误或显式 5xx 错误。

通用 Session store、Cookie middleware、过期、删除和共享失效能力已实现，并在 PostgreSQL/MySQL 上测试。JWT access token 签名、严格校验、key ID、重叠密钥轮换和 JWT 路由认证均已实现。Refresh token 摘要持久化、单次轮换、重放时撤销 family、账户级撤销和并发刷新串行化均已实现，并在 PostgreSQL/MySQL 上测试。Argon2id 密码原语、通用验证码服务、`AuthFlows` 服务组合和基础数据库角色权限均已实现，并在 PostgreSQL/MySQL 上测试。Session 登录/登出 helper 覆盖 ID 轮换、CSRF 摘要存储、脱敏、保留应用数据及移除认证 key，并在 PostgreSQL/MySQL 上测试数据库流程。生成的认证路由通过隔离数据库和 fake mail HTTP 测试；注册、邮箱验证、JWT/PAT 认证、refresh 重放、密码重置、登出撤销和限流响应已在 PostgreSQL 18 与 MySQL 8.4 上通过真实 TCP 验收。应用 handler 仍负责业务 DTO 和路由授权。真实 Twilio 送达尚未验证，需提供获授权的账号和目标地址。
