# 认证与授权

本模块提供具名 Guard、账户 Provider、密码、JWT/PAT/refresh、授权查询与可选 Fortify。完整操作见[认证指南](../../../../../docs/src/security/authentication.md)。现代 Fortify 与 JWT API profile 为本批新增组合，尚待适用行为、生成应用和集成验收；接口存在不代表完整 Laravel 认证生态已交付。


## 具名 Guard 与 `UserProvider`

`AuthServiceProvider` 将 `AuthManager` 绑定为请求 scope；应用共享的 `AuthDrivers` 只保存工厂。同请求同名 Guard 复用实例和用户缓存；`should_use`、`forget_guards` 只影响本请求，不能将管理器或已登录 Guard 注册为 singleton。递归或并发等待成环返回 `auth_guard_resolution_cycle`，失败/取消不缓存半成品。

`auth.defaults`、`auth.guards`、`auth.providers` 来自同一配置仓库。默认 Guard 为 `web/session`；`create_user_provider(None)` 读取可空的 `auth.defaults.provider`，不从默认 Guard 反推。未定义 Guard、driver 或必须的账户来源是500配置错误；直接查询未知 Provider 名仍返回 `None`。框架 `eloquent` 声明不代表真实 Eloquent driver 已实现，未注册时明确失败。根应用和模板显式注册 `app-users`，通过 `DatabaseUserProvider::with_user_factory` 构造同一应用模型、解释 `password_hash`，不冒充 Eloquent。

`Authenticatable` / `AuthUser` 保留原始数字或字符串 ID，不要求邮箱、手机号或整数。`GenericUser` 默认字段为 `id/password/remember_token`，密码和 token 可以缺失；自定义字段由模型适配。`Credentials` 只应包含应用挑选的允许字段。数据库 Provider 排除名称含 `password` 的查询条件，再单独验证明文 `password`，不自动追加禁用账户、租户等业务条件。

`AuthDrivers::extend` / `provider` 是公开扩展入口。Guard 工厂按需调用 `GuardContext::user_provider()`，准确报告 `provider_name()`。`AuthLayer::guard(name)` / `optional_guard(name)` 将实际身份投影为 `CurrentUser`；`id()` 保留原类型，`integer_id()` 是正整数业务表的显式适配，不能截断 UUID 或转换字符串数字。业务授权还须绑定相同 Guard/Provider，不能借相同数字 ID 跨来源授权。

`StatefulGuard` 提供 `attempt`、`login`、`login_using_id`、`once`、`once_using_id`、`logout`。`validate`、`once`、`set_user` 不建立持久登录。Session 登录使用同一请求句柄并轮换 ID；Guard 注销移除自身身份并轮换适用 remember token，不自动 invalidate 整份 Session。remember 默认576000分钟，凭据失败时间保护200000微秒，`hashing.rehash_on_login=true`。

记住我 Cookie 必须加密且不在 except 名单。账户 `set_remember_token` 为异步属性赋值；Guard 通过 `begin_remember_token_update` 条件安装并取得拥有式 `RememberTokenUpdate`，成功后commit，失败或取消时同步恢复仍属于本次修改的原始字段。未实现返回 `auth_token_atomic_update_unsupported`，不回退为先读再写。签发Cookie只用 `remember_token_for_cookie` 原子确认的快照，不能读取另一Guard未持久化的候选。此内存恢复不承诺撤回已发出的数据库 I/O，也不能掩盖未知提交结果或跨连接更新逆序。Rust 名称后缀承担 Laravel 类名 SHA-1 的隔离职责，不承诺 PHP Cookie/Session 字节互通。

`UserProvider::uses_database_accounts(db, table)` 默认 false，内置数据库实现核同一 `Db` 句柄及实际表。`AuthFlows`、OIDC 与受管凭据适配借此核现有账户域，不能仅凭同 ID、表名字或另一个数据库快照授权。

## Session 与持续授权

`SessionServiceProvider::new()` 安装唯一通用 Session middleware，默认使用已有 `Db` 的原生 database driver；只有实际路由启用 `session` 或 `web` 组才启动会话。`with_store` 安装公开 `SessionStore`；array/Memory是显式选择，数据库故障不降级。Cookie配置来自 `config/session.rs`，密钥与部署值通过受保护环境来源提供。详见[Session指南](../session/README.md)。

