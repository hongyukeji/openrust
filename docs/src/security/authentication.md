# 认证

OpenRust 通过具名 Guard 判断当前请求身份，通过 `UserProvider` 检索账户及验证凭据。本文面向应用开发者，介绍当前公共 API、配置和 Session 登录边界；JWT、PAT、refresh、OIDC 及显式双因素增强另见[认证模块指南](../../../vendor/openrust/framework/src/auth/README.md)。

## Guard 与账户来源

在可编辑的 `config/auth.rs` 中声明 `defaults`、`guards` 和 `providers`，由同一 `ConfigRepository` 在 Provider 生命周期中求值。下面是根应用和生成认证应用的配置结构，字段位于 `Auth` 配置返回值中：

```rust,ignore
defaults: serde_json::json!({
    "guard": "web", "passwords": "users", "provider": null
}),
guards: serde_json::json!({
    "web": {"driver": "session", "provider": "users", "remember": 576000}
}),
providers: serde_json::json!({
    "users": {"driver": "app-users", "table": "users"}
}),
timebox_duration: 200_000,
```

| 配置 | 含义 |
| --- | --- |
| `auth.defaults.guard` | `guard(None)` 或空名称使用的默认 Guard，框架回退为 `web`。 |
| `auth.guards.<name>.driver` | Guard 工厂名，内置 `session`。 |
| `auth.guards.<name>.provider` | `auth.providers` 中的账户来源名；未指定时读取 `auth.defaults.provider`。 |
| `auth.providers.<name>.driver` | `UserProvider` 工厂名，内置 `database`；应用可注册自己的 driver。 |
| `auth.defaults.provider` | 直接调用 `create_user_provider(None)` 时也使用此值，不从默认 Guard 反推；默认 null。 |
| `auth.defaults.passwords` | 默认密码 broker 名；此字段本身不创建密码重置流程。 |
| `auth.timebox_duration` | Session Guard 凭据验证失败的时间保护，默认 200000 微秒。 |
| `hashing.rehash_on_login` | 成功登录时按当前密码策略检查重哈希，默认 true。 |
| `auth.guards.<name>.remember` | 记住我 Cookie 的持续分钟数，默认 576000。 |

框架 `auth.providers.users` 的配置回退保留 Laravel 的 `eloquent` / `app::models::User` 声明，但真实 Eloquent `UserProvider` 尚未实现。未注册该 driver 时解析会明确失败，不会偷偷改用数据库 Provider。根应用和生成认证应用显式注册 `app-users`：使用同一个 `DatabaseUserProvider`，通过 `with_user_factory` 构造应用账户并将密码字段适配为既有的 `password_hash`。这是应用适配，不提供 Eloquent 模型生命周期。

装配 `AuthServiceProvider` 后，在当前请求 scope 解析 `AuthManager`。内置 Session Guard 还需要同一请求的 Cookie、Session middleware；将路由放入 `web` 组，或按顺序显式启用 `cookies`、`session`。仅注册 `SessionServiceProvider` 不会让所有 API 路由自动加载 Session。数据库 Session 的迁移与 Cookie 密钥配置见[Session 指南](../../../vendor/openrust/framework/src/session/README.md)。

## 请求内解析与缓存

异步 Guard 工厂可解析其他具名 Guard；同一管理器内递归或并发等待成环时返回 `auth_guard_resolution_cycle`。失败或取消不会缓存半成品，后续独立解析可重试；合法并发仍共享同名实例，`forget_guards` 后的新构造与在途旧实例分开。

未定义 Guard、未注册 driver 或账户来源的配置错误返回 HTTP 500；未知 Provider 名直接通过 `create_user_provider` 查询仍返回 `None`，没有通过认证的普通请求仍按路由要求返回 401。

```rust
use openrust::{Result, auth::AuthManager};

async fn current_id(auth: &AuthManager) -> Result<Option<serde_json::Value>> {
    let web = auth.guard(Some("web")).await?;
    web.id().await
}
```

同一个请求按同名并发解析 Guard，得到同一个实例，已认证账户缓存在这个实例内。`has_user()` 只检查缓存；`user()`、`check()`、`guest()` 和 `id()` 可以触发 Provider 检索，存储错误继续传播。不同请求各自持有 `AuthManager` 和用户缓存，不得将它或 Guard 注册为应用级 singleton。

`should_use("admin")` 只切换本请求后续默认解析，不修改其他请求或全局配置。`forget_guards()` 只清除本请求的 Guard 实例缓存，已持有的句柄仍代表原实例；它不注销、不删除 Session，也不清除 Provider 注册表。`AuthDrivers` 可以应用级共享，但只保存工厂，不能保存已登录用户。

不同具名 Guard 使用各自的 Session 认证键与记住我 Cookie。它们仍共用本请求唯一 Session；一个 Guard 的登录会轮换这份 Session ID。认证来源名也必须参与业务授权，不能因为两个 Provider 返回相同数值 ID 就视为同一账户。

## 账户与凭据

`Authenticatable` 提供标识字段名、原始标识、密码字段名、可选密码哈希以及可选 remember token 的读写契约。`AuthUser` 是 `Arc<dyn Authenticatable>`，供异步调用链共享；账户实现应通过内部可变性更新 token，不跨 `await` 持有同步锁。

`AuthServiceProvider` 将 Laravel 的可空 `Authenticatable` 默认绑定映射到 Rust 的 `Option<AuthUser>`。用 `Inject<Option<AuthUser>>`、`resolver.resolve::<Option<AuthUser>>().await?` 或 `ConstructorParam::class::<Option<AuthUser>>("account")` 读取当前入口的账户。每次解析调用该作用域的 `AuthManager::user(None)`，所以同一请求中替换用户解析器后立即可见。访客为 `None`，登录用户共享 Guard 返回的原账户，失败按容器规则保留原错误来源。Session Guard 仍需已装配的 Web 请求上下文，显式 JWT 则沿已选择的无状态 Guard。

`Authenticatable::set_remember_token` 返回异步 future，模型可复用同一属性 setter/cast。Guard 的 `begin_remember_token_update(expected, replacement)` 返回 `Option<RememberTokenUpdate>`：条件不匹配为 None，成功则已经原子安装完整原始补丁并持有恢复所有权。Provider 成功后 commit；失败或 future 取消时，同步回滚只恢复仍属于本次变更的字段，保留后继写入与缺失/null区别，不在 Drop 中再次调用用户访问器或异步任务。

`GenericUser` 保留原始属性语义，应用模型使用相同存储的 getter/cast；两者不复制 token 状态。外部账户可实现公开 `RememberTokenMutation` 后端并用 `RememberTokenUpdate::new` 包装，后端须同步、幂等且不panic。未实现修改能力时返回 `auth_token_atomic_update_unsupported`，不允许用先读再写模拟原子更新。

