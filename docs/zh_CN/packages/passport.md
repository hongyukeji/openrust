# Passport

## 安装 {#installation}

已提供客户端、浏览器授权及同意、授权码／PKCE 和授权码兑换端点。资源令牌守卫、刷新兑换及其他 grant 仍在开发；当前还没有 `install:api --passport`，既有 Sanctum 安装保持独立。

当前应用预留了可选的 `passport` Cargo feature。将它加入 Cargo 默认 features，让 `./artisan` 和普通构建持续启用该包，并保留原有其他默认项：

```toml
[features]
default = ["passport"]
```

包发现会注册 `openrust_passport::PassportServiceProvider`。没有包发现的应用可用 `withProviders` 显式注册。启用 Provider 会注册已实现的 OAuth 路由。使用前须发布执行迁移、设置应用密钥和同意视图，并生成 RSA 密钥。

## 部署 Passport {#deploying-passport}

生成默认 4096 位 RSA 密钥对：

```sh
./artisan passport:keys
```

密钥位于应用配置的存储目录，文件名为 `oauth-private.key` 和 `oauth-public.key`。Unix 权限分别为 0600、0660；Windows 使用文件系统访问控制。密钥已存在时退出码为 1。`--force` 替换已有密钥，`--length=2048` 指定长度。替换后重启服务。并发的非强制生成最多产生一对匹配密钥。

```sh
./artisan passport:keys --force --length=2048
./artisan vendor:publish --tag=passport-migrations
cargo build
./artisan migrate
```

五张表的迁移（包括 `oauth_device_codes`）按迁移时间戳发布。Rust 在编译时发现迁移源码，因此发布后先重新构建，再执行迁移。Provider 不自动注册或运行包迁移。重复发布保留已有文件，除非显式传入 `--force`。状态、回滚和重建命令见[数据库迁移](../database/migrations.md)。

## 配置 {#configuration}

包默认提供 `guard = "web"`、空中间件列表，以及可为空的密钥和连接配置；应用配置覆盖包默认值。`PASSPORT_PRIVATE_KEY`、`PASSPORT_PUBLIC_KEY` 支持内联 RSA PEM、字面 `\n` 换行、本地绝对路径或 `file://` 路径（Unix 包括 `file://localhost/`）。相对路径按固定 PHP 本地文件包装器行为拒绝，不会自动绑定到应用根目录。未配置时延迟读取存储目录中的文件。支持 RSA PKCS#1／PKCS#8 私钥及 RSA／SubjectPublicKeyInfo 公钥，不宣称覆盖 League CryptKey 的通用 EC 密钥 API。Debug 和加载错误不包含原始密钥内容。

需要应用编译配置覆盖时先发布：

```sh
./artisan vendor:publish --tag=passport-config
```

在 `config/mod.rs` 中增加 `mod passport;`，并在配置条目中增加 `("passport".to_owned(), passport::load(env)?),`，随后重新构建。这对应 PHP 载入发布配置的机制。发布的加载器与包默认值使用同一份实现。`config:cache` 保存解析后的值；加载新的环境变量前先清理配置缓存。

`PASSPORT_CONNECTION` 为五个迁移定义选择命名连接；null 使用应用默认连接。迁移账本仍放在迁移命令选定的连接，保持 Laravel 职责。迁移显式读取所属应用配置，并发应用不会串用连接名。

在 Provider boot 期间解析 `Passport`，在首次解析密钥服务前设置目录：

```rust
use openrust::foundation::Application;
use openrust_passport::Passport;

pub async fn configure_keys(app: &Application) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let passport = app.make::<Passport>().await?;
    passport.loadKeysFrom(passport.keyPath("keys"));
    Ok(())
}
```

生成前目录须已存在且可写。Rust 使用应用持有的 `Passport` 实例，替代 PHP 的进程全局可变设置，保留 `keyPath`、`loadKeysFrom` 名称。两个密钥独立延迟加载；仅配置成功不表示两者已经配对。Unix 文件权限不属于 0400、0440、0600、0640、0660 时校验失败。应用可显式调用 `passport.validateKeyPermissions.store(false, std::sync::atomic::Ordering::Relaxed)` 关闭该检查，不影响其他应用。内联密钥和 Windows 不检查 Unix 权限位。

默认及标准长度使用现有原生 AWS-LC 生成，其余受支持长度使用独立生成器。RustCrypto 只用于生成与 PEM 解析，不用于在线 RSA 解密或签名；无需安装 PHP 或 OpenSSL 命令行工具。

## 管理客户端 {#managing-clients}

客户端仓储支持查询、创建、更新、秘密轮换、所有权筛选和撤销，操作真实 Eloquent 模型。`passport:client` 可创建六类客户端。授权码端点已提供；资源令牌认证仍待完成。