原生数据库保存前将当前请求默认 Guard 的实际 ID 投影到 `sessions.user_id`；默认 bigint字段不能容纳任意字符串 ID，应用须提供相应存储。手动保存绑定同一请求 scope，不能借缓存身份填另一份记录。

`SessionStoreHandle::authorized_until_guard` 核实际 Guard、Session登录键与真实持久载荷，拒绝仅请求身份。`prepare_live_guard` 产生不可外部构造的 `LiveSessionCredential`，重验时从**实际 Session handler**的独立 reader 读取身份、期限和当前证明，不保留请求 Guard 缓存、不续期。原生 database 从同次行读取的 `last_activity` 与配置 lifetime 计算截止，实际 DELETE 后下一次读取会拒绝；Cookie快照没有独立服务器端读取能力。

原生 database 仍有 Laravel 的迟到 INSERT 回退，不能防止并发旧 writer 重建记录。`database-guarded` 及其显式迁移提供更强的防复活语义；强删除接口仍按保证等级拒绝，不把原生 reader 提升成 Guarded/CAS。实时连接 validator 收紧而不延长截止，重核当前家族/因子/资源权限，不持全局连接锁跨用户回调等待；撤销和网络发送不是原子事务。

## 密码、验证码与持久凭据

- `JwtServiceProvider` 与 `AuthServiceProvider` 使用统一容器生命周期，显式 key ring 在启动时校验；不要把秘密写入源码或日志。`JwtTokens` 的同步签名校验不等于当前主库授权。
- 密码、验证码与客户端秘密统一使用 `hashing::HashManager` 和 `config/hashing.rs`；默认 bcrypt12/verify=true，可显式选择 Argon2i/id。随机盐、blocking任务、默认2/最大4的共享容量与执行预算均由Hashing驱动持有；容量满返回503 `password_capacity_exhausted`，不排队。标准UserProvider按官方Hash语义验证/重哈希；增强AuthFlows额外拒绝bcrypt的72字节/NUL截断别名，且查账户前统一检查，完整字节输入可选择严格Argon。详见[哈希](../../../../../docs/src/security/hashing.md)。
- `VerificationCodes` 依赖同一数据库和密码器，默认六位、十分钟、五次错误、60秒重发冷却；`IssuedVerificationCode` 调试脱敏。应用负责Mail/SMS与公开一致响应。验证码服务不把消费和任意后续账户写入变成跨服务原子操作。
- `AuthFlowsServiceProvider` 组合注册、邮箱验证、登录、刷新、登出和密码重置，不自动定义业务路由或发消息。请求来源必须来自可信 `ClientIp`，不能信任任意转发头。邮箱流程仍有自己的已验证邮箱前置，通用模型可空邮箱不代表全部邮箱流程已改成用户名流程。
- `RefreshTokens` 只保存随机凭据摘要，轮换不延长家族原期限；重放撤销整个家族。PAT只返回一次高熵明文，数据库保存摘要/前缀，按账户列举/撤销并在认证时核撤销、期限、禁用状态和abilities。
- 选择 `CredentialAuthority` 后，JWT/refresh/PAT携带受管上下文，签发和使用时核当前来源、代际、真实owner/家族、父权限与期限。`CredentialFactorMigration` 增加上下文字段，NULL或坏上下文不会自动获得受管权限；有资产时down拒绝。`VerifiedCredential` 不能由应用布尔值或JSON构造。

## Fortify/TOTP 入口与边界

MFA 是多因素认证总称，2FA 是双因素形式，TOTP 是验证码技术。`auth-fortify` 使用同一账户的 `two_factor_secret`、`two_factor_recovery_codes`、`two_factor_confirmed_at`；前两项为密文。`TwoFactorAuthenticatable` 与 `DatabaseTwoFactorState` 支持模型原始ID和持久来源，`TwoFactorActions` 是唯一动作内核，不另建因子账户。

官方 Web 模式使用 `FortifyServiceProvider`、具名 Session Guard、Cookie/CSRF、响应契约和可选remember。`openrust new blog --fortify` 保留此默认，不是完整 `fortify:install`。`FortifyConfig::default()` 是包回退，`installed()` 是安装模板选择；根 `config/fortify.rs` 再显式覆写。