`remember_token_for_cookie` 必须原子读取可用于签发 Cookie 的已确认值；另一个 Guard 的未持久化候选不能被当作成功状态。该限制不阻止注销安装并提交后继 token。收到修改冲突或未提交读取错误应安全结束本次认证，不静默签发 Cookie。内存回滚不表示已发出的数据库 I/O 一定未提交；未知提交结果及跨连接持久更新排序须另行证明。

当前 `DatabaseUserProvider` 按固定官方数据库 Provider 直接更新传入 token；自定义用户工厂不使它成为 `EloquentUserProvider`。完整 Eloquent save/dirty/连接/事件生命周期仍需后续交付，不能用该属性前置冒称完成。

ID 保留原始 JSON 数字或非空字符串类型，UUID 可作为字符串使用。Session Guard 拒绝 null、布尔、数组、对象和空白字符串 ID，不把字符串数字静默转换为整数。`GenericUser` 默认读取 `id`、`password`、`remember_token`；其他列名由应用实现 `Authenticatable`。密码和 remember token 可以缺失或为 null，无密码账户不等于空密码账户，也不等于密码验证成功。

`Credentials::new(Map<String, Value>)` 接收应用明确选择的查询字段，例如 `username`、`email` 或手机号字段。框架不会要求三个字段同时存在。Fortify 包默认使用单个可配置的 `username` 字段；根应用另通过公开回调提供一个账号输入框，详见下文“根应用的组合账号入口”。该组合入口复用下文同一现代双因素状态；本文所述官方Web默认与显式JWT跨凭据组合已由 `54f4a087` 限定验收；完整认证/Fortify、Google真实客户端和UUID/字符串JWT持久链仍有缺口。

内置 `DatabaseUserProvider`：

- 使用当前 `Db`、逻辑表名和绑定参数；配置 `connection` 可选具名连接，未指定或为空时复用默认连接。
- `retrieve_by_id` 按 `id` 查询；`with_user_factory` 可以改变账户属性解释，不改变这个默认检索列。
- `retrieve_by_credentials` 排除名称包含 `password` 的条件；其他标量用相等条件、数组用 `IN`、null 用空值条件，没有非密码条件时返回无账户。
- `validate_credentials` 读取明文 `password`，交给应用统一 `HashManager` 验证账户提供的哈希；缺密码或无账户密码返回 false。
- `rehash_password_if_required` 按当前策略更新密码列；remember token 通过 `update_remember_token` 单独持久化。工厂和数据库错误原样传播。

不要把客户端任意 JSON 原样传作查询条件。应用应先校验和挑选允许的字段；账户禁用、租户或其他业务限制也须由应用明确表达。内置数据库 Provider 不自动追加这些业务条件。

## Session 登录与注销

通过 `guard.as_stateful()` 取得可选的 `StatefulGuard`。自定义无状态 Guard 可以返回 None，调用方应显式处理这种情况。

```rust
use openrust::{Error, Result, auth::{AuthManager, Credentials}};

async fn attempt_login(auth: &AuthManager, credentials: &Credentials) -> Result<bool> {
    let guard = auth.guard(Some("web")).await?;
    let stateful = guard.as_stateful().ok_or(Error::Unauthorized)?;
    stateful.attempt(credentials, false).await
}
```

| 方法 | 行为 |
| --- | --- |
| `validate(credentials)` | 只验证，不建立当前请求或持久登录状态。 |
| `attempt(credentials, remember)` | 查询、校验、适用时重哈希，成功后登录；凭据不匹配返回 false。 |
| `login(user, remember)` | 登录可信调用方已经确认的账户，不重新验证密码。 |
| `login_using_id(id, remember)` | 经 Provider 查找并登录，查不到返回 None。 |
| `once(credentials)` / `once_using_id(id)` | 只建立本请求身份，不写登录 Session 或 Cookie。 |
| `set_user(user)` | 直接设置本请求用户缓存，不持久登录。 |
| `logout()` | 删除本 Guard 登录键、清缓存和记住我 Cookie，适用时轮换持久 token。 |
| `StatefulGuard::session_name()` | 返回可选的持久 Session 身份键；非 Session 实现默认返回 None。 |

持久登录将原始账户 ID 写入本 Guard 的 Session 键，并执行 `regenerate(true)`，销毁旧 Session ID、再生 ID 和 CSRF token，保留其他属性。`logout()` 本身不清空整份 Session，也不调用 `invalidate()`；应用需要完整退出流程时，可在注销后显式 `invalidate().await?` 并 `regenerate_token()?`。登录和注销路由仍需应用的 CSRF、防滥用与业务授权保护。

`login` 是受信任的最终登录入口，不自行执行密码或TOTP验证。需要双因素的应用使用 Fortify 管线和可信收据，不在密码通过后直接调用它绕过挑战。通用Guard本身不成为第二套因子状态机。

### 既有邮箱应用流程与具名 Guard

`AuthFlows::login_session_with_guard` / `logout_session_with_guard` 为现有已验证邮箱应用提供带来源校验的Session适配，仍面向正整数账户域。`require_account_provider` 严格核同一Db与实际users表；其他Provider的相同数字ID不能借此登录或注销。需要双因素的账户应完成选中的Fortify流程，不以低层登录入口替代挑战。

唯一示例的[账户安全服务](../../../modules/example/services/account_security.rs)使用同scope具名Guard，受保护Web路由加载Cookie/Session、标准CSRF和 `require_session_account`。启用Fortify时，`require_session_factor` 通过 `authorize_session_factor` 核当前账户状态和真实Guard证明，而非仅查询“已绑定”布尔值。显式 `login_session` / `logout_session` 与 `AuthMode::Session` 是各自现有适配，不是JWT API profile的前置或临时桥接。

### 记住我与 Cookie 保护

设置 `remember=true` 时，Guard 将账户 ID、remember token 和密码哈希的 HMAC 写入记住我 Cookie；缺少有效 token 时生成并通过 Provider 持久化。后续可在缺少 Session 登录键时通过该 Cookie 恢复账户，`via_remember()` 标识本请求的恢复尝试状态，不代替 `check()`。恢复需要 Provider 返回有效 token 和可用密码哈希。

`CookieServiceProvider::new()` 默认使用按名称绑定的 Cookie 加密。值工厂、名称前缀及请求防伪分别对照固定框架的 [CookieJar](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Cookie/CookieJar.php)、[CookieValuePrefix](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Cookie/CookieValuePrefix.php) 与 [PreventRequestForgery](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Http/Middleware/PreventRequestForgery.php)。

记住我 Cookie 必须经 `CookieServiceProvider` 加密保护，且不能列入 `except`；否则返回 `auth_recaller_protection_required`。`hash_password_for_cookie` 只计算 HMAC，不负责 Cookie 加密。客户端 Cookie Session 的两枚 Cookie 同样需要完整保护；plain 或排除保护的载荷不能承载持久认证。普通服务器端 Session 不要求额外使用 remember Cookie。

