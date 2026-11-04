# Session 使用指南

Session 为同一请求提供共享属性与 flash，并在响应发送前持久保存。本文面向使用 `openrust::session` 的应用开发者；业务示例位于唯一的 [`modules/example`](../../modules/example/http/controllers/session.rs)。

## 启用与存储

编译 `session` feature，并在应用 Provider 清单中登记 `SessionServiceProvider`。启用 `auth` 已包含 `session` 编译能力，但仍需要应用明确装配 Provider。认证入口与通用 Session 共用同一 Provider、句柄和存储，不能再登记第二套 Session Provider。Provider 只登记运行时；路由用 `RouteGroup::new().middleware_group("web")` 或在 API 中显式追加 `middleware_alias("cookies")`、`middleware_alias("session")` 才启动和保存会话。普通 API 不自动读取 Cookie、执行 Session GC 或发送 Session cookie；实际引用但 driver 禁用时 Router 构建报配置错误。

同时启用 `view` 时，Session middleware 会在 `start` 成功后、进入处理器前恢复 `errors` 为当前请求的视图共享对象；损坏的已识别错误包会拒绝请求。处理器可用 `RequestInfo::put_view_error_bag` 修改本请求可见的命名包；已有 Session 错误对象的修改同步到 Session，但不延长原 flash 寿命；`Session::flash_errors` 显式闪存错误，已有 flash 可通过 `keep`/`reflash` 延续。整个 `errors` 属性被替换后，视图原对象保留但与 Session 断开；缺失时的空回退对象不会写回。没有 Session Provider 时，视图只得到本请求的独立空包。`Session::read_errors()` 返回独立读取快照；请求共享修改使用 `RequestInfo::put_view_error_bag`。

默认 driver 是 `database`，复用已有 `Db` 和逻辑 `sessions` 表。按后端启用 `db-postgres`、`db-mysql` 或 `db-sqlite`，登记 `DatabaseServiceProvider`，配置数据库并运行迁移。SQLite Session 的适配不代表认证、队列或完整 Schema 已适配。下面是已配置加密密钥的宿主片段；`EncryptionServiceProvider` 需要 `encryption` feature：

```rust
use openrust::core::ProviderBuilder;
use openrust::database::DatabaseServiceProvider;
use openrust::http::{CookieServiceProvider, HttpServiceProvider};
use openrust::security::EncryptionServiceProvider;
use openrust::session::SessionServiceProvider;

fn register(builder: &mut ProviderBuilder) {
    builder
        .provider(DatabaseServiceProvider)
        .provider(HttpServiceProvider)
        .provider(EncryptionServiceProvider)
        .provider(CookieServiceProvider::new())
        .provider(SessionServiceProvider::new());
}
```

