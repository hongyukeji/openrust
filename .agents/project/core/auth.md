# `openrust::auth`

✅ S3b SQLite本地认证/MFA限定验收通过，本地提交以Git为准；源17新调用方门禁、四fresh及独审齐备，源9/12/13按未变路径限定复用。双库22项、最低3.26认证52/限流2/Root5，fresh各3无skip；234生成源码归档，所有任务target已正常clean至0。完整Guard/Fortify/Google真实客户端、SQLite其他后端能力与故障窗口仍开放，见memory/testing.md「SQLite 本地认证与 MFA 限定验收」及唯一todo。原Goal active，新99/19.8与旧27/5.4历史保留、creditsfalse。

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：M3 认证运行时、M5 查询授权/OIDC/TOTP MFA 实现事实；真实 Twilio 送达仍需授权账号与目标。

## 当前实现

- 配置结构位于 `openrust::config::auth`，旧的 `openrust::auth` 配置导出保持可用；JWT 密钥可放在受保护的 `.env` 或由部署环境注入，进程环境优先；生成项目不写入真实密钥。

- `AuthServiceProvider::new()` 注册共享密码服务与数据库认证器的同容器异步工厂；boot 解析当前最终绑定的认证服务，并检查数据库后端为 PostgreSQL 或 MySQL。`AuthServiceProvider::with_authenticator` 使用应用或第三方实现，不依赖 `Db`；后序 Provider 可按容器覆盖契约提供完整自定义服务。
- `auth::Session` 与相关 store 现重导唯一 `openrust::session` 实现，旧 tower Session 内核已移除。`SessionServiceProvider` 复用最终配置、Cookie 和请求 Scope；默认数据库 store、公开 custom store 与显式内存路径见 [Session 当前事实](session.md) 和 [使用指南](../../../src/session/README.md)。40位ID按原始字节计算摘要，数据库保存有界JSON文本；Insert不覆盖、已加载记录Update不复活删除/过期ID是保留的安全增强。本簇当前Store专项、Auth database双库各8、Redis Auth10、根模块HTTP各53、模块OIDC各8及真实Keycloak主流程各1均已按新运行时验收；完整CSRF和上游Auth默认对应仍开放，详见testing。
- C04-C已验证的Cookie外层继续给Session提供明文视图，保原始wire供认证严格重复检查；其旧双库各48项记录不自动沿用为D1通过。当前配置改为lifetime=120分钟、secure=None按可信传输推导、same_site可空且默认lax，Cookie名按最终app.name推导或显式配置。最终配置延迟装配与overlay沿历史决策 082，完整Session/Web范围仍开放。
- `SessionStoreHandle::authorized_until(&Session, user_id)` 从该Session实际绑定的服务器端handler重新读取已认证会话，核对存储身份与请求快照及用户 ID，返回真正的持久截止时间；缺记录、过期、身份不符或读取失败均拒绝，不因查询滑动延长期限。模块浏览器 WebSocket 在握手时使用该截止；建连后的撤销不自动即时踢出，最长持续至已读取的截止。外部 test-app consumer 仅证明公开签名可调用，真实行为见模块双库测试。
- C04-D4的Cookie客户端快照仅在真实Cookie中间件验证完整性且ID/合法数据ID未被except时支持Session认证；`supports_session_authentication`默认为服务器后端允许、客户端后端拒绝，自定义包装须委托。实际Session凭据、默认Db认证入口、登录/登出副作用前、OIDC关联与最终save均校验；plain或被排除保护时返回500 `session_cookie_protection_required`，JWT-only不误拒。Cookie的无请求远程delete及需持续服务器撤销保证的authorized_until明确Unsupported，不假报OIDC远程会话已删。双库plain负例各1及完整加密根组各62已按各自范围通过；完整Auth上游对应与集群仍开放。
- `Authenticator` 接收路由选中的 `AuthCredential`，返回用户 ID 与 PAT abilities；凭据字段不实现 `Debug` / 序列化。默认数据库实现验证共享 Session 身份和 CSRF，或验证 PAT 到期、撤销、用户禁用并更新 `last_used_at`。
- `JwtConfig`、`JwtServiceProvider` 与 `JwtTokens` 提供严格配置的 HS256 access-token 签发/验证、issuer/audience、kid key ring 与重叠密钥轮换。验证强制 `typ=openrust-at+jwt`、正数十进制账户 ID、jti、iat/nbf/exp 边界；`AuthMode::Jwt` 直接以 JWT 服务认证，不要求 SessionServiceProvider，不逐请求查数据库。
- `AuthLayer::required` / `optional` 通过 Axum `FromFnLayer` 接入现有 `Routes::layer`。路由模式包含 JWT、Session、PAT、Session-or-PAT；组合模式中的 Bearer 明确解释为 PAT。
- `AuthServiceProvider` 注册 `Passwords` Argon2id 服务；`PasswordConfig` 默认 memory=19456 KiB、iterations=2、parallelism=1，并限制为 memory 19456..=262144 KiB、iterations 1..=10、parallelism 1..=4。显式配置走 `auth.password` 区段及最终 validator。hash 使用随机 salt 输出 PHC 编码；hash 与 verify 都在 Tokio blocking pool 执行。默认每个 `Passwords` 实例最多并行 2 个操作，可用 `AuthServiceProvider::with_password_operation_capacity` 或 `Passwords::with_max_concurrent_operations` 配置到 1..=4；满载立即以 503 拒绝且无排队。按已接受 PHC 的最大 memory 参数计算，单实例 Argon2 工作内存最多约 1 GiB。Tokio 阻塞任务持有容量许可直到实际完成，即使调用方取消等待也不提前释放。AuthServiceProvider 的认证和验证码服务共享同一实例；单独构造的实例分别计量。`EncodedPasswordHash` 不实现 Debug；解析和验证均检查 PHC 算法、版本与资源参数，不接受超出框架边界的存储值。
- `VerificationCodes` 通过 `AuthServiceProvider` + `DatabaseServiceProvider` 注册，使用 `Passwords` Argon2id code hash 和 `DatabaseVerificationCodeStore`。purpose 与调用方规范化后的 identity 绑定，默认 6 位数字、10 分钟有效、60 秒重发间隔、最多 5 次错误；每 purpose/identity 同时只有一个 challenge。一次消费、尝试计数、重发冷却与有效期均在数据库共享，验证状态未知或失效统一返回 false；持久表由 `AuthVerificationCodeMigration` 显式创建。Mail/SMS 不由该服务发送。
- `AuthServiceProvider` 同时注册 `VerificationCodes`；`AuthVerificationCodeMigration` 创建 resolver-aware `verification_codes` 表。生成 `--auth` 项目注册 Auth / refresh / verification / Policy 与 database rate-limit migrations；通用限流已由 AuthFlows 在敏感流程中组合使用。
- `AuthFlowsServiceProvider` 在 `DatabaseServiceProvider`、`AuthServiceProvider`、`JwtServiceProvider` 之后注册统一流程服务；缺少任一依赖或 refresh lifetime 越界时构建失败。服务提供注册/邮箱验证/登录/refresh/登出/密码重置编排，按 endpoint + identity 摘要 + 调用方可信 request-source 消费共享 `RateLimiter`。注册和找回返回 redacted code wrapper，不发送邮件/SMS；默认 30 天 refresh family、5 次/15 分钟固定窗口。密码重置与 refresh family 撤销在同一数据库事务提交。
- 注册仅将邮箱唯一约束冲突映射为 `None`；CHECK、截断及其他 SQL 错误继续传播，避免 MySQL `INSERT IGNORE` 将非重复错误伪装成重复账户。
- Cookie Session 登录为可选路径：`AuthFlows::login_session` 复用登录认证和限流、轮换 ID，写入用户 ID 与 CSRF token digest，并返回 Debug 脱敏的 256-bit CSRF token；`logout_session` 清理认证键、轮换 ID并保留应用数据。无效凭据不修改 Session。
- 可选 `auth-mfa` feature 由 `MfaServiceProvider` 显式启用，依赖数据库、`AuthServiceProvider` 的密码/共享限流服务与 `EncryptionServiceProvider`；启动时检查 MFA 表和数据库审计表。应用 migrator 必须注册 `AuthMfaMigration`。TOTP seed 使用本地 `Encryptor` 加密，RFC 6238 采用 SHA-1、30 秒时间步、6 位码和 ±1 窗口；因子行锁及 `last_used_step` 防止重放。
- `AuthFlows::begin_login` 返回 `LoginAttempt`，启用 MFA 的账户仅获得来源绑定、5 分钟有效、最多五次的挑战；`complete_mfa_login` 在同一事务消费挑战/因子、签发 JWT/refresh 并写审计。兼容的 `login` 与 Session 登录对已启用 MFA 的账户拒绝；MFA 启用时，`AuthLayer` 也会在每个 Session 认证请求检查因子状态并拒绝旧 Session。OIDC 使用同一挑战路径。
- MFA enrollment 需先通过已授权应用路由重新验证本地密码；待确认 seed 仅在 OTP 确认后启用。十个随机恢复码只显示一次，库中仅存 SHA-256 摘要；恢复码按事务原子消费。启用、禁用和恢复码重置写数据库审计并撤销账户 refresh token。已签发无状态 JWT 仍在原 TTL 内有效。
- `CurrentUser` / `OptionalUser` 从请求扩展读取身份，可与 `Inject<T>` 同一 handler 使用。PAT abilities 精确匹配。
- 认证主体留在当前 HTTP request extension；`ExecutionMetadata` 只跨任务传递 request ID/locale，不序列化 `CurrentUser`、权限快照、Bearer token、Session cookie 或任意 header。后台 Job 的授权须在执行边界按应用策略重新确认。
- `PersonalAccessTokens` 由 `AuthServiceProvider` 在数据库已注册后放入 `AppState`，提供一次性签发、账户范围列举、单 token 撤销及账户级全量撤销。原始 token 以 256-bit CSPRNG 生成，仅 `IssuedPersonalAccessToken::expose` 可读，Debug 脱敏；数据库只存 SHA-256 和短前缀。列表不返回 digest，撤销 SQL 同时约束 user_id。
- `Policy` 由 `AuthServiceProvider` 在 `Db` 已注册时放入 `AppState`，并通过追加的 `AuthPolicyMigration` 建立全局 roles/permissions/user_roles/role_permissions。支持确保角色、角色分配/撤销、精确权限授予/撤销与显式 `allows` / `authorize`。权限检查每次读取共享数据库，不缓存；JWT/Session 需要角色授予，PAT 还需同名 token ability。授权管理路由由应用负责保护。
- `QueryRule<E>` / `QueryScope<E>` 按 SeaORM Entity 定义资源行范围；`QueryScope` 必须从非空表达式起步，可通过 AND 与非空 OR 组扩展。`AuthorizedSelect` 只提供 AND 筛选、排序、limit/offset、列表/详情和分页；页大小最多 500，溢出偏移拒绝。批量 update/delete 在真实写语句上组合 rule AND user filter。`related` 会先按父规则校验父记录，再按子实体规则查询关联记录。F6-A 新增 `TenantQueryRule<E>`，由 `QueryRule::for_tenant` 绑定应用租户列；它在 select/related/update/delete 上将当前可信租户与原用户 scope 继续 AND，缺少 TenantContext 时拒绝。此能力不代替 `Policy::authorize`，也不拦截裸 SeaORM、原生 SQL 或自定义 driver。真实 PostgreSQL 18/MySQL 8.4 HTTP 集成覆盖 owner filter、越权详情、混合批量更新/删除和受限关联。
- `Policy::grant_permission_in_transaction` 接受调用方事务，用于将权限变更与 `openrust::audit::DatabaseAuditSink::record_in_transaction` 原子提交；常规 `grant_permission` 保持内部事务兼容。
- `AuthMigration` 创建中性逻辑名 `users`、`sessions` 与 `personal_access_tokens`，实际物理名由 历史决策 016 的 `DatabaseTableNames` resolver 解析（默认 `tb_*`）。PostgreSQL/MySQL 集成验证默认、空前缀、自定义前缀和 `users` 完整名映射，并走通认证持久化查询。
- `AuthRefreshTokenMigration` 作为追加迁移创建逻辑名 `auth_refresh_tokens`，不修改原认证迁移版本。`AuthServiceProvider` 在先注册 `DatabaseServiceProvider` 时向应用状态注册 `RefreshTokens`；它签发 32 字节随机 opaque token、只持久化 SHA-256、单次事务轮换、重放撤销整组，并提供单 token 与账户级撤销。账户行锁将签发、轮换和撤销串行化，避免并发轮换在账户级撤销后留下后继 token。初次过期时间由 AuthFlows 默认 30 天或显式配置提供，轮换不延长。
- `openrust new --db postgres|mysql --auth` 生成 JWT/Auth/AuthFlows 插件、Auth / refresh / verification / Policy migrations、`DatabaseRateLimitMigration`、JWT 默认保护的 `/api/me`、数据库 PAT 保护的 `/api/me/pat`、注册/验证/登录/refresh/logout/密码找回 HTTP routes 和 `[auth.jwt]` 配置区；密钥必须通过 `APP_AUTH__JWT__KEYS__LOCAL` 设置，不内嵌示例密钥。流程端点使用 `ClientIp` extractor；`[server].trusted_proxy_ranges` 默认为空，仅配置的代理 peer 可提供可信 XFF 链。认证代码调用 `Mailer`，由应用配置 SMTP；默认 `log` driver 只确认接收，不暴露验证码，也不代表邮件已送达。
- 认证生成物附带 `tests/auth_flows.rs`：使用 `TestDatabase` 创建独立数据库、替换 `Mailer` 为 fake，并通过真实 TCP listener 验证生成路由。覆盖统一响应、`Inject` 与 `ClientIp` 共存、可信 XFF、验证码、JWT 当前用户、默认数据库 PAT 认证、登录限流错误、refresh 单次轮换/重放拒绝、密码重置及登出撤销。由于默认 `HttpStatusMode::AlwaysOk`，错误的稳定 `code` 在统一 JSON envelope 中表达，HTTP status 保持 200。运行数据库 e2e 需设置 `OPENRUST_TEST_DATABASE_URL` 并授予测试账号创建/删除数据库权限；未设置时测试会提示并跳过。