`SessionGuard` 轮换 remember token 时通过账户的条件修改入口安装原始补丁，并跨 Provider 调用持有拥有式回执。条件已改变时拒绝，准备期间出现版本冲突时可返回 `model_attribute_mutation_conflict`；最终 Cookie 值失效时返回 `auth_remember_token_conflict`。持久化失败或 future 取消时，按字段 owner/祖先链恢复仍属于本次修改的部分，不因 Guard epoch 已变化而遗漏 raw 清理，也不覆盖后继写入。此为 Rust 共享账户并发与 future 取消的内存安全扩展，不构成数据库事务或分布式 CAS。

登录、token 持久化、Cookie 排队或认证事件失败/取消时，请求内状态按拒绝认证处理；已发出的数据库 I/O 或其他外部写入不承诺撤回或自动回滚。可通过 Events 订阅 `Attempting`、`Validated`、`Login`、`Authenticated`、`Failed`、`Logout`，但不要记录账户秘密或凭据。

## 路由身份与授权隔离

使用 `AuthLayer::guard("web")` 保护必需认证路由；`AuthLayer::optional_guard("web")` 允许访客，Provider、配置和存储失败仍返回错误。通过 `CurrentUser` / `OptionalUser` 提取路由层验证后的身份，`CurrentUser::id()` 返回原始 JSON 标识，`guard_name()` 和 `provider_name()` 返回具名来源。嵌套可选认证成为访客时会清除外层身份投影。

旧业务表明确采用正整数账户主键时，调用 `CurrentUser::integer_id()?`。它只接受正的 i64 数字，字符串数字、UUID 或超范围数字不会被截断或转换。JWT/PAT 等旧显式凭据路径仍可没有具名 Guard/Provider，不能由此推断为 `web/users`。

现有整数角色表 `Policy` 接收具名身份前，须通过 `Policy::with_guard_scope(guard, provider)` 显式绑定相同 Guard 和 Provider；不匹配返回 `auth_policy_provider_scope_mismatch`。ID 非正整数时该角色策略不授予权限。UUID 应使用适配其数据模型的授权实现，不能借同名整数 ID 查询另一来源的角色。此角色策略不是完整 Laravel Gate/Policy 生态。

普通 Web 请求安装 Session middleware 时即绑定认证请求上下文，不要求额外安装 `AuthLayer` 才能投影元数据。原生数据库 Session 每次保存前从当前默认 Guard 的实际 `id()` 投影 `sessions.user_id`，包括受管理自动保存和已绑定该请求的显式 `save()`；注销后保存写 null。切换 `should_use` 会影响后续保存的来源，不能从任意历史认证属性猜测 ID。默认迁移的 `user_id` 为 bigint；使用 UUID/字符串账户时，应用必须显式提供兼容字段类型，框架不会自动改表。该列是元数据，认证仍从 Guard 和 Provider 读取。

## 持续服务器撤销与外部登录关联

`SessionStoreHandle::authorized_until_guard(session, guard)` 读取实际 Guard 的 `id()` 与 `StatefulGuard::session_name()`，要求当前 Session 的对应键和实际持久记录都具有相同的原始类型账户 ID。它读取 Session 当前实际 handler，不续期，返回不晚于持久记录或 TTL 的授权截止。仅通过 `once` / `set_user` 建立、没有匹配持久登录键的身份不能取得此期限。

该方法通过实际 handler 的 `load_authorized` 读取当前记录。原生 database 使用同次读取的真实 `last_activity` 与配置 lifetime 计算截止，不续期；客户端 Cookie 快照不能提供独立服务器端读取，仍拒绝。`database-guarded` 提供更强的删除后迟到保存保护；原生 database 沿 Laravel 的并发 INSERT 回退，不能防止迟到 writer 重建已删除记录。唯一示例的[浏览器实时连接](../../../modules/example/http/controllers/article_socket.rs)核对来源、资源所有权与具名 Session 身份后，用这个期限限制连接。它是持续撤销需求下的扩展，不改变 Laravel 原生 database 的默认语义。

启用 `auth-oidc` 后，`ExternalIdentityFlows::attach_session_with_guard(user_id, handle, session, guard)` 先校验 Provider 使用同一 `Db` 句柄与实际 `users` 表，再要求正整数外部关联账户与实际 Guard ID、对应 Session 登录键一致，随后关联当前 Session ID；没有对应键的仅请求身份不能关联。唯一示例的[OIDC 服务](../../../modules/example/services/oidc.rs)先重新验证应用密码，再调用此接口。外部句柄只用于关联与撤销，不能代替账户认证；远程注销使用创建该 Session 的同一服务器端存储；原生 database 执行实际 DELETE，但不具备 Guarded 的防复活保证。

`authorized_until(session, user_id)` 和 `attach_session(user_id, handle, session)` 保留显式旧认证键的适配契约；具名身份使用上述带 Guard 的方法。OIDC 尚不支持 SQLite，具名 Guard 适配不会扩大该后端范围。

## 扩展 Guard 和 `UserProvider`

在应用 `ServiceProvider` 的 `register` 中从同一容器解析 `AuthDrivers`，调用 `extend(driver, creator)` 或 `provider(driver, creator)` 注册工厂。工厂返回 `BoxFuture`，结果分别为 `Arc<dyn Guard>` 与 `Arc<dyn UserProvider>`。

Guard 工厂收到 `GuardContext`，包含本请求 `resolver`、具名 `name` 和配置；`request().await` 取得当前 `RequestInfo`，`user_provider().await` 按需创建配置来源，`user_provider_name()` 取得来源名称。不需要账户来源的 Guard 可以不调用 Provider 工厂。实现 Guard 时，`provider_name()` 必须准确返回实际来源，供授权隔离使用。

Provider 工厂收到本请求 `Resolver` 和 `UserProviderConfig`。自定义来源须实现按 ID、按 token、按 credentials 检索、密码校验、token 更新及重哈希契约；查询凭据本身不得建立登录态。`create_user_provider(None)` 在默认 provider 为 null 或对应配置不存在时返回 None；已配置但 driver 未注册则报错。工厂只供请求解析，不能把 resolver、Session 或用户保存在共享注册表中。

根应用的[Provider 注册](../../../app/providers/app.rs)及[账户适配](../../../app/models/user.rs)展示 `DatabaseUserProvider::with_user_factory` 的真实接线；生成模板使用同样公开机制。唯一综合业务模块的[账户安全处理器](../../../modules/example/http/controllers/account_security.rs)经同 scope `AuthManager` 和 `AuthFlows::login_session_with_guard` 展示真实 Session 登录与业务 DTO，整数账户的业务调用显式使用 `integer_id()`；这些流程不替代自定义 Guard/Provider 的适配。更多底层 API 见[认证模块指南](../../../vendor/openrust/framework/src/auth/README.md)。

