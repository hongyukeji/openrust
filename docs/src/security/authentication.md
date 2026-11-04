# 认证

OpenRust 通过具名 Guard 判断当前请求身份，通过 `UserProvider` 检索账户及验证凭据。本文面向应用开发者，介绍当前公共 API、配置和 Session 登录边界；JWT、PAT、refresh、OIDC 及现有 MFA 增强另见[认证模块指南](../../../src/auth/README.md)。

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

装配 `AuthServiceProvider` 后，在当前请求 scope 解析 `AuthManager`。内置 Session Guard 还需要同一请求的 Cookie、Session middleware；将路由放入 `web` 组，或按顺序显式启用 `cookies`、`session`。仅注册 `SessionServiceProvider` 不会让所有 API 路由自动加载 Session。数据库 Session 的迁移与 Cookie 密钥配置见[Session 指南](../../../src/session/README.md)。

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

`Authenticatable::compare_and_set_remember_token(expected, replacement) -> Result<bool>` 要求在同一账户锁内比较并替换 token：匹配并替换返回 true，值已变化返回 false。`GenericUser` 用单个 `RwLock` 原子完成操作，根应用与生成模型委托同一实现。自定义账户未实现时，默认在 token 修改前返回 `auth_token_atomic_update_unsupported`，不允许用先读取再写入模拟原子更新；这会拒绝需要轮换 token 的记住我操作，包括适用的注销轮换。

ID 保留原始 JSON 数字或非空字符串类型，UUID 可作为字符串使用。Session Guard 拒绝 null、布尔、数组、对象和空白字符串 ID，不把字符串数字静默转换为整数。`GenericUser` 默认读取 `id`、`password`、`remember_token`；其他列名由应用实现 `Authenticatable`。密码和 remember token 可以缺失或为 null，无密码账户不等于空密码账户，也不等于密码验证成功。

`Credentials::new(Map<String, Value>)` 接收应用明确选择的查询字段，例如 `username`、`email` 或手机号字段。框架不会要求三个字段同时存在；一框组合登录输入的 resolver 尚未提供，根应用现有注册和登录流程仍使用 email schema。接受某个查询字段并不表示框架已经自动识别任意账号输入。

内置 `DatabaseUserProvider`：

- 使用当前 `Db`、逻辑表名和绑定参数；配置 `connection` 可选具名连接，未指定或为空时复用默认连接。
- `retrieve_by_id` 按 `id` 查询；`with_user_factory` 可以改变账户属性解释，不改变这个默认检索列。
- `retrieve_by_credentials` 排除名称包含 `password` 的条件；其他标量用相等条件、数组用 `IN`、null 用空值条件，没有非密码条件时返回无账户。
- `validate_credentials` 读取明文 `password`，交给 `Passwords` 验证账户提供的哈希；缺密码或无账户密码返回 false。
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

`login` 是受信任的最终登录入口；需要多因素的流程必须在第二因子完成后调用。通用 Guard 不查询应用的 MFA 状态，不会自行执行第二因子或 Fortify pipeline。已有 MFA 增强与完整 Fortify、官方 TOTP 流程是不同范围，后两者尚未交付。

### 既有邮箱应用流程与具名 Guard

`AuthFlows::login_session_with_guard(email, password, request_source, session, guard)` 保留既有应用的已验证邮箱、禁用账户、限流及 MFA 门禁，再经指定 Guard 的 `login_using_id` 建立身份，返回 `SessionCredentials` 的 `user_id()` 和 `csrf_token()`。它移除旧显式认证属性，使用 Guard 再生的标准 Session token，不签发 JWT 或 refresh。此适配仍面向既有整数账户表。`AuthFlows::require_account_provider(guard)` 校验 Guard 的 Provider 是否使用该流程同一 `Db` 句柄与实际 `users` 表；登录与注销的带 Guard 接口均先执行此校验，来源不匹配返回 Unauthorized，不会按其他 Provider 的相同整数 ID 登录或注销。

`logout_session_with_guard(request_source, session, guard)` 要求该 Guard 的正整数身份，通过退出限流后注销 Guard，移除旧显式认证属性并执行 `regenerate(true)`，保留业务偏好。它与仅调用 `guard.logout()` 的行为不同。写请求使用标准 Session CSRF middleware；登录入口的来源校验仍由应用负责。

唯一示例的[账户安全服务](../../../modules/example/services/account_security.rs)使用这两个方法和同 scope 的 `web` Guard。受保护账户 Session 路由先加载 Cookie/Session，经 `AuthLayer::guard("web")` 和标准 CSRF middleware，再在启用 `auth-mfa` 时执行应用的 `require_session_factor`：调用 `AuthFlows::mfa_enabled(integer_id)`，对已启用 MFA 的账户拒绝该 Session 路径。这保留了启用 MFA 后旧 Session 不继续充当第二因子凭据的增强策略；`mfa_enabled` 只查询启用状态，不证明当前请求完成了 MFA。示例的 MFA challenge 登录仍走既有流程，不代表官方 Fortify 已交付。