在请求或 Artisan 生命周期内解析 `ClientRepository`，容器为每个生命周期提供新仓储。同一参数的重复 `find` 共享实际模型，并缓存未命中；数据库错误传播，允许再次尝试。`findActive` 还检查当前模型的 `revoked` 属性。不要把此作用域服务保存在应用单例中。显式管理的操作可以通过 `Passport::clientRepository()` 创建独立拥有缓存的仓储。

发布并执行客户端迁移后，在上述生命周期内调用以下辅助函数：

```rust
use openrust::foundation::Application;
use openrust_passport::{ClientOwner, ClientRecord, ClientRepository};

pub async fn create_client(
    app: &Application,
    owner: Option<&ClientOwner>,
) -> Result<ClientRecord, Box<dyn std::error::Error + Send + Sync>> {
    let clients = app.make::<ClientRepository>().await?;
    Ok(clients
        .createAuthorizationCodeGrantClient(
            "My application",
            &["https://example.com/callback".to_owned()],
            true,
            owner,
            false,
        )
        .await?)
}
```

参数顺序保留 Laravel 原意。Rust 显式传递默认参数：`true` 创建机密客户端，`owner` 可空，最后的 `false` 不额外启用设备授权。使用 `ClientOwner::forUser(&user)` 从编译注册的 `Model + Authenticatable` 创建所有者，取模型主键和多态类型，不取可能不同的认证标识。公开客户端将机密参数设为 `false`。

`createPersonalAccessGrantClient`、`createPasswordGrantClient`、`createClientCredentialsGrantClient`、`createImplicitGrantClient` 和 `createDeviceAuthorizationGrantClient` 选择对应授权类型。创建记录不等于启用 Password、Implicit 或设备协议端点。`update` 修改名称和重定向地址，`regenerateSecret` 生成新秘密；两者经 Eloquent 保存并返回其布尔结果。`findForUser` 和 `forUser` 排除已撤销客户端，后者按名称排序。`personalAccessClient(provider)` 选择对应用户提供者的最新有效个人客户端，仅当提供者等于 `auth.guards.api.provider` 时也接受空提供者，找不到时返回错误。

机密客户端复用应用已有的 Eloquent 异步哈希服务。创建或轮换的 `Client` 在 `plainSecret` 临时保留 setter 输入，重新读库不会恢复明文。默认 JSON 隐藏 `secret`，不追加明文。只在向所有者交付时复制新生成的秘密，不要记录日志。`record.model::<M>()` 返回的 `Arc<tokio::sync::Mutex<M>>` 指向实际选定模型；`sameInstance` 检查共享对象身份，不代表两行模型的主键相等。

`client.append(&["plain_secret"])` 显式追加该实例的临时明文；重新读库的模型追加后得到 null。追加仍遵守 `makeHidden` 和 `setVisible`。

`ClientModel::hasScope` 和 `ClientRecord::hasScope` 在未设置 `scopes` 或其原始值为 null 时允许所有 scope；已设置时按数组内容检查，成员判断沿用 PHP 非严格比较。默认不开启父 scope 继承。若在当前应用上调用 `passport.withInheritedScopes.store(true, std::sync::atomic::Ordering::Relaxed)`，`orders` 也允许 `orders:read` 等后代。`*` 在这里是普通 scope 名称。开关保存在各应用自己的实例中，调用须位于所属应用的请求或 Artisan 生命周期。

在应用装配时、解析仓储前调用 `passport.useClientModel::<CustomClient>()`。编译期类型实现 `ClientModel` 和 Eloquent `Model`，保留唯一标识校验、全局作用域、事件及转换。格式错误的 UUID／ULID 在解析连接前直接未命中。仓储查询和未绑定实例的 `Client::query()`／`save()` 优先使用模型显式连接，然后是 `passport.connection`，最后是数据库默认连接。创建和读出的模型保留实际连接；随后更改配置也不会把该模型的保存转到其他连接。`Client::queryOn(&connection)` 显式选择连接。事务写入检查模型的有效连接，拒绝不同连接的事务。Rust 的 `resolveConnectionName()` 返回可拥有的名称，以读取应用配置；`getConnection()` 返回实际连接。秘密赋值使用 PHP 标量字符串转换。哈希失败保留原有哈希，并在 `plainSecret` 中保留本次 setter 输入。批量赋值沿用 Rust 既有的属性键顺序；后续 setter 失败时保留之前成功的赋值。

## 客户端工厂

`Client::factory()?` 创建 `database::factories::ClientFactory<Client>`。可调用 `asPublic`、`asPasswordClient`、`asPersonalAccessTokenClient`、`asImplicitClient`、`asClientCredentials` 或 `asDeviceCodeClient` 应用相应授权状态。后续状态覆盖先前属性。默认生成机密授权码客户端，包含随机 40 字符秘密和一个生成的重定向 URI；所有者为空，revoked 为 false。