`UserProvider::uses_database_accounts(db, table) -> Result<bool>` 是既有数据库账户流程与具名 Guard 桥接的显式兼容能力，默认返回 false。`DatabaseUserProvider` 同时比较 `Db` 句柄身份和经表名映射解析的实际表；仅 URL、Provider 名称或账户 ID 相等不能证明兼容。包装 Provider 若确实委托同一账户来源，可实现此能力；不能为通过校验无条件返回 true。该方法是既有增强流程的安全适配，不是 Laravel 官方 `UserProvider` 原有的第七个方法；自定义 Guard/Provider 的独立认证不要求兼容旧 `users` 账户域。

## Fortify 与 TOTP

启用 `auth-fortify` 后，Fortify 对应入口位于 `openrust::auth::fortify`，包括 `FortifyServiceProvider`、`FortifyConfig`、`TwoFactorOptions`、`TwoFactorAuthenticationProvider`、`TwoFactorActions`、`FortifyLogin` 与响应扩展。它们复用同一 Container、具名 Guard、Session 和加密器。

MFA是多因素认证的总称，2FA表示双因素形式，TOTP是其中一种验证码技术。本框架只使用同一现代Fortify三字段状态，不提供另一套独立因子账户。下文区分已交付的官方Web默认与显式JWT API限定能力；真实Google Authenticator客户端互操作、完整Fortify及UUID/字符串JWT持久链仍有未闭合边界。

### 受管凭据的显式桥接

选择 `RequireFactorAuthority` 的应用须登记 `FactorAuthorityMigration` 和 `CredentialFactorMigration`，并让持久账户模型提供同一数据库、账户表和原始类型 ID 的 authority。后者为 refresh 与 PAT 增加可空上下文字段；历史 NULL 行不能自动升级为受管凭据，存在非 NULL 资产时回滚迁移会拒绝。

实际具名 Session 可以调用 `AuthFlows::issue_credentials_from_session` 签发 JWT/refresh；需要 PAT 时先由 `CredentialAuthority::prepare_session` 取得不可外部构造的 `VerifiedCredential`，再交给 `PersonalAccessTokens::issue_from_verified`。只有实际因子验证收据才表示完成第二因子；当前无需第二因子的账户快照不产生该证明。签发时重核当前主库状态和父凭据期限，PAT 派生还受父权限与截止时间约束。

请求处理应按模式通过 `FortifyJwtApi::authenticate`、`CredentialAuthority::authenticate_access_token` 或受管 PAT 验证入口核验当前 authority 与撤销状态。`JwtTokens` 的同步密码学校验不代表主库授权；默认数据库认证器不会将非 NULL 的受管 PAT 上下文当作普通 PAT。refresh 轮换保留原家族和因子来源，旧 refresh 重放会撤销该家族。唯一示例在现有同源、CSRF 和具名 Session 账户组中提供 `/api/example/account/session/credentials` 与只授予文章读取权限的 `/session/read-token`；该示例取24小时上限与真实父凭据截止中的较早时刻，不延长 Session 授权。直接挑战、OIDC 关联和实时连接均有各自当前状态核验入口；不能将接口存在或单条链路通过推定为全部存储、后端和客户端组合已验收。

### 显式 JWT API profile

官方Fortify提供Web/Session认证流程，不内置无Session的JWT登录。Laravel生态的Passport、Sanctum属于不同可选能力。OpenRust的JWT API profile是明确选择的公开扩展，不改变框架或官方Fortify默认；当前支持下述直接挑战、管理及access/refresh流程，不表示Passport、OIDC Provider或全部认证生态已完成。

生成应用可选择：

```sh
openrust new blog --profile jwt-api --db sqlite
```

普通 `new` 不隐式选择 JWT；`--auth` 和 `--fortify` 均安装 Web/Fortify 模式。根应用在启用 `auth-fortify` 时默认选择 Web；显式设置 `AUTH_PROFILE=jwt-api` 才选择 JWT API，配置在启动期间固定，未知值会拒绝启动。JWT API profile 使用 `FortifyJwtApiServiceProvider` **替换** `FortifyServiceProvider`，不同时挂载两份认证路由。它复用同一账户模型、`UserProvider`、`TwoFactorActions`、`DatabaseFactorAuthority`、密码和加密服务。应用仍须配置数据库、JWT key ring 与加密密钥，并登记 `FortifyTwoFactorMigration`、`FactorAuthorityMigration`、`CredentialFactorMigration` 和 `CredentialChallengeMigration` 及其账户/refresh/PAT前置迁移。

手动装配时，在 `auth.guards.api` 明确配置 `driver="jwt-api"`、`provider="users"`，该 Provider 必须使用 authority 的同一 `Db` 和实际 `users` 表；`fortify-api.guard="api"` 选择此 Guard，`fortify-api.prefix="/auth"` 相对于应用 API 前缀。默认配置 `api.prefix="api"`（实际URL前缀为 `/api`）时路由如下：

| 方法 / `/api/auth` 下路径 | JSON 输入或授权 | 结果 |
| --- | --- | --- |
| POST `/login` | `fortify.username` 对应字段和 `password` | 直接凭据，或 `two_factor: true`、`challenge`、`expires_at` |
| POST `/two-factor-challenge` | `challenge`，以及 `code` 或 `recovery_code` | 完成一次挑战后返回凭据 |
| POST `/refresh` | `refresh_token` | 同一家族的新 access/refresh，原绝对期限不延长 |
| POST `/logout` | `refresh_token` | `logged_out: true`，撤销家族 |
| GET `/user` | `Authorization: Bearer <access_token>` | 当前原始 `id` |
| POST `/two-factor/authorize` | 当前 Bearer 和 JSON `password` | `management_token` |
| POST `/two-factor/restart` | 当前 Bearer 和 JSON `password` | 丢弃待确认绑定材料并返回新 `management_token`；已确认账户拒绝 |
| POST / DELETE `/two-factor` | `X-Fortify-Management` | 准备绑定 / 关闭 |
| POST `/two-factor/confirm` | 管理 header 和 JSON `code` | `confirmed: true`、`login_required: true` |
| GET `/two-factor/secret` | 管理 header | `secret_key` |
| GET `/two-factor/qr-code` | 管理 header | 本地 `svg` 与 `url` |
| GET / POST `/two-factor/recovery-codes` | 管理 header | 读取 / 整组重建 `recovery_codes` |