旧 `login_session` / `logout_session` 与 `AuthMode::Session` 继续为显式旧认证资产提供适配；新的唯一示例账户 Session 路径使用具名 Guard，不再通过旧绑定键建立身份。

### 记住我与 Cookie 保护

设置 `remember=true` 时，Guard 将账户 ID、remember token 和密码哈希的 HMAC 写入记住我 Cookie；缺少有效 token 时生成并通过 Provider 持久化。后续可在缺少 Session 登录键时通过该 Cookie 恢复账户，`via_remember()` 标识本请求的恢复尝试状态，不代替 `check()`。恢复需要 Provider 返回有效 token 和可用密码哈希。

记住我 Cookie 必须经 `CookieServiceProvider` 加密保护，且不能列入 `except`；否则返回 `auth_recaller_protection_required`。`hash_password_for_cookie` 只计算 HMAC，不负责 Cookie 加密。客户端 Cookie Session 的两枚 Cookie 同样需要完整保护；plain 或排除保护的载荷不能承载持久认证。普通服务器端 Session 不要求额外使用 remember Cookie。

`SessionGuard` 轮换 remember token 时用原子比较交换（CAS）安装候选值；若账户已被并发修改，返回 `auth_remember_token_conflict`，不覆盖新状态。持久化失败或 future 取消时，仅当账户仍持有本操作候选值且操作仍有效，才用 CAS 恢复旧值；返回 false 时保留其他 Guard 已写入的新值。这是 Rust 共享账户并发与 future 取消所需的适配，不是上游 PHP 原有的多线程要求，也不构成数据库事务或分布式 CAS。

登录、token 持久化、Cookie 排队或认证事件失败/取消时，请求内状态按拒绝认证处理；已发出的数据库 I/O 或其他外部写入不承诺撤回或自动回滚。可通过 Events 订阅 `Attempting`、`Validated`、`Login`、`Authenticated`、`Failed`、`Logout`，但不要记录账户秘密或凭据。

## 路由身份与授权隔离

使用 `AuthLayer::guard("web")` 保护必需认证路由；`AuthLayer::optional_guard("web")` 允许访客，Provider、配置和存储失败仍返回错误。通过 `CurrentUser` / `OptionalUser` 提取路由层验证后的身份，`CurrentUser::id()` 返回原始 JSON 标识，`guard_name()` 和 `provider_name()` 返回具名来源。嵌套可选认证成为访客时会清除外层身份投影。

旧业务表明确采用正整数账户主键时，调用 `CurrentUser::integer_id()?`。它只接受正的 i64 数字，字符串数字、UUID 或超范围数字不会被截断或转换。JWT/PAT 等旧显式凭据路径仍可没有具名 Guard/Provider，不能由此推断为 `web/users`。

现有整数角色表 `Policy` 接收具名身份前，须通过 `Policy::with_guard_scope(guard, provider)` 显式绑定相同 Guard 和 Provider；不匹配返回 `auth_policy_provider_scope_mismatch`。ID 非正整数时该角色策略不授予权限。UUID 应使用适配其数据模型的授权实现，不能借同名整数 ID 查询另一来源的角色。此角色策略不是完整 Laravel Gate/Policy 生态。

普通 Web 请求安装 Session middleware 时即绑定认证请求上下文，不要求额外安装 `AuthLayer` 才能投影元数据。原生数据库 Session 每次保存前从当前默认 Guard 的实际 `id()` 投影 `sessions.user_id`，包括受管理自动保存和已绑定该请求的显式 `save()`；注销后保存写 null。切换 `should_use` 会影响后续保存的来源，不能从任意历史认证属性猜测 ID。默认迁移的 `user_id` 为 bigint；使用 UUID/字符串账户时，应用必须显式提供兼容字段类型，框架不会自动改表。该列是元数据，认证仍从 Guard 和 Provider 读取。

## 持续服务器撤销与外部登录关联

`SessionStoreHandle::authorized_until_guard(session, guard)` 读取实际 Guard 的 `id()` 与 `StatefulGuard::session_name()`，要求当前 Session 的对应键和实际持久记录都具有相同的原始类型账户 ID。它读取 Session 当前实际 handler，不续期，返回不晚于持久记录或 TTL 的授权截止。仅通过 `once` / `set_user` 建立、没有匹配持久登录键的身份不能取得此期限。

该方法要求 `ServerSide` 撤销保证；原生 database 与客户端 Cookie driver 返回 `session_store_guarantee_unsupported`，应显式选择 `database-guarded` 或等价服务器端 driver。唯一示例的[浏览器实时连接](../../../modules/example/http/controllers/article_socket.rs)核对来源、资源所有权与具名 Session 身份后，用这个期限限制连接。它是持续撤销需求下的扩展，不改变 Laravel 原生 database 的默认语义。