根应用选择 `confirm=true`：未绑定时密码正常登录；生成密钥后只是待确认，确认后后续登录才需要挑战。`confirmPassword` 独立决定Web管理路由近期密码确认。`confirm=false` 保持上游secret非null规则，不全局改默认。用户可按需绑定并在所选管理入口必要身份确认后关闭。

官方恢复码为8个可重新读取的加密 `10字符-10字符` 码；成功使用时替换该码，仍8项。密钥默认16个Base32字符，HMAC-SHA1、6位、30秒、window1；QR由同一secret本地生成，不向外部服务传密钥。真实 Google Authenticator扫描与跨窗口使用尚未验收。

`DatabaseFactorAuthority` 是显式增强：同账户事务核三字段、随机source、持久generation和短期代码摘要，成功恢复码原子替换，确认与代码消费同事务。`FactorAuthorityMigration` 只增加当前元数据，不要求旧因子数据兼容接管。`FactorReceipt` 只能由实际验证消费产生，Session证明绑定真实Guard/Provider/ID与代际；管理context、密码确认、JSON成功响应均不是因子证明。

`TwoFactorAuthenticationProvider::verify_with_context` 返回绑定代码摘要、实际窗口/周期/计数的 `TwoFactorCodeVerification`；默认布尔 `verify` 复用同次验证。Google provider 保持官方cache get/verify/put，验证在账户事务外。数据库nonce期限按实际事实 `(2*window+3)*period_seconds` 计算；它不是任意扩窗或时钟回退下的永久防重放保证。取消、事件与不同存储之间不宣称跨资源自动回滚。

### 显式无 Cookie JWT API profile

官方 Fortify 不内置无 Session 的 JWT 流程；Laravel 的 Passport/Sanctum 是其他可选能力。本模块的 `FortifyJwtApiServiceProvider` 是显式JWT扩展，复用相同Provider、回调、Actions和authority。生成命令为 `openrust new blog --profile jwt-api --db sqlite`；普通生成不隐式选择 JWT，`--auth` 与 `--fortify` 保持 Web选择。根应用启用 Fortify 时默认 Web，仅显式 `AUTH_PROFILE=jwt-api` 选择 JWT API。该profile当前仍待新行为与完整生成验收。