不需要 Cookie 加密的明确使用场景可以选择 `CookieServiceProvider::plain()`；它不提供 Cookie 机密性或完整性保护，不能当作 Laravel 默认 Web 组。密钥配置见[安全能力](../security/README.md)，请求 Cookie 规则见 [HTTP 指南](../http/README.md#cookie-读取队列与响应)。

### 迁移只选一种入口

- 不使用认证迁移时，在应用 Migrator 中登记公开 `SessionMigration`。
- 已登记 `AuthMigration` 时，它内部已经复用 `SessionMigration`，不要再次登记独立 `SessionMigration`。重复迁移版本的回滚可能删除另一版本仍依赖的表。

迁移使用应用的表名前缀与完整名称映射。原生表为 `id/user_id/ip_address/user_agent/payload/last_activity`。自定义表用 `SessionMigration::for_table` 创建，运行期 `session.table` 选择同一逻辑名称。旧摘要表无法恢复原始 ID，须显式处理迁移并保全原数据，框架不会覆盖。

### 原生 database 与显式增强

`DatabaseSessionHandler` 对应固定 Laravel `d52c3b3` 的低层接口，接收原始字节及任意 ID；受管 `Session` 仍验证 40 字符 ID。原生 `payload` 是属性 JSON 的 Base64，不包含身份或绝对期限 envelope。JSON 是 Rust 属性表示的适配，不与 PHP serialize 字节互通；ID 与载荷均不进入日志。

读取过期条件为 `last_activity < 当前秒数 - lifetime秒数`；过期行返回空属性但不删除。`validate_id` 只检查存在，过期行仍返回 true。handler 的 `exists` 在每个 Session 内独立，`migrate` 清除此标志；普通写入按该标志选择 UPDATE 或 INSERT，INSERT 查询错误按上游回退 UPDATE，UPDATE 零行仍表示处理成功。连接获取、传输及已知结果不确定故障保守报错；不能据此宣称所有 PDO 异常已严格对应。

需要身份摘要、微秒绝对截止、防止删除后迟到写入复活、OIDC 远程撤销或浏览器实时授权截止时，显式选择 `session.driver = "database-guarded"` 或 `GuardedDatabaseSessionStore`。在同一原生表迁移后追加 `GuardedSessionMigration` 与 `GuardedSessionGcMigration`；它们扩展该表，不建立第二套 Session。增强载荷为受限 JSON envelope，不与原生载荷混用；切换 driver 前选择新表或显式处理已有记录。SQLite 增强列回滚经标准迁移 Provider 的旧版删列重建执行，不能放入用户自建事务。

`NativeDatabase` 保证分类允许普通登录及当前请求销毁，不能冒充 `ServerSide` 持续撤销保证。`SessionStoreHandle::delete`、`authorized_until`、`authorized_until_guard` 对原生数据库明确拒绝；这些增强的调用方须选 Guarded 或等价 driver。已有属性的原生/增强跨界切换也拒绝，保留原 handler。原生期限来自最终 `lifetime`，绝对 `Expiry` 需选增强 driver。

`SESSION_CONNECTION` 缺失或未加引号的 `null/(null)` 选择默认连接，具名值经同一 `Databases` 解析。`SESSION_TABLE` 默认 `sessions`；显式 null 因 Rust 非可空类型报配置错误，不回退默认。HTTP 绑定记录可信 IP 和 UA；UA 按固定 PHP 默认 UTF-8 与问号替代规则转换，截取 500 个 Unicode 字符。普通 Web/显式 Session middleware 在启用认证时即安装同一请求上下文，不要求路由额外使用认证层。绑定同一请求 scope 后，原生数据库 Session 在每次 `save()` 前解析当前默认具名 Guard 的实际 `id()`，自动保存与显式手动保存走同一投影；`should_use` 切换后使用新的默认来源，注销后保存 null。不扫描历史登录属性猜测身份，也不把 `user_id` 元数据当作认证依据。独立构造、未绑定认证请求的 Session 不自动取得 HTTP 身份。

默认迁移的 `sessions.user_id` 是 bigint；Guard 支持字符串与 UUID ID，不会使该列自动变为 UUID。应用须为实际账户 ID 提供兼容的 Session 表字段类型，类型不匹配的数据库错误会传播。来源隔离、`CurrentUser::id()` 和显式正整数业务适配见[认证指南](../../docs/src/security/authentication.md)。

### 具名 Guard 的持久身份与连接期限

认证代码通过 `StatefulGuard::session_name()` 取得可选持久身份键，不猜测 Guard 键或读旧显式认证属性。`SessionStoreHandle::authorized_until_guard(&session, guard)` 同时核对 Guard 原始 ID、请求 Session 对应键与当前实际 handler 的持久记录，读取不续期；没有匹配持久登录键的仅请求身份不能取得连接期限。该方法要求 `ServerSide` 保证，应选 Guarded 或等价服务器端 driver，不能将原生 database 或 Cookie 快照当作持续撤销保证。

唯一示例的浏览器实时连接使用这个带 Guard 的期限接口，OIDC 关联使用 `ExternalIdentityFlows::attach_session_with_guard`，先校验 Provider 与既有流程使用同一 `Db` 句柄和实际 `users` 表，再验证对应身份键；相同整数 ID 不能绕过账户来源隔离。旧 `authorized_until` / `attach_session` 保留显式旧认证资产的适配；它们不表示示例仍使用旧绑定键。更多调用和 MFA 的应用策略边界见[认证指南](../../docs/src/security/authentication.md)。

### 显式内存与自定义 store

内存 store 适用于测试或明确接受进程内状态的应用；不会在数据库缺失、连接失败或 driver 名称不支持时自动启用：

```rust
use openrust::config::SessionConfig;
use openrust::session::{MemorySessionStore, SessionServiceProvider};

let provider = SessionServiceProvider::with_store(MemorySessionStore::default())
    .with_cookie_config(SessionConfig {
        driver: Some("array".into()),
        secure: Some(false),
        ..SessionConfig::default()
    });
```

也可将最终配置的 `driver` 明确设为 `array`。内存 store 不共享到其他进程，进程重启后状态丢失；单实例最多保留 1024 条记录，容量不足会报错，不悄悄逐出仍有效的会话。

### 客户端 Cookie driver

将宿主最终配置中的 driver 显式设为 `cookie`，继续使用同一个 `SessionServiceProvider` 与 `CookieServiceProvider`：

```toml
[session]
driver = "cookie"
lifetime = 120
expire_on_close = false
```

正式宿主与生成应用按最终 driver 安装唯一 Session Provider，数据库应用无需额外启用 Auth。显式 database 缺少 URL 时仍会安装，在实际 I/O 明确失败，不暗降级到 memory；未知 driver 在最终配置校验报错。

此模式不需要 Session 数据表；应用其他数据库功能仍按自己的 Provider 装配。外部 driver 组合也可使用公开的 `CookieSessionStore::new(lifetime_minutes, expire_on_close)`。构造器验证正数分钟与时间范围，不在构造时保存请求。自定义包装 driver 必须委托 `guarantees()` 与 `supports_session_authentication(cookie_name)`，并保持请求绑定前后的保证分类一致。

浏览器保存两枚 Cookie：配置名 Cookie 保存会话 ID，以 ID 为名称的 Cookie 保存属性字符串和整数秒截止。默认 Cookie Provider 加密并校验两枚 Cookie，历史密钥仍可解密；业务通过普通 Session API 使用，不接触 wire 内容。显式 plain 模式没有机密性或防篡改能力，不能用来保护认证状态。数据 Cookie 采用 `CookieFactory` 默认的 `HttpOnly=true`、`Partitioned=false`；ID Cookie 单独采用 `session.http_only`、`session.partitioned`，两者的路径、域、安全传输与 `SameSite` 沿宿主统一默认值。

`expire_on_close=true` 使两枚 Cookie 都成为浏览器会话 Cookie，内含截止仍有效。截止所在整秒可读，下一秒失效；此 driver 的显式 `set_expiry` 同样按秒表示，相比纳秒期限可能延长不足一秒。Rust 使用 JSON 表示属性，不与 PHP serialize 字节互通。

Session 认证另需完整性保证。框架只信任实际 Cookie 中间件提供的保护事实，配置中放入密钥或编译 `encryption` 本身不等于 Cookie 已受保护。plain 模式、ID 名被 `except` 排除，或任一合法 40 位数据 Cookie 名被排除时，Cookie driver 不支持 Session 认证；拒绝任一合法数据 ID 排除是为了防止 ID 轮换后改变安全保证。当前 `except` 按精确名称匹配，`*` 不代表通配符。

上述组合仍可用于明确的非认证 Cookie 业务，但登录、登出和实际 Session 认证入口会返回 500 `session_cookie_protection_required`。登录/登出在限流与密码验证等副作用前检查；认证属性保存时也按最终 handler 和 Cookie 名称复核，不能通过 `set_name` 或 `set_handler` 降级保护。JWT 独立认证不会仅因编译了 Cookie driver 就被拒绝。无加密构建应使用服务器端 Session 后端；要使用 Cookie Session 认证，则需启用加密并装配受保护 Cookie Provider。

标准 Web/CSRF 组要求 Session ID Cookie 和 XSRF-TOKEN 受保护；plain 或排除 ID Cookie 的配置会在构建 Router 时以 `web_cookie_encryption_missing` 拒绝，尚不会进入上述请求期认证。仅排除数据 Cookie 的配置仍由 Cookie Session 认证入口拒绝。

Cookie/Session 机制自身失败（包括请求准备、保存、Cookie 生成、保护或头部预算失败）响应统一带 `Cache-Control: no-store` 和 `Pragma: no-cache`。替换错误响应不会携带原业务正文、Cookie 或内容长度；此规则不改变普通成功响应的缓存策略。

Cookie driver 是**客户端快照**：登出和迁移仅删除当前浏览器的旧数据 Cookie。复制的旧 Cookie 在内含期限前仍可重放，并发响应由浏览器最后接收者决定；它不能提供服务器全局撤销、跨客户端原子 Update 或全局 ID 碰撞检测。普通当前请求登录、登出使用绑定 Session；无请求的 `SessionStoreHandle::delete` 与要求服务器持续撤销保证的 `authorized_until` 返回 `session_store_guarantee_unsupported`，不会假报远程注销成功。

仍须遵守统一 Cookie 队列、完整头部与 Session 记录预算；64 KiB 的框架头部上限不表示浏览器允许同样大小的 Cookie。浏览器通常有约 4 KiB 的单 Cookie 限制，加密也会增大 wire 值，较大属性应选择数据库或 Redis。框架不在超限时悄悄改存其他 driver。

异常数据的策略也有边界：坏 JSON、非对象根、缺失/null 的截止或已过期值视为未命中；非整数截止及未过期对象中的缺失/null/非字符串 data 以 `session_cookie_payload_invalid` 拒绝。PHP 的部分弱类型比较/返回转换与此不同，不能将正常格式的往返当作这些异常语义已严格等价。driver 只读取入站快照及自身已成功排队的输出；其他代码预排同名同路径 Cookie 会使写入显式冲突，不能静默覆盖。自身写入后的队列载荷以摘要核对，若被其他代码替换则拒绝读取/续写；排队删除则按未命中处理。

公开扩展入口是 `SessionStore`，通过 `SessionServiceProvider::with_store(store)` 安装，优先于配置中的 driver 选择。实现者需要遵守[存储契约](store.rs)：

受信应用 Provider 也可用 Container 的 `extend::<SessionStoreHandle, _>` 包装框架原有 store：装饰器从 `old.store()` 克隆原 `Arc<dyn SessionStore>`，再用委托型 wrapper 构造新句柄，不需要读取或重建原 factory。`store()` 是底层逃生口；直接执行其 I/O 会绕过受管理 Session 的状态与容量检查，也绕过 `SessionStoreHandle::delete` 对 `ServerSide` 保证的检查。调用者须自行承担并发、容量和撤销契约，不能将客户端快照称为可全局撤销。独立 consumer 的[公开接线测试](../../tests/consumers/container-extension/README.md#第三方-sessionstore-装饰)展示该路径。

| 方法 | 要求 |
| --- | --- |
| `load(id, max_bytes, empty_expiry)` | 在复制或解码完整载荷前限制字节；返回指定身份的记录。`empty_expiry` 供损坏载荷的空属性回退使用。 |
| `guarantees()` | 默认 `ServerSide` 声明增强条件写入/删除；原生数据库为 `NativeDatabase`，客户端快照为 `ClientSideSnapshot`。 |
| `semantics()` / `for_session()` | 包装器转发持久语义和请求独占工厂；exists 不能保存在应用单例中。 |
| `accepts_request()` / `bind_request()` | 原生数据库接收可选 HTTP 元数据，Cookie driver 仍强制请求上下文。 |
| `reset_exists()` / `activity_lifetime_seconds()` | 包装器转发原生 ID 迁移与最终活动期限。 |
| `supports_session_authentication(cookie_name)` | 默认仅服务器后端返回 true。客户端快照必须明确提供自己的来源与完整性验证；仅声明一个 driver 名称不构成保证。 |
| `write(record, mode, max_bytes)` | 增强后端原子区分模式；原生数据库使用 handler exists，`Written` 不保证影响行数非零。客户端快照只检查请求内状态。 |
| `delete(id)` | 重复删除成功；不能因此重建记录。 |

公开 `SessionRequestContext::new` 构造的 Cookie 快照不附带受管理中间件的可信保护事实，不能用它伪装内置 Cookie 认证请求。第三方客户端 driver 若返回认证支持，须自行负责验证载荷完整性，并在请求绑定与包装中保留该保证。

自定义实现不得把 `Update` 的 `Missing` 改为插入。`ServerSide` 不自动表示多节点共享；例如内存后端仍只属于本进程。错误不得包含原始属性、ID 或服务凭据。框架仍会校验自定义 store 返回的身份、属性和整体记录上限。可选 Redis 实现需要 `session-redis` 并通过公开 `RedisSessionStore::connect` 和 `with_store` 明确安装；仅写一个 driver 名称不会自动建立服务连接。

## 最终配置与 Cookie

`SessionServiceProvider::new()` 读取最终 `session` 区段；`with_cookie_config(SessionConfig)` 在该 Provider 的 boot 中覆盖此区段，更晚的配置覆盖仍按正常顺序生效。

```toml
[session]
driver = "database"
path = "/"
http_only = true
same_site = "lax"
lifetime = 120
expire_on_close = false
partitioned = false
max_bytes = 20971520
serializer = "native"
```

| 字段 | 默认与行为 |
| --- | --- |
| `driver` | `Some("database")`。Rust 配置中的 `None` 明确禁用请求 Session 层，不降级内存。 |
| `serializer` | `native` 默认保持纯数据记录格式并在保存时拒绝对象；`json` 使用对象显式投影，重载按 PHP 关联数组语义。均不兼容 PHP serialize 字节。 |
| `cookie` | `None`：按最终 `app.name` 计算 `Str::slug(name, '_') + "_session"`；显式 `Some(name)` 优先。 |
| `path` / `domain` | `/` / `None`；`domain=None` 不发送 Domain，使用浏览器的 host-only Cookie。 |
| `secure` | `None`：按可信请求传输推导；`Some(true/false)` 是显式设置。 |
| `http_only` / `same_site` | `true` / `Some("lax")`；`same_site=None` 明确省略属性，省略配置键则仍使用默认 Lax。 |
| `lifetime` | 120 分钟非活动超时；每次保存更新截止，必须为正且可安全换算。 |
| `expire_on_close` | `false`；设为 true 时发送浏览器会话 Cookie，持久记录仍受 `lifetime` 限制。 |
| `partitioned` | `false`；启用时发送 Partitioned 标记，应用还需正确设置 Secure 并满足浏览器要求。 |
| `max_bytes` | 默认 20 MiB，只能调低。原生数据库约束 Base64 解码后的属性字节，增强 driver 计入完整 envelope。 |

TOML 没有 null 字面量。需要把可选字段明确设为 `None` 时，可以使用 `SessionConfig` 或支持 null 的配置源；不要用空字符串代替。

默认名称使用固定英文转写规则，涵盖重音、希腊及西里尔等映射；不支持的字符按源规则删除。空 slug 的结果是 `_session`，不会换成品牌常量。推导使用本项目明确固定的 portable-ascii 2.0.2 映射；其版本来源与许可见[实现说明](name.rs)。名称推导对输入、中间与输出字符串分别限制为 20 MiB；最终 Cookie 头仍统一检查 64 KiB 上限，超限不会截断名称。

`secure=None` 不相信任意客户端伪造的 `X-Forwarded-Proto`。TLS 终止代理必须按应用可信代理配置接入；直连 TLS 与受信代理才参与安全传输判断。`Secure` 只约束 Cookie 发送条件，不加密 Cookie；反之 Cookie 加密也不代替 TLS。

Session、认证与 Cookie 加密是独立能力。CLI 的 `--auth` 不要求同时使用 `--encryption`；启用加密的正式 Cookie Provider 使用当前密钥保护响应，并可按配置使用历史密钥读取旧 Cookie。显式 plain 模式下 Session Cookie 仍可能是原始 ID，不应将它称为受保护值。业务代码通过 Session API 读属性，不自行解密请求 Cookie。

## 请求共享句柄与属性

受管理请求进入处理器前已经 `start().await`。响应完成时先预检整份响应的 Cookie，包括既有 Cookie 头；预检失败不会开始本次自动保存。通过后执行 `save().await`，再按保存后的最终 Session ID 重新生成并附加出站 Cookie；首次 remember 恢复可能在保存中轮换 ID，Cookie 到期时间也在保存后重新求值，避免慢保存扣减本次出站 `lifetime`。已确认的 Precognition 请求跳过自动保存，仍执行 Cookie 预检与最终生成。属性方法同步执行，不需要 `.await`；存储 I/O、ID 迁移等方法是异步的。普通处理器不要重复启动或保存，因为每次保存都会推进 flash 年龄。

```rust
use openrust::http::{Json, Request};
use openrust::session::Session;
use serde_json::{Value, json};

async fn store(session: Session, request: Request) -> openrust::Result<Json<Value>> {
    let preference = request.string("preference")?.into_string();
    session.put_typed("example.preference", preference)?;
    session.flash_typed("example.notice", "已保存")?;
    Ok(Json(json!({"saved": true})))
}
```

正文提取器 `Request` 放在参数最后。正式 Request 默认执行 trim 与空字符串转 null；Session 自己的 `put`、`flash`、`now` 不再做这类规范化，会保留传入字符串的空白与空串。示例模块对自身偏好另有限定类型与 256 UTF-8 字节业务限制，不能将它误认为全局 Session 限制。

`Session` 提取器、`RequestInfo::session()`、`Request::session()` 与 `Inject<Session>` 指向同一请求的属性状态。`RequestInfo::session()` 返回可克隆的 `Session` 值，不是借用引用；克隆不会创建另一个会话。没有安装或启用 Session 层时，`has_session()` 为 false，访问 Session 返回 `session_layer_missing`，语义 HTTP 状态为 500；不会隐式创建内存会话。实际响应状态遵循应用统一 HTTP 状态策略。

### null、缺失与键路径

```rust
use openrust::session::Session;
use openrust::support::DataValue;

fn inspect(session: &Session) -> openrust::Result<()> {
    session.put("example.null", DataValue::Null)?;
    let existing_null = session.get("example.null")?;       // Some(Null)
    let missing = session.get("example.absent")?;          // None
    let kept_null = session.get_or("example.null", DataValue::from("fallback"))?; // Null
    let typed = session.get_typed::<String>("example.preference")?;
    Ok(())
}
```

| 操作 | 语义 |
| --- | --- |
| `get` / `get_or` / `get_or_else` | 完整顶层字面键优先，其次 dot 路径；只有缺失才使用默认值，null 不使用默认。惰性默认回调不持有属性锁。 |
| `exists(&[...])` | 所有键存在，包括 null；空列表为 true。 |
| `has(&[...])` / `has_any(&[...])` | 所有键 / 至少一个键存在且非 null；false、0、空串仍算有值。 |
| `put` / `put_many` / `replace` | dot 路径写入；批量按顺序执行并原子替换。`replace` 是合并写入，不清空其他键。 |
| `pull` / `pull_or` | 返回原值并删除；null 与缺失保持区分。 |
| `forget` / `flush` | 删除指定字面键或 dot 路径 / 清空全部属性（包含 token 和 flash 元数据）。 |
| `only` / `except` | 选择顶层字面键 / 从独立快照按删除规则排除；不修改当前属性。 |

这里不展开 `*` 通配符。属性使用 `DataValue` 单一值树；`all()` 和读出的值是快照，对象通过 `Arc` 保留身份，空属性根表示为 `[]`。参数或树超限时，批量变更保持原子性。不要直接管理框架 `_flash` 元数据：手动删除尾项后再追加按当前可见整数键计算，不保存 PHP 数组隐藏的 next-free 历史；普通 flash 与稀疏删除语义见差异说明。

## 便捷读写与计数

```rust
use openrust::support::DataValue;
// 已有非 null 值直接返回；缺失或 null 才调用回调并写回。
let subtotal = session.remember("cart.subtotal", || Ok(DataValue::from(120_i64)))?;
session.push("cart.items", DataValue::from("book"))?;
let quantity = session.increment("cart.quantity")?;
let adjusted = session.decrement_by("cart.quantity", 2)?;
let previous_discount = session.remove("cart.discount")?;
```

`remember` 保留已有 `false`、`0` 和空字符串。回调返回 null 后，下次仍会计算。回调及序列化时不持状态锁，可以调用同一 Session；回调写入其他键会保留，最终计算值覆盖目标键。回调或序列化失败不撤销其自身已经发生的副作用；这不是跨请求的原子锁或分布式“只计算一次”。

`push` 对缺失、null、false 从空数组追加；字符串、整数等非法目标报错，原属性不变。关联数组、稀疏整数键沿同一PHP数组表示规则追加，不复制另一套集合。`remove` 返回被删除的原值；Rust用 `None` 表示缺失、`Some(DataValue::Null)` 表示已有null，与现 `get` / `pull` 一致，PHP上游二者均返回null。

`increment` / `decrement` 默认步长1，`increment_by` / `decrement_by` 接收显式amount并返回、保存结果。数值字符串、null与布尔值按固定上游运算，整数溢出提升为有限浮点；减法保留先对amount乘以-1的顺序。非法类型或Laravel默认会升级成异常的数值警告返回错误，不悄悄把尾垃圾字符串当作数字；非有限数不能由JSON存储，返回错误且不发布部分属性。数组加数组沿PHP左并集，不是数字求和。业务仍应验证允许的输入类型与计数范围。

每次托管写入都检查当前生命周期及完整记录预算后一次发布，保持关闭、忙碌和撤销约束；不能用这些便捷方法恢复已删除的旧认证Session。

### 名称与后端扩展

`session.set_name("checkout_session")?` 改变本请求实际输出的Cookie名称，不改变ID、不自动删除旧名Cookie；本请求启动时仍按原配置读取Cookie。名称受Cookie验证约束，非法名称不会改变原状态。通常应在应用 `session.cookie` 配置中选择稳定名称。

`session.handler()?` 返回当前实际 `Arc<dyn SessionStore>`，供可信应用实现 driver 扩展；它不是另一份 store。返回 `Result` 用于传播状态锁错误。直接调用 driver 属于逃生口，不参与托管 Session 的属性、busy/closed/revoked 状态机，也不提供跨操作原子保证；实现者仍须遵守 driver 自身的 Insert/Update 与资源契约。

### 显式选择 ID 与驱动

`set_id`、`set_handler` 是可信应用的显式操作，保持当前属性与 `started` 状态，不自动加载目标记录。不要把客户端任意参数直接传给这些方法。

```rust
use openrust::session::Session;

fn keep_current_binding(session: &Session) -> openrust::Result<()> {
    let id = session.id()?;
    session.set_id(Some(id.as_str()))?;
    let handler = session.handler()?;
    session.set_handler(handler)?;
    Ok(())
}
```

`set_id(Some(id))` 接受大小写敏感的40位 ASCII 字母数字；非法值或 `None` 生成随机 ID。与 Laravel 一样，换 ID 不会自行删除旧记录，也不再生 token；需要登录态迁移时优先使用 `regenerate` / `invalidate` 等明确生命周期方法。初始未加载时 `set_id` 后 `start` 会读取所选记录；已有来源且正在转移到不同目标时再次 `start` 返回 `session_rebind_pending`，避免混合两份认证属性。

`set_handler(Arc<dyn SessionStore>)` 返回传入的 Arc；只有传入新后端时，它才成为配置驱动。框架分别记录逻辑后端与请求内 I/O 实例；传回当前配置 Arc 或当前实际 Arc 均为同绑定操作，保留来源证明，不把请求实例提升为长期配置。返回的请求实例不得跨请求保存。

显式转移保存保留属性，目标只能 `Insert`：已有目标返回 `session_store_write_conflict`，不会覆盖目标或悄悄改成随机 ID。成功后下一次保存使用 `Update`；保存前切回原绑定也恢复原 `Update`。显式重申当前合法 ID 同样保留指定意图，只有系统生成 ID 才允许有限随机碰撞重试。目标碰撞不会永久撤销句柄，可修正目标后再试；已经观测到源记录被删除或过期则拒绝并撤销当前句柄。

跨驱动保存前的源记录检查是尽力检查，检查与创建目标之间仍有并发窗口，不保证跨后端原子撤销或事务转移。这种可信显式复制，与被动使用旧 Cookie 时同绑定 `Update` 的防复活保证不同。不要把历史属性快照作为未来请求的授权依据。

`migrate(true)` / `regenerate(true)` / `invalidate()` 在待转移时、删除任何记录前返回 `session_rebind_pending`；切回原来源或完成授权保存后再执行。成功销毁的 ID 在同一句柄生命周期内禁止回退，最多记录1024个，超过上限在删除前拒绝。所有 setter 与请求 hook 拒绝 busy/closed/revoked，失败不发布部分状态。

### 请求内驱动绑定

普通 `SessionStore` 不需要请求。需要 Cookie 输入或响应 Cookie 队列的 driver 可以覆盖以下默认方法：

| 方法 | 契约 |
| --- | --- |
| `needs_request()` | 默认 `false`，普通驱动不会收到绑定回调。 |
| `bind_request(context)` | 默认 `Ok(None)`；需要请求的驱动必须返回 `Some(Arc<dyn SessionStore>)`，表示同一逻辑后端的本请求实例。 |

`SessionRequestContext::new(cookies, jar)` 校验 Cookie 快照的树和64 KiB预算；`cookie(name)`按精确名称读取，支持 PHP 根数组的数字键，不展开点路径；`cookie_jar()`返回同一请求的公开 `CookieJar`。上下文 Debug 脱敏，不持有 body、Session 或 Scope。驱动不得把上下文或可变请求保存到应用级单例。

请求中间件在 `start` 前自动调用 `set_request_on_handler(context).await`。手动构造的 Session 需要自己完成绑定；需请求但未绑定时 I/O 返回 `session_request_unbound`，绑定返回 `None` 则为 `session_request_binding_invalid`。`handler_needs_request()?` 用于查询能力；显式更换配置驱动后须重新绑定需要请求的驱动。hook 在状态锁外执行且忙碌时立即拒绝，不排队等待；绑定不改变逻辑后端或清除已有来源证明。

受管理HTTP绑定/加载失败清理请求队列，取消不后台补存，已发生外部副作用不自动回滚。请求绑定接口也供内置Cookie driver使用；其双Cookie、到期、保护与删除边界见本指南“客户端Cookie driver”，不代表全部driver生态或全局撤销已完成。

## flash 的保存时序

| 请求 | 操作与可读结果 |
| --- | --- |
| A | `flash("example.notice", "已保存")` 后立即可读；A 保存后为下一请求保留。 |
| B | 可读该提示；若不调用 `keep` 或 `reflash`，B 保存时删除。 |
| C | 提示已经缺失。 |

`now` 也立即可读，但本次保存就删除。`keep(&["example.notice"])` 只延长指定键；空列表不等于保留全部。`reflash()` 延长所有旧 flash，也会影响其他业务的提示，业务模块应优先保留自己的键。

每次显式 `save().await` 都老化一次，同一请求保存两次可能提前删除 flash；`save` 不是幂等操作。未修改的请求和处理器返回 500 的请求也会保存。Provider 自动保存时，不要为了“确保保存”在处理器末尾重复调用 `save`。

## ID、token 与撤销

本节的防复活、碰撞重试和持久绝对截止属于 Guarded / `ServerSide` 增强语义；原生数据库遵循前述 handler exists、活动期限和零行 UPDATE 规则。

构造 `Session::new(id, store, expiry)` 时 ID 已立即可见；ID 为大小写敏感的 40 位 ASCII 字母数字。合法 ID 没有持久记录时保留此 ID，属性重新开始。`start` 为缺失或 null 的 `_token` 生成同样长度与字符集的随机 token；已有 false、0、空串等非 null 值按 Store 规则保留。token 的存在不代表请求已经通过 CSRF 验证。

| 操作 | ID / token / 属性 |
| --- | --- |
| `migrate(destroy).await` | 更换 ID，保留 token 与属性；destroy=true 时先撤销旧记录。 |
| `regenerate(destroy).await` | 更换 ID 并再生 token，保留其他属性。 |
| `invalidate().await` | 清空属性、撤销旧 ID 并换 ID；不立即再生 token。需要立即使用时显式 `regenerate_token()`，或由后续 `start` 生成。 |
| `SessionStoreHandle::delete(id).await` | 通过同一个 store 撤销指定 ID，不写出凭据。 |

读取过的记录被删除或过期后，迟到保存必须得到 `Missing`，不能变成一次新插入来恢复旧认证属性；当前句柄进入撤销状态，后续保存或换 ID 也会拒绝。新记录的 Insert 碰撞最多尝试三次，然后安全失败。这些是防止并发撤销后复活的安全增强，不是普通请求的完整冲突检测。

数据库截止列或 Redis 剩余 TTL 是持久授权上界，记录 envelope 不能把截止延长到其之后。数据库保存将精度规范为微秒；认证的持久检查复用同一存储。ID/token 是凭据，不放进响应、URL 或日志。

独立构造的 Session 可以使用 `Expiry::OnSessionEnd`、`Expiry::OnInactivity(time::Duration)` 或 `Expiry::AtDateTime(time::OffsetDateTime)`；未提供策略时使用 120 分钟截止。受管理请求默认按最终 `lifetime` 设置无活动期限。`set_expiry` / `expiry_date` 管理持久截止，不自动改写 Provider 的 Cookie 期限配置，`OnSessionEnd` 也不等于修改 `expire_on_close`。

## 失败、取消与并发边界

- 响应发出前保存失败，会替换原业务响应并清空该响应的 Cookie 队列，不发送先前的业务或 Session Set-Cookie。业务已经提交的数据库事务、发送的外部消息等副作用不会因此回滚；应用应按自身业务事务与幂等机制设计。
- handler 被取消时不后台补保存；请求结束或取消会关闭句柄，泄露到请求之外的克隆不能继续修改或保存。已取得的快照不能作为未来请求的授权依据。
- 同一句柄进行 I/O 时同步变更返回 `session_operation_in_progress`；取消 I/O 会回收门控，不承诺外部 store 已执行的写入自动回滚。
- 普通同 ID 并发请求仍可能后写覆盖先写（last writer wins），包括各自的 flash 快照。当前不提供请求阻塞锁、属性级合并或 compare-and-swap 版本控制。

常见错误包括 `session_layer_missing`、`session_closed`、`session_operation_in_progress`、`session_store_write_conflict`（均为语义 500）与 `session_limits_exceeded`（413）。属性限制为最多 1024 项、64 层、20 MiB 键预算，框架元数据也计入；完整记录另受 `max_bytes` 约束，包含值的实际编码字节。资源错误不截断、不吞为匿名成功。

## 示例与当前范围

[示例处理器](../../modules/example/http/controllers/session.rs) 与[模块 HTTP 回归](../../modules/example/tests/session_flows.rs) 使用 `/api/example/session` 的 GET/POST，仅读写示例自己的偏好和提示；支持固定 put/forget/flash/now/keep 操作，不接收任意 Session 键。公开 API 的边界分别见[生命周期测试](../../tests/http/session.rs)、[受管理 HTTP 测试](../../tests/http/session_http.rs)与[数据库存储测试](../../tests/http/session_store.rs)。这些入口对应不同验证层次，不能互相替代。`http_session` 测试入口挂载同目录的 `session_helpers.rs`、`session_source_cases.rs` 与 `session_extensions.rs` 三个私有模块，保留全部生命周期、便捷方法和扩展回归。使用 `cargo test --no-default-features --features session --test http_session` 运行该入口；环境子进程、数据库和 Redis 测试仍使用各自独立入口。

`Session::flash_input(DataValue)` 将对象或数组写入现有 `_old_input` flash；可用 `get_old_input(None)` 读取完整 bag（缺失为 `[]`），或用 `Some(key)` 在 bag 内按完整字面键优先、其次 dot 路径读取。`get_old_input_or` 仅在缺失时使用默认值，显式 null 保持原值；`has_old_input` 判断整 bag 非空或指定键非 null，false 与 0 均算有值。写入沿唯一 Session 状态机和记录预算，失败不发布部分 flash 元数据。

`openrust::support::DataValue` 是可保持 PHP 键顺序及共享对象身份的值树。常规 `put`、`flash`、`now`、`push`、`flash_input` 写入同一属性树，`all`、`get`、`get_old_input`、`pull`、`only`、`except` 返回共享对象快照。纯数据便利写入显式命名 `*_typed`，JSON 读取显式命名 `*_json`；`get_typed` 同样拒绝对象，绝不把对象当作 null 或缺失。对象由 `DataObject` 提供类型安全的 `as_any` 下转入口，可用 `DataValue::same_object` 比较同一 `Arc` 身份；Debug 和错误不输出对象内容。

`session.serializer = "native"` 是默认纯数据兼容编码策略；它在 `save` 遇对象时拒绝持久化，不声称与 PHP serialize 字节兼容。`"json"` 仅使用对象显式声明的 `json_projection`；未提供投影的对象仍拒绝。JSON 重载将空对象按 PHP 关联数组解码为 `[]`。`Store::save` 先推进 flash 年龄再编码，因此编码失败不会写后端，但本请求内 flash 已老化。第三方 `json_projection` 在返回前的瞬时分配由实现方负责，框架校验返回值与整树预算。

只读 `RequestInfo::old` / `old_or` / `old_all` 使用同一请求 Session；没有 Session 时按缺失或空 bag 处理。拥有正文的 `Request::flash` / `flash_only` / `flash_except` 使用已经规范化的 `all` / `only` / `except` 快照，`Request::flush` 仅闪存空旧输入，不执行 `Session::flush`。默认 `flash` 不过滤密码；应用需显式 `flash_except(&["password"])`。当前输入不合并上传文件，不能把这里的旧输入称为完整文件输入。Precognition 跳过自动 Session 保存时，本请求的闪存不保证持久化。

当前Session属性、flash、旧输入、错误bag、redirect闪存与Web CSRF已接入同一请求机制；这些子范围不表示完整Laravel Web组、请求阻塞、全部driver生态或所有响应缓存语义已完成。记录到期不保证立即物理清除，内存store不作持久层。


## 请求启动时回收过期记录

`session.lottery = [2, 100]` 默认每次请求以 2% 概率回收。分母必须非零，分子不得大于分母；`[0, 100]` 禁用，`[1, 1]` 每次执行。抽签在 Session 成功启动后、业务前运行，调用本请求已绑定的实际驱动；传入最终 `lifetime` 的秒数。

外部驱动覆盖 `SessionStore::gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>>`，返回实际删除数量。原生数据库按 `last_activity <= 当前秒数 - lifetime_seconds` 删除；Guarded 按 `expires_at <= 当前时间`，不缩短显式长截止。Memory 在同一锁内清扫；Cookie 返回 0，Redis 由服务端 TTL 回收。

Guarded 的绝对期限与条件写入属于已分类增强，不是 Laravel 数据库默认语义。SQLite 增强 GC 在同一 `BEGIN IMMEDIATE` 所有权内分批读取截止并作 UTC 比较；不依靠文本日期排序或秒精度近似。它不是分布式请求锁。

GC 错误阻止业务与保存，返回脱敏且禁止缓存的错误响应；取消关闭本请求句柄并清理本请求 Cookie 队列，不后台继续。已在外部后端完成的删除不回滚。


原生索引按固定 Blueprint 命名规则从实际物理表名生成：默认 `sessions_user_id_index` 与 `sessions_last_activity_index`，ASCII 转小写并将点和横线替换为下划线。`SessionMigration` 创建这两个索引，默认清单保留 `SessionGcMigration` 活动索引升级。增强截止索引用独立 `GuardedSessionGcMigration`，在 `GuardedSessionMigration` 后显式登记；同名异列索引正常失败。迁移支持表名前缀与完整名称映射，不删除已有记录。