启用 `auth-oidc` 后，`ExternalIdentityFlows::attach_session_with_guard(user_id, handle, session, guard)` 先校验 Provider 使用同一 `Db` 句柄与实际 `users` 表，再要求正整数外部关联账户与实际 Guard ID、对应 Session 登录键一致，随后关联当前 Session ID；没有对应键的仅请求身份不能关联。唯一示例的[OIDC 服务](../../../modules/example/services/oidc.rs)先重新验证应用密码，再调用此接口。外部句柄只用于关联与撤销，不能代替账户认证；要求远程注销时仍须具备服务器持续撤销存储。

`authorized_until(session, user_id)` 和 `attach_session(user_id, handle, session)` 保留显式旧认证键的适配契约；具名身份使用上述带 Guard 的方法。OIDC 尚不支持 SQLite，具名 Guard 适配不会扩大该后端范围。

## 扩展 Guard 和 `UserProvider`

在应用 `ServiceProvider` 的 `register` 中从同一容器解析 `AuthDrivers`，调用 `extend(driver, creator)` 或 `provider(driver, creator)` 注册工厂。工厂返回 `BoxFuture`，结果分别为 `Arc<dyn Guard>` 与 `Arc<dyn UserProvider>`。

Guard 工厂收到 `GuardContext`，包含本请求 `resolver`、具名 `name` 和配置；`request().await` 取得当前 `RequestInfo`，`user_provider().await` 按需创建配置来源，`user_provider_name()` 取得来源名称。不需要账户来源的 Guard 可以不调用 Provider 工厂。实现 Guard 时，`provider_name()` 必须准确返回实际来源，供授权隔离使用。

Provider 工厂收到本请求 `Resolver` 和 `UserProviderConfig`。自定义来源须实现按 ID、按 token、按 credentials 检索、密码校验、token 更新及重哈希契约；查询凭据本身不得建立登录态。`create_user_provider(None)` 在默认 provider 为 null 或对应配置不存在时返回 None；已配置但 driver 未注册则报错。工厂只供请求解析，不能把 resolver、Session 或用户保存在共享注册表中。

根应用的[Provider 注册](../../../app/providers/app.rs)及[账户适配](../../../app/models/user.rs)展示 `DatabaseUserProvider::with_user_factory` 的真实接线；生成模板使用同样公开机制。唯一综合业务模块的[账户安全处理器](../../../modules/example/http/controllers/account_security.rs)经同 scope `AuthManager` 和 `AuthFlows::login_session_with_guard` 展示真实 Session 登录与业务 DTO，整数账户的业务调用显式使用 `integer_id()`；这些流程不替代自定义 Guard/Provider 的适配。更多底层 API 见[认证模块指南](../../../src/auth/README.md)。

`UserProvider::uses_database_accounts(db, table) -> Result<bool>` 是既有数据库账户流程与具名 Guard 桥接的显式兼容能力，默认返回 false。`DatabaseUserProvider` 同时比较 `Db` 句柄身份和经表名映射解析的实际表；仅 URL、Provider 名称或账户 ID 相等不能证明兼容。包装 Provider 若确实委托同一账户来源，可实现此能力；不能为通过校验无条件返回 true。该方法是既有增强流程的安全适配，不是 Laravel 官方 `UserProvider` 原有的第七个方法；自定义 Guard/Provider 的独立认证不要求兼容旧 `users` 账户域。

## Laravel 对应与 Rust 边界

本文对照固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的 `AuthManager`、`CreatesUserProviders`、`SessionGuard`、`DatabaseUserProvider` 与认证契约，以及 docs `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 authentication 章节。异步 trait 使用 `BoxFuture`，账户使用 `Arc`，模型通过已编译工厂显式注册，不能运行期按 PHP 类名实例化。

Laravel 用 `sha1(static::class)` 生成 Session/remember 名称后缀。Rust `SessionGuard` 采用固定实现后缀 `ff9a88a790ea81b2b89656e56873ed257a77b366`，分别生成 `login_<guard>_<后缀>` 与 `remember_<guard>_<后缀>`，对应上游类名 SHA-1 的名称隔离职责。Rust 没有 PHP 的动态类名机制，不运行期复刻 `static::class`，也不承诺与 PHP Session/Cookie 字节互通；该后缀不承担密码散列或凭据签名职责。Cookie 加密检查、失败/取消时拒绝部分认证和显式 Provider 授权隔离属于安全边界；Guarded Session 的持续撤销保证则是显式增强，不是 Laravel 原生 database 的默认语义。

当前具名认证 API 不代表完整 Laravel 认证章节、真实 Eloquent、所有 Guard driver、Gate/Policy、Fortify 或官方 TOTP 已完成；已有 JWT、PAT 与 MFA 也不替代这些对应项。