## 外部身份（F7 OIDC 子簇）

- 可选 `auth-oidc` 提供 OAuth2 authorization-code + S256 PKCE relying-party、OIDC Discovery/JWKS 与 ID Token 验证。使用静态 issuer/client/redirect/scope，并通过受限命名 `DependencyClient` 访问 IdP；正常地址只允许 HTTPS，loopback 开发地址可使用 HTTP。`openidconnect` 未采用，具体依赖边界见 历史决策 053。
- `ExternalIdentityFlows` 将短时 state/nonce/verifier 加密保存在数据库，以单次事务消费；额外校验十分钟 HttpOnly/Secure/SameSite=Lax 浏览器绑定 Cookie。identity key 为 provider + issuer + subject，不按邮箱合并；新账户要求已验证邮箱，绑定/解绑要求本地密码重新认证并写入审计。需要 `AuthExternalIdentityMigration` 和应用侧身份创建策略。
- Discovery、token、JWKS 请求共享一次总时限与 1 MiB 响应体上限。JWKS 最长缓存 5 分钟，过期刷新失败时拒绝认证；未知 `kid` 主动刷新受 30 秒冷却限制。短暂 IdP 故障在缓存过期后会使 OIDC 登录失败关闭。
- PostgreSQL 18 和 MySQL 8.4 各完成一次数据库集成；Keycloak 26.8.1 fixture 完成真实浏览器表单授权码、PKCE、issuer、nonce、RS256/JWKS 验证。可复跑命令和跳过边界见 [测试基线](../memory/testing.md)。TOTP 与恢复码已按 历史决策 054 完成并单独通过 PostgreSQL 18/MySQL 8.4 验收；双应用 SSO/统一退出已按 历史决策 055 完成双库真实验收。