`make().await?` 返回未保存模型，`create().await?` 持久化模型。赋值执行模型的异步修改器和转换：新 Client 保留 `plainSecret`，重新读库只包含哈希。`state` 添加应用属性；`intoFactory()` 返回现有 Eloquent 工厂 Builder，以使用数量、关系与回调。

Rust 在编译期选择工厂模型：自定义 `ClientModel` 使用 `ClientFactory::<CustomClient>::new()?`，不动态读取 `useClientModel`。构造时解析并保存模型的有效连接，包括 `passport.connection`，配置错误通过 Result 返回；修改配置后应新建工厂。当前仅提供授权码协议。

## 客户端命令与撤销

执行迁移后，用 `./artisan passport:client` 创建客户端。`--personal`、`--password`、`--client`、`--implicit`、`--device` 选择授权类型；不传这些选项时创建授权码客户端。多个选项同时存在时按上述顺序优先。`--name`、`--provider`、`--redirect_uri` 提供交互值，多个重定向地址用逗号分隔。`--public` 为密码、设备和授权码客户端省略秘密。隐式客户端始终公开；个人和客户端凭据客户端始终保密。

使用 `--no-interaction` 时，名称默认取 `app.name`，回调默认取应用 URL 加 `/auth/callback`，附加设备流程默认关闭。交互创建授权码客户端时，若 `deviceCodeGrantEnabled` 为 true（默认值），会询问是否启用设备流程。命令输出客户端 ID；保密客户端还输出临时秘密和保存提醒。个人客户端创建只输出成功消息。

`clients.delete(&client).await?` 按每批 1000 个加载客户端的访问令牌，逐个先撤销刷新令牌、再撤销访问令牌，最后撤销客户端。保存经过 Eloquent 转换和事件；错误向上传递，之前的保存保留。事件取消保存返回 false，不中止撤销链，与 Passport 一致。授权码不变。`TokenModel::revoke`、`RefreshTokenModel::revoke` 也可单独撤销实例。

## 模型与关系

`Token`、`RefreshToken`、`AuthCode` 使用非自增字符串主键、布尔 `revoked`、日期时间 `expires_at` 转换，以及 Passport 连接回退。只有 `Token` 使用时间戳，其 scopes 为数组。`Token::can` 拒绝空 scope 列表，支持 PHP 非严格比较的 `*` 通配符，并读取当前应用的继承设置；`cant` 返回相反结果。

`ClientModel` 提供 `tokens`、`authCodes`、`owner` 和已弃用的 `user::<U>()`；`Token` 提供 `client`、`refreshToken` 和已弃用的 `user::<U>("认证标识列")`；`RefreshToken` 提供 `accessToken`；`AuthCode` 提供 `client`。Token 的用户键须与编译 provider 模型的 `Authenticatable` 标识列一致，Client 的用户关系使用主键。多态所有者通过既有 `owner().target::<U>(别名, 主键)` Builder 绑定。

Rust 在编译时绑定关系返回类型。默认辅助方法返回默认具体模型；自定义模型使用 `tokensFor::<T>`、`clientFor::<C>`、`refreshTokenFor::<R>`、`accessTokenFor::<T>`。`passport.useTokenModel::<T>()` 和 `useRefreshTokenModel::<R>()` 选择仓储撤销时使用的实际模型，保留其连接和事件；共享注册也作用于已解析的仓储。用户 provider 由调用方选择编译模型，替代 PHP 的运行时类名。

`clientUuids` 默认 true。已有数字自增客户端表时，在构造模型前调用 `passport.clientUuids.store(false, std::sync::atomic::Ordering::Relaxed)`。每个实例保留构造时选定的策略。开关不会修改已有表结构；发布的客户端迁移仍创建 UUID 主键。

## Scope JSON

`Scope::toJson()` 保留 `id`、`description` 字段顺序，以及 PHP 默认的 Unicode 和斜杠转义。`toJsonWithOptions(ScopeJsonOptions)` 支持 `JSON_HEX_TAG`、`JSON_HEX_AMP`、`JSON_HEX_APOS`、`JSON_HEX_QUOT`、`JSON_NUMERIC_CHECK`、`JSON_UNESCAPED_SLASHES`、`JSON_PRETTY_PRINT`、`JSON_UNESCAPED_UNICODE`、`JSON_PRESERVE_ZERO_FRACTION`、`JSON_UNESCAPED_LINE_TERMINATORS`。用 `|` 组合常量，`fromBits` 可传入 PHP 选项位。Rust 字符串始终是有效 UTF-8，因此非法 UTF-8 相关标志不会改变结果。

```rust
use openrust_passport::{Scope, ScopeJsonOptions};
let scope = Scope::new("orders/read", "读取订单");
let json = scope.toJsonWithOptions(
    ScopeJsonOptions::JSON_UNESCAPED_UNICODE | ScopeJsonOptions::JSON_UNESCAPED_SLASHES,
)?;
```