凭据响应为裸 JSON，包含 `user_id`、`access_token`、`token_type: "Bearer"`、`refresh_token`、`refresh_expires_at`；该适配的所有响应附 `Cache-Control: no-store` 和 `Pragma: no-cache`。只从请求体接收密码、challenge 和 refresh token，不放入 URL。请求来源取可信 `ClientIp`，须按实际反向代理链配置；缺少可信地址会拒绝。受保护资源用选中域的 `AuthLayer::guard("api")` 或 `FortifyJwtApi::authenticate`，不能以同步 JWT 签名校验代替主库家族和当前因子核验。

API 路由只挂 `api` middleware 组，不挂 Session、Cookie、CSRF 或 remember；应用也不得给该组叠加 `web`。显式 JWT API 流程均无 Session/Cookie 依赖，不以 Web 登录作为前置。未来授权中心默认遵对应官方 Web 机制，只有显式 JWT 中心模式遵循无 Cookie 契约；中心 SSO 的 token 持有、回访和浏览器限制留待授权服务器能力设计取证，不把token放入URL自建回访机制。客户端持有 Bearer 和短挑战，全程不借临时 Session 模拟无状态认证。挑战不是正式身份，完成前不签发 access/refresh；恢复码优先于 TOTP。直接入口的挑战有效期五分钟，最多五次实际尝试，并绑定账户、当前代际、请求来源及所选 Guard/Provider scope。公开底层入口为 `LoginAttempt::TwoFactorRequired(TwoFactorLoginChallenge)` 和 `AuthFlows::complete_two_factor_login`；本 profile 通过 `FortifyJwtApi` 进一步核验选中 scope。

管理授权须先提交**当前有效 access JWT 与真实密码**。`JwtApiFactorManagement` 返回加密 grant，最长300秒，且不超过父 access/真实家族期限；它绑定密码摘要、Guard、Provider、账户与家族。随后只将它放入唯一 `X-Fortify-Management` header，不当 Bearer、PAT、第二因子证明或一般资源授权。管理限流按方法和路径分别计数，不是整次绑定生命周期共享五次额度。首次启用和恢复码重建后，同一有效管理授权可连续读取新密钥、QR 与恢复码；它不让旧 access 获得新代际的资源权限。遗失待确认绑定的管理授权时，可用当前 JWT 和密码调用 `/two-factor/restart`；此操作生成全新密钥，旧绑定材料和授权失效，不能读取旧密钥，也不能重置已确认的因子。确认或关闭后按 `login_required` 重新登录取得当前凭据，不能靠客户端 JSON 的成功标志自行续期或升级身份。

当前 JWT/refresh/PAT 与 API 管理持久适配限于同一 `users.id` 的真实正整数域。通用 Guard、`Authenticatable`、Fortify 模型和三字段 store 保留数字/字符串/UUID原始 ID；不能据此宣称字符串账户已有完整 JWT profile 闭环。

### 实时连接与当前凭据

`Realtime::connect_with_validator` 接受公开 `RealtimeValidator`，在注册及发送、输入、心跳前异步核验真实凭据，收紧而不延长原连接截止。唯一示例的 JWT validator 重核真实家族、当前因子状态及文章所有权；浏览器使用 `SessionStoreHandle::prepare_live_guard` 取得不可外部构造的 `LiveSessionCredential`，从实际 Session handler 的独立 reader 重新读取持久载荷、身份、证明和期限。票据不保留请求 Guard 缓存，首次成功后绑定 source/generation，后续代际变化拒绝。

Native database 从同次实际行读取计算活动截止，删除后下一次实际读取会拒绝；这不改变其迟到 INSERT 可能重建记录的原生语义。Cookie 快照仍不能做独立服务器端重验。验证器不持连接集合互斥锁或数据库事务跨任意回调等待，也不承诺撤销与已经发生的网络发送原子化。

### 包回退、安装默认与应用覆写

`FortifyConfig::default()` / `TwoFactorOptions::default()` 对应包回退；`installed()` 显式选择官方安装模板默认。根应用在可编辑的 [`config/fortify.rs`](../../../config/fortify.rs) 中再作应用覆写，所有值通过同一配置仓库求值。

| 配置 | 包回退 | 官方安装默认 | 根应用选择 |
| --- | --- | --- | --- |
| `fortify.guard` | `web` | `web` | `web` |
| `fortify.username` | `email` | `email` | `account`，由组合回调解释 |
| `fortify.lowercase_usernames` | false | true | false，组合回调不改写大小写 |
| `fortify.home` | `/home` | `/home` | 可由应用覆写 |
| `fortify.prefix` / `domain` | 空字符串 / null | 相同 | 可由应用覆写 |
| `fortify.middleware` / `views` | `web` / true | 相同 | 可由应用覆写 |
| `fortify.limiters.login` | null | `login` | 继承安装选择 |
| `fortify.limiters.two-factor` | null | `two-factor` | 继承安装选择 |
| `fortify.features` | 固定包7项，含双因素与邮箱验证 | 固定模板6项，不含邮箱验证 | 继承安装清单 |
| `confirm` | false | true | true |
| `confirmPassword` | false | true | true |
| `secret-length` | 16 个 Base32 字符 | 相同 | 相同 |
| `window` | null，沿引擎默认1 | 相同，可显式覆写 | 相同 |

双因素开关为 `fortify.features` 中的 `two-factor-authentication`，不再使用选项对象中的 `enabled`。功能名称清单保留固定包/模板原值，不表示其余尚未验收功能已经交付。路由及 `confirmPassword` 中间件在应用启动时装配，启动后更改清单不会自动重建路由；默认登录管线选择、确认规则和真正启用/强制重建时的密钥长度在调用时读取同一配置仓储。显式 null 密钥长度按上游整数转换为0，由内置生成器拒绝；负值无法表示为 Rust 的 `usize`，在实际取长度时明确拒绝。已有非假值密钥且未 force 的幂等动作不提前读取无效长度。

`confirm` 控制绑定后是否必须确认才能用于登录；`confirmPassword` 控制管理路由是否要求近期密码确认，二者相互独立。关闭其中一个不隐含关闭另一个。配置公开名称保留 `confirmPassword`、`secret-length`；它们位于双因素选项对象中，不是哈希成本参数。

CLI 的脚手架便利入口为：

```sh
openrust new blog --fortify
```

它选择同一应用中的 Fortify/TOTP 配置、Provider、迁移及认证/加密能力，仍需应用的数据库与密钥配置。这个 `new` 选项不是完整 `fortify:install` 命令，也不表示完整包安装发布流程或生成应用的安全组合已验收。官方包其余功能不能由此选项视为全部实现。

### 根应用的组合账号入口

根应用的 [`app/providers/fortify.rs`](../../../app/providers/fortify.rs) 经 `FortifyCallbacks::authenticate_using` 接入一个 `account` 输入框和一个密码框。用户填写用户名、已绑定邮箱或手机号任一种；不要求三个标识同时提交。回调使用所选具名 Guard 的同一 `UserProvider`，验证成功归一为同一账户 ID。