## 双应用 SSO 与统一退出（F7 验收完成）

- `complete` 保持兼容；`complete_with_handle` 返回 `ExternalLoginResult` 与不透明 `ExternalLoginHandle`。应用应仅在服务端安全保存句柄。调用 `attach_session` 会校验 Session 内账户 ID，并使用本地 `Encryptor` 加密保存 Session ID；启用 MFA 时，`AuthFlows::complete_mfa_login` 在完成 challenge 的事务中将新 refresh family 绑定回关联。
- `AuthExternalSessionMigration` 创建关联、Logout Token replay、一次性 RP logout state 与并发墓碑表。关联按 provider/issuer、subject hash、可选 `sid`、refresh family、MFA challenge hash 和加密 Session ID 保存，不保留原始 OIDC token。callback 在签发前检查 sid 与主体级退出墓碑；Back-Channel Logout 验证/重放检查后共享撤销 refresh、MFA challenge 和关联 Session。重复通知会继续尝试删除此前未能删除的 Session。
- `begin_logout` 先提交本地关联撤销，再按静态 provider endpoint/client/allowlist 回调 URI生成 RP-Initiated Logout URL；`complete_logout` 一次性消费 state。Session store 删除发生在数据库事务外，失败不会恢复已撤销凭据；重复提交同一关联句柄可补做 RP Session 删除，重放 Back-Channel Logout 可补做通知侧清理。
- callback 成功后释放未用于撤销的临时 scope 行；过期退出墓碑按索引每次最多清理 100 行，避免长期累积和单次清理造成无界事务。
- stateless JWT 仍有效至其签名 TTL 到期；本地退出/IdP 通知不提供即时 access-token 撤销。默认 access TTL 为 900 秒且配置上限 3600 秒；应用若覆盖默认值，应以其配置为实际残留上界。
- PostgreSQL 18 与 MySQL 8.4 的 Keycloak/Redis 真实验收均使用两个隔离应用、每应用两个独立 API 子进程及 TCP round-robin，覆盖双 client SSO、应用 callback/RP logout、跨节点 MFA completion、logout 期间 pending challenge 撤销后拒绝 completion、挂起 callback 在 logout 后拒绝、refresh/logout 并发、退出后两个节点拒绝共享 Session/refresh、节点重启重放与 15 秒 JWT 到期窗口。provider fake 双库测试另覆盖 RP state、审计、关联撤销与旧 callback；Redis store 上的一次删除错误注入后相同通知可重试清理。真实 Redis 断连期间返回 503，恢复后重启双应用四个 API 子进程并以相同 Logout Token 重放成功，PostgreSQL 18/MySQL 8.4 均通过；F7 仍等待本簇完整门禁，详见 历史决策 055 与 [测试基线](../memory/testing.md)。

