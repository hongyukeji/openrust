# `openrust::auth`

## MFA、2FA 与用户选择

多因素认证（MFA）是总体安全能力名称；双因素认证（2FA）是其中使用两类因素的形式，TOTP 是验证码验证技术。本批对应 Laravel Fortify 的 Two-Factor Authentication，支持密码与 TOTP／恢复流程，不据此宣称支持其他尚未实现的认证因子；官方 API、配置及来源名称保持不变。

用户按需绑定和开启。当前应用 `confirm=true`：未绑定可正常密码登录，待确认不当作已开启，验证确认后登录执行双因素挑战；完成必要身份确认后可关闭。应用开放绑定功能不等于所有账户已开启。其他应用显式选用 `confirm=false` 时仍遵循固定官方语义，不将本应用策略强改为框架默认。

删除旧 MFA 仅指旧 OpenRust 独立实现及旧数据兼容，保留 MFA 概念、TOTP 与正常安全能力；不恢复 `auth-mfa` feature 承载术语。


R7具名认证核心已由 `5ad34b9` 限定验收提交；官方Web默认与显式JWT组合另由 `54f4a087` 交付，证据见[组合验收](../memory/testing.md#fortify-官方默认与显式-jwt-组合验收)。两者均不关闭完整Auth、Eloquent、Fortify或Google真实客户端验收。

> 受众：AI Agent
> 更新：2026-11-06
> 摘要：具名认证、唯一Fortify二因素和显式JWT API的当前限定事实；自建授权服务器、完整官方对应及外部客户端缺口仍保留。

## 当前单体系整改

旧独立 `MfaService`、`auth-mfa` feature、旧因子表/挑战及数据接管兼容已在 `54f4a087` 删除并完成本簇限定验收。官方Web使用同一Guard/Session/Fortify三字段与Actions；显式JWT API共用这些机制，以有用途限制的短挑战和管理授权提供无Session/Cookie流程。框架默认不改为JWT，不清空共享数据库；其他旧兼容整改仍见[唯一待办](../todo.md#其他旧兼容整改已授权当前fortify优先)。

## 当前实现

`54f4a087` 的受管凭据组合已经限定验收：`CredentialAuthority` 串联实际Session、JWT、PAT、refresh及直接/OIDC挑战；关联与签发在最后异步边界核当前账户、父凭据、期限和撤销状态。真实Redis父Session派生JWT/PAT及父记录删除回滚、真实Keycloak双RP/独立API/故障恢复与适用生成/consumer已收齐。Redis与数据库之间不承诺分布式原子提交；完整Fortify、Google真实客户端和UUID/字符串JWT持久链仍未完成。具体输入复用及服务边界见[组合验收](../memory/testing.md#fortify-官方默认与显式-jwt-组合验收)。

`TwoFactorAuthenticationProvider::verify_with_context` 返回同次 `TwoFactorCodeVerification`；内置Google Provider读取同一ConfigRepository的动态window，消费入口使用实际验证事实、代码绑定和期限。当前只保留唯一现代Fortify状态与公开扩展，不提供旧seed/hash/floor资产接管、映射或重绑兼容。完整动态选项、任意窗口变化及真实Google客户端边界不得由单个接口存在推定关闭。

✅ R7具名认证限定基线（5ad34b9）：AuthServiceProvider注册AuthDrivers与scoped AuthManager，Session/Web请求共享真实上下文；Guard/Provider按配置和请求缓存、CurrentUser保留数字/字符串ID及来源。DatabaseUserProvider走同一Db查询/工厂，根app-users适配password_hash并拒绝disabled_at，真实Eloquent仍未实现。StatefulGuard提供Session/remember/once/事件语义；GenericUser原子CAS用于Rust取消恢复，未实现能力前置拒绝，既有AuthFlows/OIDC桥接核同一数据库句柄和账户表，不按相同整数ID跨域。唯一模块账户/浏览器实时/OIDC Session调用方、CLI生成模型/配置/测试与正式指南已编码；主11门禁、当前默认workspace、7consumer、四fresh、真实双库各33及最低3.26迁移30断言已通过；最终证据独审与提交状态见唯一todo。详见唯一todo与Fortify计划的R7前置边界。

- 认证配置经同一配置仓储读取；JWT密钥使用受保护的 `.env` 或部署环境，不写入生成源码。当前公开路径以独立框架包及正在进行的命名空间迁移为准，不把旧配置re-export作为兼容使用指引。

- `AuthServiceProvider::new()` 注册共享密码服务与数据库认证器的同容器异步工厂；boot 解析当前最终绑定的认证服务，并检查当前已支持的 PostgreSQL、MySQL 或 SQLite 后端。`AuthServiceProvider::with_authenticator` 使用应用或第三方实现，不依赖 `Db`；后序 Provider 可按容器覆盖契约提供完整自定义服务。
- auth::Session重导唯一Session实现。默认database使用原始ID、Base64属性载荷与Laravel handler exists语义；身份摘要envelope、绝对截止、防删除后迟到写入复活属于显式database-guarded增强。历史验收按原输入范围保留，不作为当前R7已验收证明。
- C04-C已验证的Cookie外层继续给Session提供明文视图，保原始wire供认证严格重复检查；其旧双库各48项记录不自动沿用为D1通过。当前配置改为lifetime=120分钟、secure=None按可信传输推导、same_site可空且默认lax，Cookie名按最终app.name推导或显式配置。最终配置延迟装配与overlay沿历史决策 082，完整Session/Web范围仍开放。
- 浏览器实时连接使用 `SessionStoreHandle::prepare_live_guard` 获取 `LiveSessionCredential`，并在实际发送/输入/心跳前通过独立reader重核真实Session、身份、因子与期限。原生数据库删除按下一次读取拒绝，Cookie快照不具服务器端独立重验能力；读取与网络发送之间不承诺原子撤销。
- 标准Web/CSRF组要求Session ID与XSRF Cookie受保护；plain或排除这些名称会在构建Router时报web_cookie_encryption_missing。未经过该组且实际使用Cookie Session认证，或运行时更换handler/name导致保护不足，才返回session_cookie_protection_required；仅排除合法数据Cookie名由Cookie认证入口拒绝。JWT独立路径不因编译Cookie driver误拒。
- `Authenticator` 接收路由选中的 `AuthCredential`，返回用户 ID 与 PAT abilities；凭据字段不实现 `Debug` / 序列化。默认数据库实现验证共享 Session 身份和 CSRF，或验证 PAT 到期、撤销、用户禁用并更新 `last_used_at`。
- `JwtConfig`、`JwtServiceProvider` 与 `JwtTokens` 提供HS256、issuer/audience、kid轮换及时间边界的密码学处理。受管资源认证必须通过 `FortifyJwtApi::authenticate`、`CredentialAuthority::authenticate_access_token` 或对应Guard重核当前持久授权；同步签名验证不等于完成因子与撤销检查。JWT API持久adapter当前仍限定正整数 `users.id`。
- `AuthLayer::required` / `optional` 通过 Axum `FromFnLayer` 接入现有 `Routes::layer`。路由模式包含 JWT、Session、PAT、Session-or-PAT；组合模式中的 Bearer 明确解释为 PAT。
- `AuthServiceProvider` 依赖同容器 `HashServiceProvider`；认证与验证码默认注入动态 `HashManager`，公开 `dyn Hasher` 则保持首次解析的 driver 身份。唯一成本权威为 `config/hashing.rs` / `HashingConfig`，默认 bcrypt12，显式 Argon 默认65536KiB/4/1。独立 `Passwords`、`PasswordConfig` 与 `auth.password` 已移除。标准 UserProvider 保留官方 make/check 与 CAS 重哈希；增强 `EncodedPasswordHash` 不实现 Debug，额外校验非空、1024字节及 bcrypt72字节/NUL别名，严格Argon混合存储失败经当前驱动 dummy 计算归一。驱动计算共享每管理器默认2个、可配置1..=4个许可，满载503无排队，取消不提前释放；计算参数另受 ExecutionLimits 限制。新接线的固定来源、真实双库/生成与输入边界限定证据见[统一Hashing验收](../memory/testing.md#统一-hashing-与认证接线限定验收)，不继承旧独立密码服务证据。
- `VerificationCodes` 通过 `AuthServiceProvider` + `DatabaseServiceProvider` 注册，使用统一 `HashManager` 当前驱动的 code hash 和 `DatabaseVerificationCodeStore`。purpose 与调用方规范化后的 identity 绑定，默认 6 位数字、10 分钟有效、60 秒重发间隔、最多 5 次错误；每 purpose/identity 同时只有一个 challenge。一次消费、尝试计数、重发冷却与有效期均在数据库共享，验证状态未知或失效统一返回 false；持久表由 `AuthVerificationCodeMigration` 显式创建。Mail/SMS 不由该服务发送。
- `AuthServiceProvider` 同时注册 `VerificationCodes`；`AuthVerificationCodeMigration` 创建 resolver-aware `verification_codes` 表。生成 `--auth` 项目注册 Auth / refresh / verification / Policy 与 database rate-limit migrations；通用限流已由 AuthFlows 在敏感流程中组合使用。
- `AuthFlowsServiceProvider` 在 `DatabaseServiceProvider`、`AuthServiceProvider`、`JwtServiceProvider` 之后注册统一流程服务；缺少任一依赖或 refresh lifetime 越界时构建失败。服务提供注册/邮箱验证/登录/refresh/登出/密码重置编排，按 endpoint + identity 摘要 + 调用方可信 request-source 消费共享 `RateLimiter`。注册和找回返回 redacted code wrapper，不发送邮件/SMS；默认 30 天 refresh family、5 次/15 分钟固定窗口。密码重置与 refresh family 撤销在同一数据库事务提交。
- 注册仅将邮箱唯一约束冲突映射为 `None`；CHECK、截断及其他 SQL 错误继续传播，避免 MySQL `INSERT IGNORE` 将非重复错误伪装成重复账户。
- 唯一示例的账户Session流程通过具名Guard与同一真实账户Provider建立身份，使用标准Web/CSRF保护；需要派生JWT/PAT时显式走受管凭据桥接，重核父Session及当前因子状态。不把独立用户ID键或CSRF摘要路径作为新的接入方式。
- `auth-fortify` 启用唯一Fortify/TOTP能力，依赖同一Guard/UserProvider、加密与应用迁移清单。根应用默认Web管线；显式JWT profile经正式Provider选择。旧`MfaServiceProvider`和`AuthMfaMigration`已移除，不存在并行旧因子表或自动接管数据。TOTP seed继续由本地Encryptor加密，算法、重放与因子变更按同一Fortify状态/authority核验。
- `AuthFlows::begin_login` 在配置了唯一Fortify因子authority且当前账户需要挑战时返回 `LoginAttempt::TwoFactorRequired`；直接API与OIDC来源共用 `AuthFlows::complete_two_factor_login` 完成一次挑战，再签发受管access/refresh。挑战按实际账户、代际、Guard/Provider、用途与来源绑定，有限期、限次且一次消费；未绑定账户不被强制开启TOTP。
- Fortify管理复用 `TwoFactorActions`：按当前confirm策略准备并确认绑定，恢复码为八份加密可重新读取的值，成功消费后替换该份；关闭清除secret、recovery_codes及confirmed_at。近期密码确认、管理授权和受管凭据失效由对应Web或显式JWT流程执行，不能把一次管理成功响应当资源授权升级。
- `CurrentUser` / `OptionalUser` 从请求扩展读取身份，可与 `Inject<T>` 同一 handler 使用。PAT abilities 精确匹配。
- 认证主体留在当前 HTTP request extension；`ExecutionMetadata` 只跨任务传递 request ID/locale，不序列化 `CurrentUser`、权限快照、Bearer token、Session cookie 或任意 header。后台 Job 的授权须在执行边界按应用策略重新确认。
- `PersonalAccessTokens` 由 `AuthServiceProvider` 在数据库已注册后放入 `AppState`，提供一次性签发、账户范围列举、单 token 撤销及账户级全量撤销。原始 token 以 256-bit CSPRNG 生成，仅 `IssuedPersonalAccessToken::expose` 可读，Debug 脱敏；数据库只存 SHA-256 和短前缀。列表不返回 digest，撤销 SQL 同时约束 user_id。
- `Policy` 由 `AuthServiceProvider` 在 `Db` 已注册时放入 `AppState`，并通过追加的 `AuthPolicyMigration` 建立全局 roles/permissions/user_roles/role_permissions。支持确保角色、角色分配/撤销、精确权限授予/撤销与显式 `allows` / `authorize`。权限检查每次读取共享数据库，不缓存；JWT/Session 需要角色授予，PAT 还需同名 token ability。授权管理路由由应用负责保护。
- `QueryRule<E>` / `QueryScope<E>` 按 SeaORM Entity 定义资源行范围；`QueryScope` 必须从非空表达式起步，可通过 AND 与非空 OR 组扩展。`AuthorizedSelect` 只提供 AND 筛选、排序、limit/offset、列表/详情和分页；页大小最多 500，溢出偏移拒绝。批量 update/delete 在真实写语句上组合 rule AND user filter。`related` 会先按父规则校验父记录，再按子实体规则查询关联记录。F6-A 新增 `TenantQueryRule<E>`，由 `QueryRule::for_tenant` 绑定应用租户列；它在 select/related/update/delete 上将当前可信租户与原用户 scope 继续 AND，缺少 TenantContext 时拒绝。此能力不代替 `Policy::authorize`，也不拦截裸 SeaORM、原生 SQL 或自定义 driver。真实 PostgreSQL 18/MySQL 8.4 HTTP 集成覆盖 owner filter、越权详情、混合批量更新/删除和受限关联。
- `Policy::grant_permission_in_transaction` 接受调用方事务，用于将权限变更与 `openrust::audit::DatabaseAuditSink::record_in_transaction` 原子提交；常规 `grant_permission` 保持内部事务兼容。
- `AuthMigration` 创建中性逻辑名 `users`、`sessions` 与 `personal_access_tokens`，实际物理名由 历史决策 016 的 `DatabaseTableNames` resolver 解析（默认 `tb_*`）。PostgreSQL/MySQL 集成验证默认、空前缀、自定义前缀和 `users` 完整名映射，并走通认证持久化查询。
- `AuthRefreshTokenMigration` 作为追加迁移创建逻辑名 `auth_refresh_tokens`，不修改原认证迁移版本。`AuthServiceProvider` 在先注册 `DatabaseServiceProvider` 时向应用状态注册 `RefreshTokens`；它签发 32 字节随机 opaque token、只持久化 SHA-256、单次事务轮换、重放撤销整组，并提供单 token 与账户级撤销。账户行锁将签发、轮换和撤销串行化，避免并发轮换在账户级撤销后留下后继 token。初次过期时间由 AuthFlows 默认 30 天或显式配置提供，轮换不延长。
- `openrust new --auth` 与 `--fortify` 默认生成官方Web/Session模式；`--profile jwt-api` 才生成无Session/Cookie的JWT API、access/refresh、同一Fortify挑战及管理流程。两种profile共用Provider/Guard/Actions，显式模式替换Web Provider，不并挂两套认证入口。配置与调用示例见[认证指南](../../../docs/src/security/authentication.md)。
- 当前生成的Web SQLite/MySQL、JWT SQLite及JWT+gRPC MySQL已按绑定输入完成new/fmt/check/test；普通默认生成亦有独立验收。HTTP状态、裸JSON或应用envelope由对应模式和公开响应扩展决定，不再把AlwaysOk/HTTP200作为全局认证默认。真实生成与ignored边界见[组合验收](../memory/testing.md#fortify-官方默认与显式-jwt-组合验收)。

## 外部身份（F7 OIDC 子簇）

- 可选 `auth-oidc` 提供 OAuth2 authorization-code + S256 PKCE relying-party、OIDC Discovery/JWKS 与 ID Token 验证。使用静态 issuer/client/redirect/scope，并通过受限命名 `DependencyClient` 访问 IdP；正常地址只允许 HTTPS，loopback 开发地址可使用 HTTP。`openidconnect` 未采用，具体依赖边界见 历史决策 053。
- `ExternalIdentityFlows` 将短时 state/nonce/verifier 加密保存在数据库，以单次事务消费；额外校验十分钟 HttpOnly/Secure/SameSite=Lax 浏览器绑定 Cookie。identity key 为 provider + issuer + subject，不按邮箱合并；新账户要求已验证邮箱，绑定/解绑要求本地密码重新认证并写入审计。需要 `AuthExternalIdentityMigration` 和应用侧身份创建策略。
- Discovery、token、JWKS 请求共享一次总时限与 1 MiB 响应体上限。JWKS 最长缓存 5 分钟，过期刷新失败时拒绝认证；未知 `kid` 主动刷新受 30 秒冷却限制。短暂 IdP 故障在缓存过期后会使 OIDC 登录失败关闭。
- OIDC RP的Discovery/JWKS、签名、state/nonce与S256 PKCE有真实Keycloak及双库证据。当前唯一Fortify二因素与受管凭据接线的限定交付见[组合验收](../memory/testing.md#fortify-官方默认与显式-jwt-组合验收)；旧独立MFA验收只保留历史输入事实，不作为当前实现说明或自建授权服务器完成证明。

## 外部 IdP 双 RP 与统一退出（限定验收）

- `complete_with_handle` 返回 `ExternalLoginResult` 与不透明 `ExternalLoginHandle`，应用仅在服务端保存。`attach_session` / `attach_session_with_guard` 与refresh关联按实际账户和父凭据核验；完成当前Fortify挑战使用 `AuthFlows::complete_two_factor_login`，不再调用旧独立MFA接口。
- `AuthExternalSessionMigration` 创建关联、Logout Token replay、一次性 RP logout state 与并发墓碑表。关联按 provider/issuer、subject hash、可选 `sid`、refresh family、MFA challenge hash 和加密 Session ID 保存，不保留原始 OIDC token。callback 在签发前检查 sid 与主体级退出墓碑；Back-Channel Logout 验证/重放检查后共享撤销 refresh、MFA challenge 和关联 Session。重复通知会继续尝试删除此前未能删除的 Session。
- `begin_logout` 先提交本地关联撤销，再按静态 provider endpoint/client/allowlist 回调 URI生成 RP-Initiated Logout URL；`complete_logout` 一次性消费 state。Session store 删除发生在数据库事务外，失败不会恢复已撤销凭据；重复提交同一关联句柄可补做 RP Session 删除，重放 Back-Channel Logout 可补做通知侧清理。
- callback 成功后释放未用于撤销的临时 scope 行；过期退出墓碑按索引每次最多清理 100 行，避免长期累积和单次清理造成无界事务。
- 原始无状态JWT只具签名与TTL边界；受管JWT资源入口还须查询当前家族、因子与撤销状态。不能用原始同步验证绕过受管入口，也不承诺已经发生的远端副作用或发送可随撤销原子回收。
- Keycloak26.8.1作为外部IdP的双RP、独立API、签名backchannel及Redis故障恢复已纳入 `54f4a087` 限定验收：拒绝失效父Session/家族与重放，恢复后仍按原凭据重新核验。它证明现有RP调用链，不代表自建Passport授权服务器、OIDC Provider或完整SSO产品已经实现。具体真实命令、复用输入与残余边界见[组合验收](../memory/testing.md#fortify-官方默认与显式-jwt-组合验收)。

## 边界与未纳入范围

- AuthFlows service contract 已记录在 历史决策 024 并已实现；验证码与 RateLimiter 数据库实现分别见 历史决策 026/025，Policy contract 和基础实现见 历史决策 027。验证码消费和账户变更不是同一事务；密码更新与 refresh family revoke 是单一事务。
- 生成项目 HTTP handlers 的真实 HTTP/fake-mail/`ClientIp` 集成覆盖已加入。2026-10-26 按当前模板新生成的 PostgreSQL 18 与 MySQL 8.4 项目均通过格式检查、`cargo check` 和完整 JWT/PAT HTTP 流程；Session 登录/退出及 CSRF 流程也在双库隔离集成测试覆盖。
- 注册、登录、找回、邮箱验证、refresh/logout 与共享限流已有 service 编排和生成 HTTP 路由模板；可信代理来源策略按 历史决策 028 实现。
- Policy/basic RBAC 已实现；复杂业务策略、租户范围与可缓存授权模型未纳入本 contract。
- 同一 workspace 内的 `tests/server/` 的认证集成测试构建各自测试 Application；默认 `Server::build` 没有安装 Auth/JWT/Database 插件，启用 `auth` feature 不会自动安装它们。完整认证业务 HTTP 流程由 CLI 生成项目通过真实 TCP 测试覆盖。
- 按 历史决策 041，`AuthFlows` 提供 direct `RegistrationValidating`、`LoginValidating`、泛化 `LoginFailed`，以及事务内 `RegistrationSucceeded`、`LoginSucceeded`。拒绝校验或成功监听器错误会回滚对应账户/refresh 写入；注册验证码在账户提交后单独签发，Session 登录不声明 SessionStore 原子提交。规范化邮箱只供验证策略使用，监听器不得记录；事件不包含任何认证秘密。
- AuthFlows 敏感接口限流及可配置的通用 HTTP `RateLimitLayer` 已实现，路由/key/策略边界见 [HTTP 事实](http.md)；`generate mailer` / `generate policy` 的双库生成验收见 [测试基线](../memory/testing.md)。这些交付不代表公开业务钩子或总体资源限制已完成。
- 内置哈希计算使用受参数限制的 `spawn_blocking` 和共享操作许可；独立构造多个管理器分别计量，单管理器边界不等于全进程内存有界。

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

显式 FortifyJwtApi 已绑定时，gRPC Bearer JWT 每次在独立 scope 中解析该服务并复用其完整认证，向业务传递所选 Guard / Provider；PAT 与未选 profile 的 JWT 沿既有机制。Redis 父 Session 独立读取以原子有界载荷/PTTL及记录截止为准，不续期、无TTL明确拒绝，计算截止保守向下取整秒且不足一秒拒绝；跨 Redis 与数据库不承诺分布式原子提交。当前批次最终验收与提交状态由唯一 todo 记录。