组合回调选择大小写敏感的逐字匹配，不自动 lowercase 或 trim 账号值；查询后仍核对账户字段与输入相等，不依赖数据库默认的不区分大小写查询来决定归属。`phone` 只参与已是 E.164 形式的输入匹配，不猜国家或区域、不自动为本地号码补前缀；同字符串仍可作为用户名匹配。这不表示手机号归属已经验证。不同字段命中不同账户时拒绝歧义，不按查询顺序任选账户。

这是根应用的增强策略，不改变 Fortify 默认的单个 `username=email` 字段。应用账户的 `email`、`phone` 可以缺失或为 null，只有用户名的账户也可由该回调验证密码；这不意味着既有邮箱注册、验证码、找回密码或第三方登录流程都已支持无邮箱。密码确认使用 `FortifyCallbacks::confirm_passwords_using`，对当前受验证账户通过同一 Provider 校验密码，不重新猜账号来源。

数据库唯一索引控制可创建的标识集合，其大小写、等价字符与 collation 规则仍取决于实际后端和列定义。回调逐字匹配不等于已统一所有后端的唯一性语义，也不保证数据库会允许同时创建仅大小写不同的两个标识。应用须按自己的列和索引策略管理这些数据；组合入口及本文所述双因素跨凭据组合已有 `54f4a087` 限定验收；不据此声明所有后端唯一性、全部标识持久链或完整Fortify已完成。

### 应用账户结构升级与回滚

根应用的 [`AccountIdentifiers`](../../../database/migrations/accounts.rs) 以追加迁移将既有 `email` 改为可空，新增可空 `username`、`phone` 及各自唯一索引；不改写已执行的基础迁移。空字段不是可匹配账号，新增字段也不会自动生成或验证真实登录标识。

回滚在删除列前检查数据：只要有 `email IS NULL`，或任意非 null 的 `username` / `phone`，即拒绝无损回退。空字符串同样属于非 null 标识；迁移不会填假邮箱、删除已有标识或自行转换数据来强行通过。需要回滚时由应用先显式迁出数据，再评估原结构是否仍适用。

MySQL 的列修改必须重述定义，因此此迁移只接受应用原有的默认 `email` 列形状。自定义类型、default、额外属性、comment 或独立 collation 等会明确拒绝，要求应用提供保全这些属性的显式升级；不静默重置为框架默认。该拒绝同样适用于回滚预检，不能将其解释成通用数据库迁移器已支持任意自定义列形状。

### 三项双因素属性与动作

`TwoFactorAuthenticatable` 暴露持久状态读取和条件更新，`TwoFactorState` 包含 `two_factor_secret`、`two_factor_recovery_codes` 与 `two_factor_confirmed_at`。前两项保存密文，第三项为可空时间。账户不必是 Eloquent 模型，也不要求整数 ID；通过 `DatabaseTwoFactorState` 或公开模型能力接入自己的持久来源。

| 动作 | 官方语义与现有公开入口 |
| --- | --- |
| 启用 | `TwoFactorActions::enable(user, force)`：无 secret 或 force=true 时生成 secret 和恢复码；重复启用不默认覆盖。 |
| 确认 | `confirm(user, code)`：验证当前 secret 对应代码并记录 confirmed_at；无 secret、空码或错误码产生字段验证错误。 |
| 禁用 | `disable(user)`：清除 secret、恢复码和适用的确认时间；不默认要求每次再提交 TOTP。 |
| 读取恢复码 | `recovery_codes(user)`：解密读取当前官方模式的恢复码列表，不是仅在生成时可读。 |
| 重建恢复码 | `generate_new_recovery_codes(user)`：替换整组恢复码。 |
| 手动密钥 / QR | `secret_key(user)`、`qr_code_url(user, company, username)` 与 `two_factor_qr_code_svg(url)`，只在授权读取路径暴露秘密。 |

根应用 `confirm=true`：用户可按需绑定，未绑定时密码正常登录；生成 secret 后尚待确认，不是已开启双因素。确认后后续登录进入挑战；关闭按管理入口要求先作必要身份确认。`confirm=false` 遵循上游 secret 非 null 的启用规则，不全局改写包回退。Web 与 JWT 模式的 Fortify 动作共用应用加密绑定。先用 `key:generate` 设置 `APP_KEY`，轮换时配置 `app.previous_keys` 以读取历史 Laravel 密文。TOTP 密钥与恢复码 JSON 均按 PHP 字符串值写入密文。覆盖优先级依次为 `TwoFactorActions::with_encrypter`、模型的 `two_factor_encrypter`、应用容器的 `dyn ApplicationEncrypter`。

`TwoFactorActions::with_recovery_code_generator`、`with_options` 与公开 Provider/响应绑定允许应用替换对应机制；扩展不得把账户秘密放进日志或调试输出。条件更新竞争失败和持久化错误会传播，不应被调用方吞为匿名成功或“已经禁用”。

### 官方恢复码

官方默认生成8个 `10字符-10字符` 格式恢复码，加密为 JSON 数组保存；受保护 GET 可以重新读取，POST 重建替换整组。成功使用一个恢复码后用新随机码替换该码，列表仍保留8项，旧码不能因重复展示而再次有效。恢复码生成器可由公开 hook 覆写。

### TOTP 与本地 QR

默认 secret 为16个 Base32 字符，不是16或20随机字节；算法为 HMAC-SHA1、6位数字、30秒时间步，window=1 接受相邻窗口，可显式配置0。密钥由同一公开 Provider 生成并按模型/动作所选加密器持久化。

二维码与手动密钥来自同一 secret。URI 包含 issuer、账户标签、algorithm、digits、period；issuer 使用最终应用名称，账户标签由 `TwoFactorAuthenticatable::two_factor_account_name` 提供，不以可空邮箱作为全局必需字段。组合入口的 `account` 是输入名，不等于持久账户字段；根应用模型显式依次选择非空 username、email、phone，均缺失时使用账户标识文本作为显示标签。这不转换或改变认证使用的原始 ID。完整组合 HTTP 与真实客户端流程仍未验收。本地 SVG 生成不向外部二维码服务发送密钥。HTTP 无 secret 时 QR 返回 `[]`、手动 secret 接口返回404，读取仍服从认证和 `confirmPassword` 规则。

RFC 向量、URI 差分与 SVG 解码只提供本地协议证据。真实 Google Authenticator 扫描、手动输入、生成代码及跨时间窗操作仍未验收，不能将“兼容 TOTP 参数”写成“Google 客户端已测”。

### HTTP 与近期密码确认

Fortify 路由按最终 prefix、domain、路径覆写与 middleware 配置注册，不强加 `/api` 前缀或统一 JSON envelope。默认管理接口包括：