手工装配时用 API Provider 替换 Web Provider，配置 `auth.guards.api={driver:"jwt-api",provider:"users"}`、`fortify-api.guard="api"`、`fortify-api.prefix="/auth"`。默认API前缀下注册 `/api/auth/login`、`two-factor-challenge`、`refresh`、`logout`、`user` 和 `two-factor/*` 管理路由。请求只走api组，不挂Session/Cookie/CSRF/remember；业务API不能叠加web组。详见[请求字段与完整路由表](../../../../../docs/src/security/authentication.md#显式-jwt-api-profile)。

密码首因子通过后返回access/refresh，或不透明 `TwoFactorLoginChallenge`；`LoginAttempt::TwoFactorRequired` 不代表正式身份。完成TOTP/恢复码才签发所选Guard/Provider scope的当前家族。资源使用 `AuthLayer::guard("api")` 或 `FortifyJwtApi::authenticate` 重核主库，不能只验JWT签名。

管理先以当前Bearer+真实密码调用 `/two-factor/authorize` 取得加密 `management_token`，再用 `X-Fortify-Management`。授权至多300秒且不超过access/实际家族期限，不是资源凭据。启用/重建后可连续读取新QR/secret/恢复码；确认或关闭后重新登录，旧access不因管理成功自动升级。响应为裸JSON并设置no-store/no-cache，无临时SessionCookie。

当前JWT持久适配及API管理限真实正整数 `users.id`；通用Guard/模型支持UUID不意味着JWT完整闭环也支持。选中的认证中心与业务API整个流程均无Session/Cookie依赖；官方Web独立可选，不作API前置。中心SSO的token持有/回访与浏览器限制待后续授权服务器能力设计取证，不自建URL传token机制。

`ApiLoginAuditSink` 是显式通用事务扩展。`DatabaseApiLoginAudit` 严格同Db/实际 `audit_records` 表，使用传入凭据事务和通用脱敏策略；可选择API Provider的 `with_database_audit`。未绑定时不查审计表，错误回滚本次消费/签发，不记录秘密。

## 资源查询授权

`Policy::authorize` 判断操作权限；`QueryRule<E>` / `QueryScope` 定义资源行范围，用户筛选始终与范围相交。受保护列表、详情、分页、批量写入和关联读取不会向外暴露底层SeaORM builder。页大小上限500，偏移溢出拒绝；关联先核父记录再核子规则，软删除条件由应用显式组合。

普通SeaORM、原生SQL和自定义driver不会自动套范围。租户通过可信resolver和 `QueryRule::for_tenant` 绑定；未经验证的header/query不是租户身份。长任务使用 `AuthorizedSelect::into_batches` 保留范围，详见[查询consumer](../../../../../tests/consumers/query-authorization/README.md)、[租户](../tenant/README.md)与[数据库分批读取](../db/README.md#稳定键有界分批读取)。

## SQLite 本地认证边界

SQLite复用标准Db、Provider、迁移和同一现代Fortify状态，最低3.26，不依赖RETURNING或JSON1。认证条件写在首次读取前通过同连接 `BEGIN IMMEDIATE` 取得写所有权；不改变普通 `Db::transaction` 或自动重放回调。Provider/缓存等外部验证不持该写锁，最终事务重核当前状态、来源和期限。自定义监听器另取连接写同一文件仍可能自锁，应使用提供的同一事务。

SQLite支持本地账户、Session、PAT、refresh与现代Fortify，不支持OIDC数据库路径，实际外部关联返回 `unsupported_oidc_database`；单文件不是跨主机共享集群。适用真实后端和当前验收事实见[测试记录](../../../../../.agents/project/memory/testing.md)。

## 外部 OIDC 登录与退出

`auth-oidc` 的 `OidcProvider` / `ExternalIdentityFlows` 是OIDC relying party，不是授权服务器。Discovery、JWKS、token请求走受限 `DependencyClient`；配置固定issuer、回调和 `HttpTargetPolicy`，正常地址使用HTTPS，仅loopback开发允许HTTP。应用登记 `AuthExternalIdentityMigration`，需要统一退出时再登记 `AuthExternalSessionMigration` 及相应账户/凭据前置迁移。

`authorize` 产生独立state和浏览器绑定Cookie，callback传入同一Cookie验证后清理；缺失/不匹配在访问token endpoint前拒绝。不能按相同邮箱自动合并账户；`bind` / `unbind` 重验本地密码，新增账户要求已验证邮箱。协议秘密和ID Token不写日志；自定义Provider负责签名、issuer、audience、时效、state、nonce和令牌绑定。

`complete_with_handle` 返回不透明外部关联句柄，只用于关联和撤销，不是授权。选中authority时，IdP只证明首因子，需要本地2FA则返回 `TwoFactorRequired`，经 `complete_two_factor_login` 同事务消费现代挑战、签发并关联家族；不能把JWT结果写一个用户ID就伪装Web Session。Web须用真实 `FortifyLogin`/SessionGuard建立当前身份，再 `attach_session_with_guard`；关联在事务内重核当前authority与Session绑定，不持users锁跨Guard/Provider回调。

`attach_refresh_credentials` 只接受当前真实raw token及匹配scope、owner、家族、上下文和期限，拒绝消费/撤销/混合数据；本身不轮换token。`begin_logout` 先撤销家族并删除同一实际Session存储中的记录，再生成IdP退出重定向；后通道退出使用签名与claim校验、墓碑及可重试清理，不能因已有墓碑跳过失败重试。Native执行实际DELETE，Cookie快照不能远程删除，跨资源不宣称原子提交。

多副本应共享外部关联数据库、加密密钥和服务器端Session存储，待挑战状态不得依赖进程内存或粘性会话。应用实际退出URI、CSRF和Cookie安全域仍须显式配置。完整OIDC/Socialite、真实IdP与所有存储组合的当前验收不能从局部测试推定，见[正式指南](../../../../../docs/src/security/authentication.md#oidc-关联的当前-authority-核验)。
