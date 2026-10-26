# `openrust::auth`

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：M3 认证运行时实现和双库验收事实；真实 Twilio 送达仍需授权账号与目标。

## 当前实现

- 配置结构位于 `openrust::config::auth`，旧的 `openrust::auth` 配置导出保持可用；本地 JWT 密钥可在 `.env` 设置，生成项目不写入真实密钥，production 忽略 `.env`。

- `AuthPlugin::new()` 注册数据库认证器；`finish` 检查认证服务未被后续插件覆盖、`Db` 已注册且 SeaORM backend 是 PostgreSQL 或 MySQL。`AuthPlugin::with_authenticator` 使用应用或第三方实现，不依赖 `Db`。
- `SessionPlugin` 通过 `tower-sessions` 安装通用 Cookie middleware；默认 `DatabaseSessionStore` 以 resolver 解析的 Session 表共享存储（默认 `tb_sessions`），支持自定义 `SessionStore`。可选 `session-redis` feature 提供 `RedisSessionStore`，通过 `SessionPlugin::with_store` 显式启用；以 namespace 隔离应用，键名仅使用 Session ID 的 SHA-256 摘要。`SET NX/XX PX` 分别保证创建不覆盖和保存不复活已删除 ID，TTL 采用当前记录到期时间；Redis 错误传播为 store 错误。PostgreSQL/MySQL 集成覆盖数据库 store 生命周期；真实 Redis 覆盖两个独立 App 共享读取、删除撤销、过期、碰撞、namespace 隔离和 CSRF。
- `Authenticator` 接收路由选中的 `AuthCredential`，返回用户 ID 与 PAT abilities；凭据字段不实现 `Debug` / 序列化。默认数据库实现验证共享 Session 身份和 CSRF，或验证 PAT 到期、撤销、用户禁用并更新 `last_used_at`。
- `JwtConfig`、`JwtPlugin` 与 `JwtTokens` 提供严格配置的 HS256 access-token 签发/验证、issuer/audience、kid key ring 与重叠密钥轮换。验证强制 `typ=openrust-at+jwt`、正数十进制账户 ID、jti、iat/nbf/exp 边界；`AuthMode::Jwt` 直接以 JWT 服务认证，不要求 SessionPlugin，不逐请求查数据库。
- `AuthLayer::required` / `optional` 通过 Axum `FromFnLayer` 接入现有 `Routes::layer`。路由模式包含 JWT、Session、PAT、Session-or-PAT；组合模式中的 Bearer 明确解释为 PAT。
- `AuthPlugin` 注册 `Passwords` Argon2id 服务；`PasswordConfig` 默认 memory=19456 KiB、iterations=2、parallelism=1，并限制为 memory 19456..=262144 KiB、iterations 1..=10、parallelism 1..=4。`with_password_config` 用于显式覆盖。hash 使用随机 salt 输出 PHC 编码；hash 与 verify 都在 Tokio blocking pool 执行。`EncodedPasswordHash` 不实现 Debug；解析和验证均检查 PHC 算法、版本与资源参数，不接受超出框架边界的存储值。
- `VerificationCodes` 通过 `AuthPlugin` + `DatabasePlugin` 注册，使用 `Passwords` Argon2id code hash 和 `DatabaseVerificationCodeStore`。purpose 与调用方规范化后的 identity 绑定，默认 6 位数字、10 分钟有效、60 秒重发间隔、最多 5 次错误；每 purpose/identity 同时只有一个 challenge。一次消费、尝试计数、重发冷却与有效期均在数据库共享，验证状态未知或失效统一返回 false；持久表由 `AuthVerificationCodeMigration` 显式创建。Mail/SMS 不由该服务发送。
- `AuthPlugin` 同时注册 `VerificationCodes`；`AuthVerificationCodeMigration` 创建 resolver-aware `verification_codes` 表。生成 `--auth` 项目注册 Auth / refresh / verification / Policy 与 database rate-limit migrations；通用限流已由 AuthFlows 在敏感流程中组合使用。
- `AuthFlowsPlugin` 在 `DatabasePlugin`、`AuthPlugin`、`JwtPlugin` 之后注册统一流程服务；缺少任一依赖或 refresh lifetime 越界时构建失败。服务提供注册/邮箱验证/登录/refresh/登出/密码重置编排，按 endpoint + identity 摘要 + 调用方可信 request-source 消费共享 `RateLimiter`。注册和找回返回 redacted code wrapper，不发送邮件/SMS；默认 30 天 refresh family、5 次/15 分钟固定窗口。密码重置与 refresh family 撤销在同一数据库事务提交。
- 注册仅将邮箱唯一约束冲突映射为 `None`；CHECK、截断及其他 SQL 错误继续传播，避免 MySQL `INSERT IGNORE` 将非重复错误伪装成重复账户。
- Cookie Session 登录为可选路径：`AuthFlows::login_session` 复用登录认证和限流、轮换 ID，写入用户 ID 与 CSRF token digest，并返回 Debug 脱敏的 256-bit CSRF token；`logout_session` 清理认证键、轮换 ID并保留应用数据。无效凭据不修改 Session。
- `CurrentUser` / `OptionalUser` 从请求扩展读取身份，可与 `Inject<T>` 同一 handler 使用。PAT abilities 精确匹配。
- 认证主体留在当前 HTTP request extension；`ExecutionMetadata` 只跨任务传递 request ID/locale，不序列化 `CurrentUser`、权限快照、Bearer token、Session cookie 或任意 header。后台 Job 的授权须在执行边界按应用策略重新确认。
- `PersonalAccessTokens` 由 `AuthPlugin` 在数据库已注册后放入 `AppState`，提供一次性签发、账户范围列举、单 token 撤销及账户级全量撤销。原始 token 以 256-bit CSPRNG 生成，仅 `IssuedPersonalAccessToken::expose` 可读，Debug 脱敏；数据库只存 SHA-256 和短前缀。列表不返回 digest，撤销 SQL 同时约束 user_id。
- `Policy` 由 `AuthPlugin` 在 `Db` 已注册时放入 `AppState`，并通过追加的 `AuthPolicyMigration` 建立全局 roles/permissions/user_roles/role_permissions。支持确保角色、角色分配/撤销、精确权限授予/撤销与显式 `allows` / `authorize`。权限检查每次读取共享数据库，不缓存；JWT/Session 需要角色授予，PAT 还需同名 token ability。授权管理路由由应用负责保护。
- `Policy::grant_permission_in_transaction` 接受调用方事务，用于将权限变更与 `openrust::audit::DatabaseAuditSink::record_in_transaction` 原子提交；常规 `grant_permission` 保持内部事务兼容。
- `AuthMigration` 创建中性逻辑名 `users`、`sessions` 与 `personal_access_tokens`，实际物理名由 ADR-016 的 `DatabaseTableNames` resolver 解析（默认 `tb_*`）。PostgreSQL/MySQL 集成验证默认、空前缀、自定义前缀和 `users` 完整名映射，并走通认证持久化查询。
- `AuthRefreshTokenMigration` 作为追加迁移创建逻辑名 `auth_refresh_tokens`，不修改原认证迁移版本。`AuthPlugin` 在先注册 `DatabasePlugin` 时向应用状态注册 `RefreshTokens`；它签发 32 字节随机 opaque token、只持久化 SHA-256、单次事务轮换、重放撤销整组，并提供单 token 与账户级撤销。账户行锁将签发、轮换和撤销串行化，避免并发轮换在账户级撤销后留下后继 token。初次过期时间由 AuthFlows 默认 30 天或显式配置提供，轮换不延长。
- `openrust new --db postgres|mysql --auth` 生成 JWT/Auth/AuthFlows 插件、Auth / refresh / verification / Policy migrations、`DatabaseRateLimitMigration`、JWT 默认保护的 `/api/me`、数据库 PAT 保护的 `/api/me/pat`、注册/验证/登录/refresh/logout/密码找回 HTTP routes 和 `[auth.jwt]` 配置区；密钥必须通过 `APP_AUTH__JWT__KEYS__LOCAL` 设置，不内嵌示例密钥。流程端点使用 `ClientIp` extractor；`[server].trusted_proxy_ranges` 默认为空，仅配置的代理 peer 可提供可信 XFF 链。认证代码调用 `Mailer`，由应用配置 SMTP；默认 `log` driver 只确认接收，不暴露验证码，也不代表邮件已送达。
- 认证生成物附带 `tests/auth_flows.rs`：使用 `TestDatabase` 创建独立数据库、替换 `Mailer` 为 fake，并通过真实 TCP listener 验证生成路由。覆盖统一响应、`Inject` 与 `ClientIp` 共存、可信 XFF、验证码、JWT 当前用户、默认数据库 PAT 认证、登录限流错误、refresh 单次轮换/重放拒绝、密码重置及登出撤销。由于默认 `HttpStatusMode::AlwaysOk`，错误的稳定 `code` 在统一 JSON envelope 中表达，HTTP status 保持 200。运行数据库 e2e 需设置 `OPENRUST_TEST_DATABASE_URL` 并授予测试账号创建/删除数据库权限；未设置时测试会提示并跳过。