| 方法 / 路径 | 用途 |
| --- | --- |
| POST `/user/two-factor-authentication` | 启用，接受可选 force |
| POST `/user/confirmed-two-factor-authentication` | 提交确认 code |
| DELETE `/user/two-factor-authentication` | 禁用 |
| GET `/user/two-factor-qr-code` / `/user/two-factor-secret-key` | 本地 QR / 手动密钥 |
| GET / POST `/user/two-factor-recovery-codes` | 读取 / 重建恢复码 |

`confirmPassword=true` 时，管理接口要求近期密码确认；确认成功将时间记在 `auth.password_confirmed_at`，不是每个动作都重新提交密码。超时按普通请求重定向，JSON 请求返回423；密码确认本身不表示已经完成第二因子。

首因子发现需要挑战时保存 `login.id` 与 `login.remember`，返回 `two_factor=true` 或挑战重定向，完成前不应发布正式身份或 bearer 凭据。成功二因子才经同一 Guard 登录并处理 remember、Session 再生与响应；失败通过字段错误或公开响应契约呈现。官方 Web 管线不因此改为 JWT；无 Cookie API 的操作见上文显式 profile。

### 同一现代状态与可信证明

根应用及生成的 Fortify 配套选择 `DatabaseFactorAuthority`，由同一账户模型公开 `two_factor_authority()`。它复用 `DatabaseTwoFactorState` 的真实连接、逻辑表和原始 ID；`FactorAuthorityMigration` 保存随机 source、持久 generation、短期验证码摘要及受限管理绑定，不创建第二份账户。未选择该 adapter 的官方三字段模型保留原路径。没有独立旧因子服务、兼容接管或旧数据迁移前置。

`FactorReceipt` 只由框架在实际验证和持久消费后签发。`StatefulGuard::login_with_factor_receipt`、`session_factor_proof` 将证明与实际 Guard/Provider、原始身份、source/generation 绑定。客户端 JSON、普通布尔值、密码确认或管理授权都不是该证明。SessionGuard 在转换提交前不公开证明；普通登录、once、set_user、注销及失败取消不会保留不相符的旧证明。自定义 Guard 默认拒绝不支持的带收据登录，不能静默忽略。

Web 的 `FactorManagementContext` 由真实持久 Guard 身份产生，只给同一 Session 在授权的启用、force、恢复码重建之后继续管理新代际；它不放行资源。确认绑定将实际验证收据附加到当前 Session。JWT API 的管理 grant 另通过当前 Bearer+密码建立，不能从 Web Session 或请求属性伪造。两种适配都调用同一 `TwoFactorActions` 和 authority。

带真实因子收据的 remember 登录可在已认证加密 Cookie 中追加有界证明段，仍核实际 Provider、原始 ID 与主库 source/generation；无证明的普通 remember 不因复用 token 自动升级。状态变化或证明损坏拒绝恢复，不能降级为密码-only。

### 并发、取消与审计

三字段 CAS 和同账户事务在提交前核验当前快照与持久代际。成功 TOTP 在同事务消费短期摘要，恢复码成功则原子替换该码；竞争者不能同时消费同一状态。确认字段、代际与代码消费共同提交，失败回滚不留下已确认状态。管理变更保留尚未过期的代码摘要。

Provider/cache 验证在账户事务外完成；数据库回滚不撤回第三方缓存、网络或已发出的 I/O。Session transition 不提前公开未完成登录；数据库、Session Store、事件或外部回调之间不宣称跨资源事务。有限验证码账本不承诺任意未来扩窗或时钟回退下永久不重放。

通用审计扩展为 `ApiLoginAuditSink`，成功双因素直接登录在凭据消费的同一事务调用它。`DatabaseApiLoginAudit::new(db, audit)` 复用通用脱敏策略，并严格核同一 `Db` 与实际 `audit_records` 表；可通过 `FortifyJwtApiServiceProvider::with_database_audit` 显式选择。默认不绑定该 sink，不自动查询审计表；自定义 sink 必须使用传入事务，错误回滚本次消费和签发，不记录密码、代码、secret 或 token。

## 双因素事件订阅

八个双因素事件沿用 `openrust::auth::fortify` 的公开 Rust 类型。字符串事件名由 `OpenRust\Fortify\Events\` 和类型名组成，覆盖启用、确认、关闭、恢复码生成和替换，以及登录挑战、失败和验证码通过。使用 `EventsServiceProvider::listen::<TwoFactorAuthenticationEnabled, _>` 可按类型订阅；需要按名称订阅时，可用该类型的 `Event::NAME` 或 `OpenRust\Fortify\Events\*`。

升级时，将写死的 `Laravel\Fortify\Events\` 或 `OpenRust\Auth\TwoFactor\Events\` 字符串前缀改为 `OpenRust\Fortify\Events\`。受信监听器可以通过 `EventContext::transaction()` 取得派发方事务。恢复码只可由授权监听器显式读取，调试输出脱敏。 [上游启用动作](https://github.com/laravel/fortify/blob/c456642584c102f6a6aafee9932299542da637b5/src/Actions/EnableTwoFactorAuthentication.php) · [上游挑战完成入口](https://github.com/laravel/fortify/blob/c456642584c102f6a6aafee9932299542da637b5/src/Http/Controllers/TwoFactorAuthenticatedSessionController.php)

## Laravel 对应与 Rust 边界

本文对照固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的 `AuthManager`、`CreatesUserProviders`、`SessionGuard`、`DatabaseUserProvider` 与认证契约，以及 docs `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 authentication 与 fortify 章节。Fortify/TOTP 对照固定 Fortify `c456642584c102f6a6aafee9932299542da637b5` 与 google2fa `f00bc788c555adfb6765c437ff3538e59cd88af1`，不随浮动分支静默切换默认值。异步 trait 使用 `BoxFuture`，账户使用 `Arc`，模型通过已编译工厂显式注册，不能运行期按 PHP 类名实例化。

Laravel 用 `sha1(static::class)` 生成 Session/remember 名称后缀。Rust `SessionGuard` 采用固定实现后缀 `ff9a88a790ea81b2b89656e56873ed257a77b366`，分别生成 `login_<guard>_<后缀>` 与 `remember_<guard>_<后缀>`，对应上游类名 SHA-1 的名称隔离职责。Rust 没有 PHP 的动态类名机制，不运行期复刻 `static::class`，也不承诺与 PHP Session/Cookie 字节互通；该后缀不承担密码散列或凭据签名职责。Cookie 加密检查、失败/取消时拒绝部分认证和显式 Provider 授权隔离属于安全边界；Guarded Session 的持续撤销保证则是显式增强，不是 Laravel 原生 database 的默认语义。