## 边界与未纳入范围

- AuthFlows service contract 已记录在 历史决策 024 并已实现；验证码与 RateLimiter 数据库实现分别见 历史决策 026/025，Policy contract 和基础实现见 历史决策 027。验证码消费和账户变更不是同一事务；密码更新与 refresh family revoke 是单一事务。
- 生成项目 HTTP handlers 的真实 HTTP/fake-mail/`ClientIp` 集成覆盖已加入。2026-10-26 按当前模板新生成的 PostgreSQL 18 与 MySQL 8.4 项目均通过格式检查、`cargo check` 和完整 JWT/PAT HTTP 流程；Session 登录/退出及 CSRF 流程也在双库隔离集成测试覆盖。
- 注册、登录、找回、邮箱验证、refresh/logout 与共享限流已有 service 编排和生成 HTTP 路由模板；可信代理来源策略按 历史决策 028 实现。
- Policy/basic RBAC 已实现；复杂业务策略、租户范围与可缓存授权模型未纳入本 contract。
- 同一 workspace 内的 `tests/server/` 的认证集成测试构建各自测试 Application；默认 `Server::build` 没有安装 Auth/JWT/Database 插件，启用 `auth` feature 不会自动安装它们。完整认证业务 HTTP 流程由 CLI 生成项目通过真实 TCP 测试覆盖。
- 按 历史决策 041，`AuthFlows` 提供 direct `RegistrationValidating`、`LoginValidating`、泛化 `LoginFailed`，以及事务内 `RegistrationSucceeded`、`LoginSucceeded`。拒绝校验或成功监听器错误会回滚对应账户/refresh 写入；注册验证码在账户提交后单独签发，Session 登录不声明 SessionStore 原子提交。规范化邮箱只供验证策略使用，监听器不得记录；事件不包含任何认证秘密。
- AuthFlows 敏感接口限流及可配置的通用 HTTP `RateLimitLayer` 已实现，路由/key/策略边界见 [HTTP 事实](http.md)；`generate mailer` / `generate policy` 的双库生成验收见 [测试基线](../memory/testing.md)。这些交付不代表公开业务钩子或总体资源限制已完成。
- 密码服务使用受参数限制的 Argon2id 与 `spawn_blocking`，当前服务本身没有独立的总体并发许可或排队容量；生产资源控制按 补强计划 补齐，不能由单次参数上限推定全进程内存有界。