## 授权码授权 {#authorization-code-grant}

Provider 注册 GET、POST、DELETE `/oauth/authorize`，使用 `web` 中间件和 Session 锁；POST `/oauth/token` 使用限流。`passport.path` 修改默认 `oauth` 前缀，`passport.middleware` 为这些路由添加中间件。在包启动前调用 `passport.ignoreRoutes()` 可自行注册路由。浏览器认证使用 `passport.guard`，默认为 `web`；配置应用自身的登录路由和 SessionGuard 用户提供者，并先发布执行迁移、设置 `APP_KEY`、生成 RSA 密钥。

在应用装配时解析 `Passport`，调用 `passport.tokensCan(BTreeMap::from([("read".to_owned(), "Read data".to_owned())]))` 注册 scope，按需调用 `passport.setDefaultScope(vec!["read".to_owned()])`，并设置 `passport.authorizationView(consent)`。这些设置属于当前应用。下面的回调为应用界面返回同意数据；界面负责显示客户端、权限和批准／拒绝控件：

```rust
use openrust::http::{HttpError, HttpFuture, Request, Response, StatusCode};
use openrust::contracts::session::Session as _;
use openrust_passport::http::AuthorizationViewParameters;

pub fn consent(request: &mut Request, params: AuthorizationViewParameters)
    -> HttpFuture<'_, Result<Response, HttpError>>
{
    Box::pin(async move {
        Response::json(StatusCode::OK, serde_json::json!({
            "auth_token": params.auth_token,
            "csrf": request.session()?.token(),
            "user": params.user.get_auth_identifier(),
            "scopes": params.scopes,
        }))
    })
}
```

批准时向 POST `/oauth/authorize` 提交 `auth_token` 和 Session CSRF `_token`；拒绝使用 DELETE。浏览器必须保留 Session Cookie。服务端一次性取出已保存的请求和随机令牌，并核对当前认证主体；表单中的客户端、回调和 scope 不能替换它们。批准前重新核对当前客户端和注册地址；拒绝会向已验证地址返回 `error=access_denied` 和原始 `state`。

客户端必须生成不可预测的新 `state`，回调时先核对它，再接受授权码或错误。机密客户端兑换时使用 HTTP Basic，或在正文提交 `client_id`、`client_secret`；正文凭据优先，查询串不提供兑换参数。成功响应包含 `token_type`、`expires_in`、`access_token`，允许刷新的客户端还获得 `refresh_token`，并带 `Cache-Control: no-store`、`Pragma: no-cache`。访问令牌使用 RS256。授权码十分钟后过期，消费与令牌保存属于同一事务，并发兑换只有一个成功。当前访问／刷新期限为十二个日历月；刷新兑换和期限定制在下一生命周期单元交付。

`prompt=none` 在需要交互时返回带 `login_required` 或 `consent_required` 的重定向。已有未撤销、未过期授权可跳过已授予权限的同意页；`prompt=consent` 重新请求同意，`prompt=login` 请求重新登录。无效客户端和未注册回调在本地失败，不返回不可信 Location。OAuth 错误保留协议字段和状态码。

## 使用 PKCE 的授权码授权 {#code-grant-pkce}

用 `passport:client --public` 创建公开客户端，必须提交 challenge；机密客户端也可使用 PKCE。每次生成由字母、数字和 `-._~` 组成的 43–128 字符 verifier，计算 SHA-256 后作无填充 base64url 编码，使用 `S256`：

```text
GET /oauth/authorize?response_type=code&client_id=CLIENT_ID&redirect_uri=https%3A%2F%2Fclient.test%2Fcb&scope=read&state=RANDOM_STATE&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256
```

批准后，用原 verifier 和授权时提供的完全相同的回调地址兑换：

```text
POST /oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&client_id=CLIENT_ID&redirect_uri=https%3A%2F%2Fclient.test%2Fcb&code=AUTHORIZATION_CODE&code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk
```

这些是固定示例值，实际授权必须每次重新生成。缺失／错误 verifier、篡改回调、过期和重放均拒绝签发。授权阶段允许已注册 HTTP loopback 回调改变端口；兑换阶段绑定原来提供的精确字符串。

Rust 在 Session 保存已验证的原生请求，通过编译回调提供视图，不使用 PHP 对象序列化。授权码和刷新令牌使用应用现有认证加密并绑定应用上下文，内部密文格式不与 PHP 互通。客户端／授权码锁和单个数据库事务保证消费及访问／刷新保存原子完成。自定义令牌模型沿用实际连接、Eloquent cast 和事件；保存被取消或数据库失败都会回滚签发。本单元尚不提供资源 Guard、刷新兑换、设备授权、其他 grant 和统一 API 安装器。