当前具名认证 API 与 Fortify/TOTP 实现不代表完整 Laravel 认证章节、真实 Eloquent、所有 Guard driver、Gate/Policy 或完整 Fortify 已交付。本文所述现代因子与显式 JWT API profile 已由 `54f4a087` 完成本批限定组合验收；完整认证章节、Google真实客户端和UUID/字符串JWT持久链仍开放，既有 JWT、PAT 能力不替代这些剩余对应项。

### OIDC 关联的当前 authority 核验

选用 `CredentialAuthority` 的 `ExternalIdentityFlows` 与 `AuthFlows` 必须使用同一 `Db` 句柄和物理账户/凭据表；构造期拒绝错误组合。IdP 返回的身份只证明外部第一因素。当前账户需要本地因子时，回调返回同一直接挑战体系的 `LoginAttempt::TwoFactorRequired`，由 `complete_two_factor_login` 验证真实恢复码/TOTP后签发受管家族；因子来源保留实际 `external:<provider>` 流程。

`attach_session_with_guard` 在事务外准备真实 Guard、Provider、当前因子证明及 Session 绑定，在同一主库事务内重核当前代际，并同步复核同一 Session。`attach_session` 在所选策略下要求已有受保护的显式 Session 上下文；只有手写用户 ID 不足以关联。准备后的 Session 轮换、身份或证明变化会拒绝关联。此检查不承诺数据库与 Session Store 的跨资源原子提交，也不将客户端 Cookie 快照变成可即时远程撤销存储。

`attach_refresh_credentials` 在所选策略下检查实际当前 raw token、完整家族、真实 owner、因子上下文与截止时间；已消费、撤销、到期或家族混合数据均拒绝。attach 本身不轮换或消费凭据，拒绝旧 raw 不会顺带撤销仍有效的后继家族，关联期限取实际校验的截止。使用真实当前凭据关联待完成句柄时，会在同一事务删除其原挑战；过期或已撤销的句柄不能复活。后通道与应用注销覆盖现代挑战及家族。未选择该显式策略的普通调用方保留原契约。

注销调用方必须传入创建关联 Session 的同一应用存储。Native 数据库 driver 使用其实际低层 DELETE，失败仍保留未清理标记供重试；它沿用 Laravel 的并发 INSERT 回退，不能承诺防复活。客户端快照仍不能远程删除。挑战清理使用当前实际选中的现代 authority，不以编译 feature 推断迁移已安装；关联撤销墓碑阻断已撤销句柄再次完成或关联。

本段为当前 WIP 接口说明；限定测试记录和完整组合验收分开报告，完整 OIDC/Socialite、真实 IdP 协议与所有 Session 存储组合不由局部测试推定完成。

### 单次验证码事实与 Provider 扩展

框架默认将 `TwoFactorAuthenticationProvider` 登记为应用单例，`TwoFactorActions` 保持 scoped。内置 Google Provider 每次验证从同一配置仓储读取整数 `fortify-options.two-factor-authentication.window`；null、缺失或非整数沿用该实例上一次窗口，初始窗口为 1。显式整数包括零与负数；零只检查当前步，负数执行空扫描。一次验证固定其开始读取的窗口，等待缓存期间的新配置由后一次验证读取。

自定义 Provider 需要实现 `verify_with_context`，返回 `TwoFactorCodeVerification`。原 `verify` 方法保留布尔门面，默认委托同一次验证，不会再次消费缓存。包装器应直接转交内层事实：

```rust,ignore
fn verify_with_context<'a>(
    &'a self,
    secret: &'a str,
    code: &'a str,
) -> BoxFuture<'a, Result<TwoFactorCodeVerification>> {
    self.inner.verify_with_context(secret, code)
}
```

自行校验的实现可用 `TwoFactorCodeVerification::verified(code, window, current_counter, matched_counter)` 报告默认30秒周期，或用 `verified_with_period` 报告实际周期。该类型绑定原始 code 的摘要并校验计数范围、周期及溢出；拒绝结果使用 `rejected()`。这是供原子消费使用的验证事实，不能代替当前账户、代际、Session/Guard 绑定或 `FactorReceipt`。公开 trait 的这一变更要求自定义实现同步新方法；仅布尔结果无法表达实际验证的窗口与时域，框架不会按 Actions 选项猜测。

确认、Session 挑战及直接/外部挑战均传递同一次事实。Provider 与缓存校验在账户事务外完成，事务内只核当前状态及消费，不再次调用 Provider。nonce 保留期使用该次实际窗口和周期，即 `(2 * window + 3) * period_seconds`；使用真实验证事实，不按另一份配置猜测窗口。

Provider 本身仍遵循官方非原子缓存 get/verify/put：缓存 miss 成功后再次读取时钟作为 put 的时间值，返回的验证事实保留真正校验时的计数。有限 nonce 只覆盖该次接受域；管理员之后任意扩窗、冻结或大幅回退时钟可能让历史码重新进入接受域，不承诺跨任意未来策略的永久不重放。挑战的一次性消费与当前 source/generation 核验不能由短期缓存替代。

密码驱动、默认值、已有哈希迁移与bcrypt输入等价边界见[哈希](hashing.md)。

## 增强密码与验证码的 manager 注入

增强入口使用有类型的 manager：`EncodedPasswordHash::make(&HashManager, &str)`，实例 `verify`、`needs_rehash` 和静态 `validate_input` 也接收 `&HashManager`。`AuthFlows::new` 与 `VerificationCodes::new` 的密码依赖为 `Arc<HashManager>`，其余账户、验证码 Store、JWT、refresh 与限流参数保持各自职责。应用 Provider 应传入同一容器解析的 manager，不创建并行密码服务。

每个秘密域的一次操作仅选择一次 `driver_for_operation()` 最终具体驱动，并保活到该操作的相关 await 完成。登录的能力检查、真实校验和原有 dummy 复用同一叶；另一次操作仍能采用新的 live 默认。验证码不存在或数字格式非法时保留原 false 旁路，非法格式仍扣失败尝试。reset 的新密码生成与验证码核验是两个秘密操作，按原先密码先行的顺序分别使用各自注入的 manager；公开手动构造允许不同实例，不用密码流程配置覆盖验证码配置。

非空和1024 UTF-8字节边界仍先检查，bcrypt 72字节/NUL别名的增强拒绝与既有错误保持。普通 UserProvider 不因此强加增强限制；选择一次驱动不证明当前账户、权限或因子已通过，最后状态核验与版本 CAS 仍不可省略。原有 Web/Session、CSRF、remember 和显式无 Cookie JWT profile 不因 manager 类型变化而改变。

已有 custom 驱动经 `HashManager::extend` 返回原 `Arc<dyn Hasher>`，保留算法、成本、容量与状态，不能为了改参数重建默认驱动。详细契约见[哈希指南](hashing.md#增强认证的单次最终叶)。Model binary setter、非 UTF-8 encoded hash 与隐藏 custom 转发仍有独立边界，不能从 manager 注入推定其已完成。