- Twilio 适配器有请求协议、认证、错误脱敏、timeout 与无重试测试，尚无已授权账号和目标号码的真实送达证据。认证和通用安全本地实现及双库验收不依赖该外部条件。

## 验证

- `cargo test -p openrust --test http_router --test http_resource`：全局 Tower layer 与错误响应包装测试通过。
- `cargo test -p openrust --features auth --test auth_runtime`：共享 Session middleware 与身份认证运行时测试通过；`session-redis` feature 及 Redis adapter 证据见 `memory/testing.md`。
- PostgreSQL 18 / MySQL 8.4：`cargo test -p openrust --features 'auth db-<backend>' --test auth_query_authorization` 使用隔离库和真实认证 HTTP 请求，验证过滤 OR 不扩大 owner 范围、越权详情不可见、分页计数、批量 update/delete 与父子关联规则；独立 consumer `tests/consumers/query-authorization` 通过 `cargo check --manifest-path ...`。
- `cargo test -p openrust --features auth,db-postgres --test auth_database` / `--features db-mysql --test database`：真实本地容器验证迁移、CSRF、PAT 签发/哈希存储/列举/过期输入/归属隔离/跨实例撤销与最近使用时间、通用 Session store 和两个应用实例间撤销；MySQL 使用具备建库权限的测试账号。
- `cargo test -p openrust --features auth --lib`：JWT 配置、签发/验证、错误边界及密钥轮换测试通过。
- `cargo test -p openrust --features auth --lib`：验证码用途/身份绑定、错误尝试耗尽、一次消费和策略边界单测通过；`session_login_rotates_id_preserves_app_data_and_redacts_csrf` 验证 ID 轮换、应用数据保留、CSRF digest 与 Debug 脱敏。
- `OPENRUST_TEST_DATABASE_URL=... cargo test -p openrust --features auth,db-postgres --test auth_database` 和 MySQL 对应命令：验证 refresh 哈希存储、旋转与不延长有效期、重放撤销、账户撤销、两请求并发刷新与并发账户撤销；验证 `AuthServiceProvider` 注册的应用级 `RefreshTokens`。
- 历史 PostgreSQL 14 / MySQL 8.0 真实隔离库 `database` 集成测试覆盖 verification migration 与 table-name resolver、Argon2id 摘要存储、resend cooldown、purpose/identity 绑定、尝试耗尽、过期失效，以及两个独立 App/AuthServiceProvider 实例间恰好一次并发消费。
- `cargo test -p openrust --features auth --test auth`：facade JWT Bearer 路由在不注册 SessionServiceProvider 时认证成功，并与 `Inject<T>` 共存。
- `cargo test -p openrust-cli`：生成模板断言 JWT/AuthFlows plugins、认证路由、`ClientIp`、根 `config/` 与外置密钥变量。
- `OPENRUST_TEST_DATABASE_URL=... cargo test -p openrust --features auth,db-postgres|auth,db-mysql --test auth_database`：真实隔离库认证流程新增 Session 登录/登出、CSRF、旧 ID 与并发旧请求失效及保留应用数据断言；2026-10-26 在 PostgreSQL 16 与 MySQL 8.4 上真实运行通过。
- `OPENRUST_TEST_DATABASE_URL=mysql://... cargo test --test auth_flows`：此前生成项目端到端流程在 MySQL 8.4.11 通过，覆盖验证码、限流 envelope、JWT、refresh 重放、密码重置和登出；PostgreSQL 最新生成项目复验待本机数据库恢复。
- 2026-10-26 当前基线复核：PostgreSQL 18 与 MySQL 8.4 的 `auth_database` 各 6 项、`auth_runtime` 各 6 项通过；当前生成模板项目在两库分别通过 `auth_flows` 真实 TCP HTTP 测试。覆盖注册/重复注册、邮箱验证、登录、Session 轮换/CSRF/退出、JWT 与 PAT、refresh 轮换/重放、密码重置、共享限流、Policy/RBAC 与 PAT ability 交集、可信代理和统一错误 envelope。
- 2026-10-26：默认及 all-features workspace fmt/clippy/test/doc 全通过；认证真实隔离库流程测试在 PostgreSQL 16 与 MySQL 8.4 通过；新生成 PostgreSQL、MySQL 项目分别运行 `cargo fmt --all --check && cargo check && cargo test` 通过。MySQL 模板构建提示有数个未启用可选 crate patch，不影响已启用图谱或测试结果。
- 2026-10-26：`policy_grants_and_revocations_are_visible_to_authenticated_routes` 在 PostgreSQL 16 与 MySQL 8.4 真实隔离库通过，验证 JWT 路由默认拒绝、授予/撤销即时生效、PAT ability 与数据库授权取交集，并覆盖自定义用户表映射。