## 边界与未纳入范围

- AuthFlows service contract 已记录在 [ADR-024](../decisions/adr-024-authentication-flow-service.md) 并已实现；验证码与 RateLimiter 数据库实现分别见 ADR-026/025，Policy contract 和基础实现见 ADR-027。验证码消费和账户变更不是同一事务；密码更新与 refresh family revoke 是单一事务。
- 生成项目 HTTP handlers 的真实 HTTP/fake-mail/`ClientIp` 集成覆盖已加入。2026-10-26 按当前模板新生成的 PostgreSQL 18 与 MySQL 8.4 项目均通过格式检查、`cargo check` 和完整 JWT/PAT HTTP 流程；Session 登录/退出及 CSRF 流程也在双库隔离集成测试覆盖。
- 注册、登录、找回、邮箱验证、refresh/logout 与共享限流已有 service 编排和生成 HTTP 路由模板；可信代理来源策略按 ADR-028 实现。
- Policy/basic RBAC 已实现；复杂业务策略、租户范围与可缓存授权模型未纳入本 contract。
- `apps/server` 的认证集成测试构建各自测试 Application；默认 `Server::build` 没有安装 Auth/JWT/Database 插件，启用 `auth` feature 不会自动安装它们。完整认证业务 HTTP 流程由 CLI 生成项目通过真实 TCP 测试覆盖。
- 按 [ADR-041](../decisions/adr-041-transaction-and-auth-flow-events.md)，`AuthFlows` 提供 direct `RegistrationValidating`、`LoginValidating`、泛化 `LoginFailed`，以及事务内 `RegistrationSucceeded`、`LoginSucceeded`。拒绝校验或成功监听器错误会回滚对应账户/refresh 写入；注册验证码在账户提交后单独签发，Session 登录不声明 SessionStore 原子提交。规范化邮箱只供验证策略使用，监听器不得记录；事件不包含任何认证秘密。
- AuthFlows 敏感接口限流及可配置的通用 HTTP `RateLimitLayer` 已实现，路由/key/策略边界见 [HTTP 事实](http.md)；`generate mailer` / `generate policy` 的双库生成验收见 [测试基线](../memory/testing.md)。这些交付不代表公开业务钩子或总体资源限制已完成。
- 密码服务使用受参数限制的 Argon2id 与 `spawn_blocking`，当前服务本身没有独立的总体并发许可或排队容量；生产资源控制按 [补强计划](../plans/v1-performance-and-readiness.md#2-过载阻塞操作与超时) 补齐，不能由单次参数上限推定全进程内存有界。

- Twilio 适配器有请求协议、认证、错误脱敏、timeout 与无重试测试，尚无已授权账号和目标号码的真实送达证据。认证和通用安全本地实现及双库验收不依赖该外部条件。

## 验证

- `cargo test -p openrust --test http_router --test http_resource`：全局 Tower layer 与错误响应包装测试通过。
- `cargo test -p server --features auth --test auth_runtime`：共享 Session middleware 与身份认证运行时测试通过；`session-redis` feature 及 Redis adapter 证据见 `memory/testing.md`。
- `cargo test -p openrust-server --features auth,db-postgres --test auth_database` / `--features db-mysql --test database`：真实本地容器验证迁移、CSRF、PAT 签发/哈希存储/列举/过期输入/归属隔离/跨实例撤销与最近使用时间、通用 Session store 和两个应用实例间撤销；MySQL 使用具备建库权限的测试账号。
- `cargo test -p openrust --features auth --lib`：JWT 配置、签发/验证、错误边界及密钥轮换测试通过。
- `cargo test -p openrust --features auth --lib`：验证码用途/身份绑定、错误尝试耗尽、一次消费和策略边界单测通过；`session_login_rotates_id_preserves_app_data_and_redacts_csrf` 验证 ID 轮换、应用数据保留、CSRF digest 与 Debug 脱敏。
- `OPENRUST_TEST_DATABASE_URL=... cargo test -p openrust-server --features auth,db-postgres --test auth_database` 和 MySQL 对应命令：验证 refresh 哈希存储、旋转与不延长有效期、重放撤销、账户撤销、两请求并发刷新与并发账户撤销；验证 `AuthPlugin` 注册的应用级 `RefreshTokens`。
- 历史 PostgreSQL 14 / MySQL 8.0 真实隔离库 `database` 集成测试覆盖 verification migration 与 table-name resolver、Argon2id 摘要存储、resend cooldown、purpose/identity 绑定、尝试耗尽、过期失效，以及两个独立 App/AuthPlugin 实例间恰好一次并发消费。
- `cargo test -p openrust-server --features auth --test auth`：facade JWT Bearer 路由在不注册 SessionPlugin 时认证成功，并与 `Inject<T>` 共存。
- `cargo test -p openrust-cli`：生成模板断言 JWT/AuthFlows plugins、认证路由、`ClientIp`、根 `config/` 与外置密钥变量。
- `OPENRUST_TEST_DATABASE_URL=... cargo test -p openrust-server --features auth,db-postgres|auth,db-mysql --test auth_database`：真实隔离库认证流程新增 Session 登录/登出、CSRF、旧 ID 与并发旧请求失效及保留应用数据断言；2026-10-26 在 PostgreSQL 16 与 MySQL 8.4 上真实运行通过。
- `OPENRUST_TEST_DATABASE_URL=mysql://... cargo test --test auth_flows`：此前生成项目端到端流程在 MySQL 8.4.11 通过，覆盖验证码、限流 envelope、JWT、refresh 重放、密码重置和登出；PostgreSQL 最新生成项目复验待本机数据库恢复。
- 2026-10-26 当前基线复核：PostgreSQL 18 与 MySQL 8.4 的 `auth_database` 各 6 项、`auth_runtime` 各 6 项通过；当前生成模板项目在两库分别通过 `auth_flows` 真实 TCP HTTP 测试。覆盖注册/重复注册、邮箱验证、登录、Session 轮换/CSRF/退出、JWT 与 PAT、refresh 轮换/重放、密码重置、共享限流、Policy/RBAC 与 PAT ability 交集、可信代理和统一错误 envelope。
- 2026-10-26：默认及 all-features workspace fmt/clippy/test/doc 全通过；认证真实隔离库流程测试在 PostgreSQL 16 与 MySQL 8.4 通过；新生成 PostgreSQL、MySQL 项目分别运行 `cargo fmt --all --check && cargo check && cargo test` 通过。MySQL 模板构建提示有数个未启用可选 crate patch，不影响已启用图谱或测试结果。
- 2026-10-26：`policy_grants_and_revocations_are_visible_to_authenticated_routes` 在 PostgreSQL 16 与 MySQL 8.4 真实隔离库通过，验证 JWT 路由默认拒绝、授予/撤销即时生效、PAT ability 与数据库授权取交集，并覆盖自定义用户表映射。
