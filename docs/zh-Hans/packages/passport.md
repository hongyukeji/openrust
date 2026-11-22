# Passport

## 安装 {#installation}

默认应用可执行：

```sh
./artisan install:api --passport --without-migration-prompt --no-interaction
./artisan passport:install --no-interaction
```

API 安装器启用随项目提供的 `passport` Cargo feature，保留其他默认 feature，为默认 User 添加 `HasApiTokens`、配置 `api` Passport guard，并生成及注册 `routes/api.rs`。它重建应用，调用 `passport:install`，再重建已发布的配置和迁移。Cargo feature 与编译承担 Composer 安装的职责，不提供 `--composer`。执行迁移前设置 `APP_KEY` 和数据库连接。默认用户端点使用 `auth:api`，遵守 `API_PREFIX`，支持多级前缀。

`passport:install` 保留已有完整密钥对，发布配置和五个迁移并挂载配置加载器。真实终端中询问运行迁移和创建个人客户端，两项默认 yes；`--no-interaction` 执行两项，每次成功重跑可能再次创建个人客户端。`--without-migration-prompt` 只发布并明确报告迁移待执行，就绪后执行第二条命令；此显式跳过选项是包安装器的原生扩展。没有终端也未传 `--no-interaction` 时报告待执行工作，不等待输入。生产迁移仍需要明确确认，可先执行部署步骤 `artisan migrate --force`，再单独执行 `artisan passport:client --personal --no-interaction` 创建个人客户端。

已有路由、配置及迁移文件会保留。API 的 `--force` 只重写 API 路由 stub，保留密钥；包命令 `passport:install --force --length=2048` 才明确重新生成密钥。密钥对不完整时失败并提示恢复方式。构建、发布、配置、迁移或客户端创建失败均返回非零状态，修正报错后可重试。自定义模型、guard、配置源码缺少可识别插入点时需要手动接线，命令明确报告未完成。已有独立装配的应用也可使用下列手动步骤。


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

包默认提供 `guard = "web"`、空中间件列表，以及可为空的密钥和连接配置；应用配置覆盖包默认值。`PASSPORT_PRIVATE_KEY`、`PASSPORT_PUBLIC_KEY` 支持内联 RSA PEM、字面 `\n` 换行、本地绝对路径或 `file://` 路径（Unix 包括 `file://localhost/`）。相对路径按固定 PHP 本地文件包装器行为拒绝，不会自动绑定到应用根目录。未配置时延迟读取存储目录中的文件。支持 RSA PKCS#1／PKCS#8 私钥及 RSA／SubjectPublicKeyInfo 公钥，不宣称覆盖 League CryptKey 的通用 EC 密钥 API。Debug 和加载错误不包含原始密钥内容。 在线 RS256 使用 AWS-LC，支持 2048–8192 位 RSA 密钥。

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

客户端仓储支持查询、创建、更新、秘密轮换、所有权筛选和撤销，操作真实 Eloquent 模型。`passport:client` 可创建六类客户端。授权码、设备端点及资源令牌认证已提供。

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

`createPersonalAccessGrantClient`、`createPasswordGrantClient`、`createClientCredentialsGrantClient`、`createImplicitGrantClient` 和 `createDeviceAuthorizationGrantClient` 选择对应授权类型。创建记录不会启用可选的 Password 或 Implicit 协议；设备端点遵循应用设备开关。`update` 修改名称和重定向地址，`regenerateSecret` 生成新秘密；两者经 Eloquent 保存并返回其布尔结果。`findForUser` 和 `forUser` 排除已撤销客户端，后者按名称排序。`personalAccessClient(provider)` 选择对应用户提供者的最新有效个人客户端，仅当提供者等于 `auth.guards.api.provider` 时也接受空提供者，找不到时返回错误。

机密客户端复用应用已有的 Eloquent 异步哈希服务。创建或轮换的 `Client` 在 `plainSecret` 临时保留 setter 输入，重新读库不会恢复明文。默认 JSON 隐藏 `secret`，不追加明文。只在向所有者交付时复制新生成的秘密，不要记录日志。`record.model::<M>()` 返回的 `Arc<tokio::sync::Mutex<M>>` 指向实际选定模型；`sameInstance` 检查共享对象身份，不代表两行模型的主键相等。

`client.append(&["plain_secret"])` 显式追加该实例的临时明文；重新读库的模型追加后得到 null。追加仍遵守 `makeHidden` 和 `setVisible`。

`ClientModel::hasScope` 和 `ClientRecord::hasScope` 在未设置 `scopes` 或其原始值为 null 时允许所有 scope；已设置时按数组内容检查，成员判断沿用 PHP 非严格比较。默认不开启父 scope 继承。若在当前应用上调用 `passport.withInheritedScopes.store(true, std::sync::atomic::Ordering::Relaxed)`，`orders` 也允许 `orders:read` 等后代。`*` 在这里是普通 scope 名称。开关保存在各应用自己的实例中，调用须位于所属应用的请求或 Artisan 生命周期。

在应用装配时、解析仓储前调用 `passport.useClientModel::<CustomClient>()`。编译期类型实现 `ClientModel` 和 Eloquent `Model`，保留唯一标识校验、全局作用域、事件及转换。格式错误的 UUID／ULID 在解析连接前直接未命中。仓储查询和未绑定实例的 `Client::query()`／`save()` 优先使用模型显式连接，然后是 `passport.connection`，最后是数据库默认连接。创建和读出的模型保留实际连接；随后更改配置也不会把该模型的保存转到其他连接。`Client::queryOn(&connection)` 显式选择连接。事务写入检查模型的有效连接，拒绝不同连接的事务。Rust 的 `resolveConnectionName()` 返回可拥有的名称，以读取应用配置；`getConnection()` 返回实际连接。秘密赋值使用 PHP 标量字符串转换。哈希失败保留原有哈希，并在 `plainSecret` 中保留本次 setter 输入。批量赋值沿用 Rust 既有的属性键顺序；后续 setter 失败时保留之前成功的赋值。

## 客户端工厂

`Client::factory()?` 创建 `database::factories::ClientFactory<Client>`。可调用 `asPublic`、`asPasswordClient`、`asPersonalAccessTokenClient`、`asImplicitClient`、`asClientCredentials` 或 `asDeviceCodeClient` 应用相应授权状态。后续状态覆盖先前属性。默认生成机密授权码客户端，包含随机 40 字符秘密和一个生成的重定向 URI；所有者为空，revoked 为 false。

`make().await?` 返回未保存模型，`create().await?` 持久化模型。赋值执行模型的异步修改器和转换：新 Client 保留 `plainSecret`，重新读库只包含哈希。`state` 添加应用属性；`intoFactory()` 返回现有 Eloquent 工厂 Builder，以使用数量、关系与回调。

Rust 在编译期选择工厂模型：自定义 `ClientModel` 使用 `ClientFactory::<CustomClient>::new()?`，不动态读取 `useClientModel`。构造时解析并保存模型的有效连接，包括 `passport.connection`，配置错误通过 Result 返回；修改配置后应新建工厂。当前提供授权码、刷新与设备协议。

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
            "user": params.user.getAuthIdentifier(),
            "scopes": params.scopes,
        }))
    })
}
```

批准时向 POST `/oauth/authorize` 提交 `auth_token` 和 Session CSRF `_token`；拒绝使用 DELETE。浏览器必须保留 Session Cookie。服务端一次性取出已保存的请求和随机令牌，并核对当前认证主体；表单中的客户端、回调和 scope 不能替换它们。批准前重新核对当前客户端和注册地址；拒绝会向已验证地址返回 `error=access_denied` 和原始 `state`。

客户端必须生成不可预测的新 `state`，回调时先核对它，再接受授权码或错误。机密客户端兑换时使用 HTTP Basic，或在正文提交 `client_id`、`client_secret`；正文凭据优先，查询串不提供兑换参数。成功响应包含 `token_type`、`expires_in`、`access_token`，允许刷新的客户端还获得 `refresh_token`，并带 `Cache-Control: no-store`、`Pragma: no-cache`。访问令牌使用 RS256。授权码十分钟后过期，消费与令牌保存属于同一事务，并发兑换只有一个成功。访问／刷新期限分别默认为一个日历年，可按下文定制。

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

Rust 在 Session 保存已验证的原生请求，通过编译回调提供视图，不使用 PHP 对象序列化。授权码和刷新令牌使用应用现有认证加密并绑定应用上下文，内部密文格式不与 PHP 互通。客户端／授权码锁和单个数据库事务保证消费及访问／刷新保存原子完成。自定义令牌模型沿用实际连接、Eloquent cast 和事件；保存被取消或数据库失败都会回滚签发。

## 设备授权 {#device-authorization-grant}

用 `./artisan passport:client --device --public` 创建公开设备客户端，机密设备客户端省略 `--public`。设备授权默认开启。若要关闭端点，在 Passport 装配路由之前的 Provider boot 阶段设置 `passport.deviceCodeGrantEnabled.store(false, std::sync::atomic::Ordering::Relaxed)`；应用启动后修改开关不会移除已注册路由。

向 POST `/oauth/device/code` 提交 `client_id` 和可选的空格分隔 `scope`。响应包含 `device_code`、`user_code`、`verification_uri`、`verification_uri_complete`、`expires_in`（600 秒）和 `interval`（5 秒）。创建步骤只识别客户端，不验证秘密。设备保存 `device_code`，向用户显示验证 URI 和用户码，或在浏览器打开完整 URI。响应带 `no-store` 和 `no-cache`。

在应用装配时配置 `passport.deviceUserCodeView(code_view)` 与 `passport.deviceAuthorizationView(consent)`。`code_view` 的签名为 `fn(&mut Request) -> HttpFuture<'_, Result<Response, HttpError>>`，同意回调复用上文 `authorizationView` 的 `AuthorizationViewParameters` 签名。应用负责渲染 HTML，并转义客户端及 scope 显示值。GET `/oauth/device` 展示输入页，GET 表单将 `user_code` 提交回该地址。输入码仅移除连字符，保留大小写。`verification_uri_complete` 跳过输入，重定向到 GET `/oauth/device/authorize`，该端点要求配置的 Session 用户登录并展示同意页。

同意表单携带保存的 `auth_token` 与 Session CSRF `_token`，提交至 POST `/oauth/device/authorize` 批准，或 DELETE 拒绝。浏览器保留 Session Cookie。服务器将请求绑定到实际登录用户，仅消费一次 nonce，已决定的设备码不能被竞争浏览器重新绑定。完成后重定向到 `/oauth/device`，Session `status` 为 `authorization-approved` 或 `authorization-denied`。错误用户码返回输入页，带旧 `user_code` 输入及验证错误；缺少视图回调会明确失败。

设备向 POST `/oauth/token` 轮询，提交 `grant_type=urn:ietf:params:oauth:grant-type:device_code`、`device_code`、`client_id`。机密客户端还须通过 Basic 或正文秘密认证。至少等待响应指定的间隔：HTTP 400 `authorization_pending` 表示等待用户，HTTP 400 `slow_down` 要求降低轮询频率，两者都会保存最近轮询时间。拒绝返回 HTTP 401 `access_denied`，过期返回 HTTP 400 `expired_token`。成功兑换在一个事务内保存访问／刷新令牌并撤销设备码，重放失败。资源守卫、scope 与期限沿用普通访问／刷新流程。`passport.path` 同样作用于全部设备端点。`DeviceCode` 是真实 Eloquent 模型，含 scope、布尔及日期转换，并回退到 Passport 连接。

## 刷新令牌 {#refreshing-tokens}

向 `/oauth/token` POST `grant_type=refresh_token`、`refresh_token` 和客户端凭据。公开客户端提交 `client_id`；机密客户端还须验证 secret。省略 `scope` 保留原 scope，也可请求其子集；按 OAuth 参数规则，空参数使用原 scope。新增 scope 会被拒绝。

成功刷新会撤销旧访问／刷新令牌并返回新的一对。轮换和保存处于同一数据库事务；并发请求只有一个成功，保存失败或被事件取消时保留旧凭据。访问令牌过期或被单独撤销后，刷新令牌仍可兑换；单独撤销刷新令牌不会撤销对应访问令牌。

访问／刷新期限分别默认为一个日历年。在签发前配置本应用的 `Passport` 实例；`None` 读取期限，`Some` 更新期限。`GrantLifetime::Interval(DateInterval)` 通过框架 Date 支持有符号日历分量，保留应用时区、夏令时和 PHP 月份溢出规则。

```rust
use openrust_passport::GrantLifetime;
passport.tokensExpireIn(Some(GrantLifetime::Seconds(3600)));
passport.refreshTokensExpireIn(Some(GrantLifetime::CalendarYears(1)));
let current = passport.tokensExpireIn(None);
```

## 撤销令牌 {#revoking-tokens}

对 Eloquent 实例调用 `TokenModel::revoke` 或 `RefreshTokenModel::revoke`。对当前认证请求，克隆 `TokenGuard::token(request, "api")` 后调用 `token.revoke().await?`，更新选定的访问令牌存储。仓储撤销客户端仍按前述 refresh → access → client 顺序执行。

## 清理令牌 {#purging-tokens}

运行 `artisan passport:purge`，删除已撤销和过期超过 168 小时的记录。`--revoked` 或 `--expired` 仅选择一个条件，同时传入时按 OR 合并；`--hours=24` 修改过期保留期限。若访问令牌仍关联未撤销且期限为空或晚于当前时间的刷新令牌，则保留该访问记录。授权码和刷新令牌同时清理；设备授权启用时也清理设备码。自定义访问／刷新模型的实际表、连接及查询作用域参与执行。

## 保护路由 {#protecting-routes}

在 `config/auth.rs` 将守卫配置为 `{"driver":"passport","provider":"users"}`，为受保护路由添加 `auth:api`。Provider 须解析应用编译后的用户模型。每个请求验证 RS256 签名、可选 JWT 时间声明和选定访问存储的撤销状态。客户端失效、provider 不匹配或用户不存在时视为未认证；用户查询异常也视为未认证，权威令牌存储故障则令请求失败。

通过 `Auth::user(request).await?` 读取真实用户。Rust 将轻量访问令牌按守卫名附到请求，保留原编译用户对象；通过 `openrust_passport::guards::TokenGuard::token(request, "api")` 读取，使用 `can`、`cant`、`transient` 检查 scope。`ResourceServer::validateAuthenticatedRequest` 还插入类型化 `AccessToken` 请求属性。守卫解析仅在本请求内缓存，按守卫及应用隔离。Cookie 认证和 scope 中间件见下文。

## 客户端凭据授权 {#client-credentials-grant}

通过 `artisan passport:client --client` 创建机密机器客户端，向 `POST /oauth/token` 发送 `grant_type=client_credentials`、`client_id`、`client_secret` 和可选的空格分隔 `scope`；也支持 HTTP Basic 凭据。响应包含 `access_token`、`token_type`、`expires_in` 和 `Cache-Control: no-store`，没有刷新令牌。公开客户端被拒绝。JWT subject 为客户端 ID，数据库 user ID 为空。

机器资源使用 `EnsureClientIsResourceOwner::shared(["read"])` 验证令牌和 scope，不要求用户 provider；用户守卫仍要求真实用户。通过 `passport.clientCredentialsTokensExpireIn(Some(GrantLifetime::Seconds(300)))` 设置机器期限，未单独配置时沿用 `tokensExpireIn`。

## 个人访问令牌 {#personal-access-tokens}

使用 `artisan passport:client --personal --provider=users` 创建个人客户端。在应用上下文调用 `PersonalAccessTokenFactory::make(user_id, "Automation", &["read".into()], "users").await?`。个人授权走专用服务器，公网令牌端点不接受它。按 provider 选择客户端；无 provider 的客户端仅能作为已配置 API provider 的回退。

编译用户模型可以实现 `HasApiTokens`，在 `getProviderName` 明确返回其配置 provider，再调用 `user.createToken("Automation", &["read".into()]).await?`。Rust 要求显式声明，不推断 PHP 模型类名。令牌保存用户、名称和 scope，无刷新令牌。`personalAccessTokensExpireIn` 默认独立为一个日历年。

`PersonalAccessTokenResult` 的序列化、`toArray`、`toJson` 提供 `accessTokenId`、`accessToken`、`tokenType`、`expiresIn`。`getToken().await?` 加载并缓存真实默认 `Token`，返回 `Option<PersonalToken<Token>>`，即 `Arc<tokio::sync::Mutex<Token>>`；重复读取共享同一模型。编译自定义模型使用 `getTokenFor::<CustomToken>()`。锁定模型后通过 `TokenModel` 撤销。Debug 输出隐藏 Bearer。

## 令牌作用域 {#token-scopes}

使用 `tokensCan(BTreeMap::from([("read".into(), "读取数据".into())]))` 注册 scope 描述；`setDefaultScope(vec!["read".into()])` 设置省略 `scope` 时的默认值。显式未知 scope 被拒绝，签名前应用客户端 scope 限制；机器令牌可以保留 `*`。`defaultScopes(None)` 读取，`defaultScopes(Some(scopes))` 更新，协议层规范化空格分隔字符串。

`scopeIds`、`hasScope`、`validScopes` 和 `scopesFor` 查询配置。`validScopes` 按输入顺序过滤并去重，`scopesFor` 保留重复并过滤未知 ID。`hasScope("*")` 为真，但 `validScopes` 仅在显式配置时保留 `*`。

```rust
use openrust_passport::http::middleware::{CheckToken, CheckTokenForAnyScope};
route.middleware(CheckToken::shared(["read", "write"]));
route.middleware(CheckTokenForAnyScope::shared(["read", "write"]));
```

原生中间件值替代 PHP 中间件字符串。`CheckToken` 要求全部 scope，`CheckTokenForAnyScope` 要求至少一个；空的全部列表通过，空的任一列表拒绝。未认证返回 401，scope 不足返回 403、`Invalid scope(s) provided.`。原生错误类型位于 `exceptions::MissingScopeException`。`TokenGuard::currentToken(request)` 读取选定守卫的令牌，第一方瞬态令牌允许全部 scope。

## SPA 认证 {#spa-authentication}

将共享的 `CreateFreshApiToken::using(Some("web"))` 放入 web 栈，位于 Session 和 Cookie 中间件之后。已认证 GET 响应追加加密 HttpOnly `openrust_token` Cookie，已有同名响应 Cookie 时保留原值。内部签名 JWT 包含真实用户标识、CSRF 和期限，期限及 Cookie 属性沿用 Session 配置。`passport.cookie(Some("custom_name"))` 修改名称。

API 资源使用 Passport 守卫。API 栈通常不含 web `EncryptCookies`，由 Passport 自行解密；如果应用已经提前解密，应显式调用 `withoutCookieEncryption`。`withCookieEncryption`、`withCookieSerialization`、`withoutCookieSerialization`、`encryptTokensUsing` 和 `tokenEncryptionKey` 配置生产与消费一致的格式和密钥。设置归属应用实例；相同密钥和格式的应用可能互读 Cookie。

通过 `X-CSRF-TOKEN` 发送 Session CSRF，或用 `X-XSRF-TOKEN` 发送加密 XSRF Cookie，即使 Cookie API 请求为 GET 也要求匹配。错误签名、过期、CSRF 或用户不存在均未认证。显式 Bearer 优先，失败时不回退 Cookie。`ignoreCsrfToken(true)` 为显式绕过，默认关闭。

`POST /oauth/token/refresh` 经过 web Session 和 CSRF，返回 200 `Refreshed.` 并刷新 Cookie。Cookie 认证生成请求内瞬态令牌，不保存数据库访问令牌。web 登出路由也须安装 `CreateFreshApiToken`，通过前后身份变化清除此 Cookie；这是替代 PHP 全局请求登出监听器的原生适配，未安装的路由不会自动清除。

## 事件 {#events}

通过框架分发器监听类型化 `events::AccessTokenCreated`、`RefreshTokenCreated` 和 `AccessTokenRevoked`。载荷含令牌、客户端和用户 ID，不含 Bearer 或客户端秘密。先保存访问模型并分发创建事件，再保存刷新模型并分发。同步监听器失败回滚签发；`Delivery::AfterCommit` 读取已提交记录。刷新轮换在事务内分发撤销事件，直接调用访问令牌 `revoke` 仅修改存储，不分发该轮换事件。

## 测试 {#testing}

在应用上下文用 `passport.actingAs(&mut request, Box::new(user), &["read".into()], "api", None).await?` 设置用户，或用 `passport.actingAsClient(&mut request, &client, &["read".into()], "api").await?` 设置机器。这些辅助方法选定守卫并将带 scope 的令牌附到显式请求，不签发、不落库，也不影响后续请求。`actingAsClient` 要求请求没有缓存用户，从用户身份切换时创建新请求。真实协议测试仍须获取并提交签名令牌。

## 密码授权 {#password-grant}

Password 和 Implicit 默认关闭，创建对应客户端记录不会启用协议。明确需要 Password 的集成，在解析授权服务器前调用应用实例的 `passport.enablePasswordGrant()`；`passwordGrantEnabled` 属于应用实例，设为 false 后拒绝后续请求。浏览器常规流程仍使用授权码及 PKCE。

通过 `artisan passport:client --password --provider=users` 创建客户端（`--public` 创建公开客户端）。向 `POST /oauth/token` 发送 `grant_type=password`、`client_id`、`username`、`password` 和可选 `scope`；机密客户端还提供 `client_secret` 或 HTTP Basic。成功返回签名访问令牌，客户端支持时也签发刷新令牌，沿用普通访问／刷新期限。错误用户凭据返回 400 `invalid_grant`、`The user credentials were incorrect.`；缺字段、错误客户端和未知 scope 保留 OAuth 错误响应。

`bridge::UserRepository::getUserEntityByUserCredentials` 选择客户端 provider，空值回退到 `auth.guards.api.provider`。默认编译 Eloquent provider 查询 `email` 并验证真实密码哈希。框架自定义 `UserProvider` 通过 `retrieveByCredentials`、`validateCredentials` 提供模型 `findForPassport` 和 `validateForPassportPasswordGrant` 的原生等价职责，不启动 Session、不重新哈希密码。

组合模型钩子 `findAndValidateForPassport` 使用 `passport.findAndValidateForPassportUsing("users", resolver)` 注册。原生 `bridge::PasswordGrantResolver` 是接收借用用户名、密码和 `ClientRecord` 的编译函数，返回 `AuthFuture<AuthResult<Option<AuthIdentifier>>>`；它优先处理该 provider，须执行应用真实查询及验证。显式注册替代 PHP 运行时方法发现。异常返回脱敏 500，不冒充密码错误。

## 隐式授权 {#implicit-grant}

仅在集成明确要求时调用 `passport.enableImplicitGrant()`，再通过 `artisan passport:client --implicit --redirect_uri=https://client.example/callback` 创建公开客户端。向 `GET /oauth/authorize` 发送 `response_type=token`、`client_id` 及可选 `redirect_uri`、`scope`、`state`、`prompt`，沿用 web Session、CSRF、授权视图、用户绑定和一次性同意 nonce。此可选旧流程不替代上文 PKCE 示例。

批准后 302 回到已校验的回调，fragment 包含 `access_token`、`token_type`、`expires_in` 和已提供的 `state`，不创建授权码或刷新令牌。未知 scope、需要登录／同意及拒绝也使用 fragment，错误客户端或不可信回调在本地失败，保留既有回调查询。Implicit 和设备授权的 scope 最终处理移除 `*`；Password、PAT 和客户端凭据保留它，客户端 scope 限制仍参与。

`implicitGrantEnabled` 属于应用实例，设为 false 也拒绝尚未完成的 Implicit 同意请求。原生实现提交前复查有效客户端及注册地址，模型／监听器失败回滚事务。同意 POST 不能修改已保存的客户端、用户、scope 或响应类型。令牌端点始终不接受 `grant_type=implicit`。

## 覆写默认模型 {#overriding-default-models}

在接收请求前，通过应用的 `Passport` 实例调用 `useClientModel::<C>()`、`useTokenModel::<T>()`、`useRefreshTokenModel::<R>()`、`useAuthCodeModel::<A>()` 和 `useDeviceCodeModel::<D>()`。对应的 `clientModel`、`tokenModel`、`refreshTokenModel`、`authCodeModel`、`deviceCodeModel` 返回所选编译类型名称，注册属于各应用。协议持久化、资源验证、撤销和 `passport:purge` 使用真实选定的 Eloquent 模型，不把记录复制到默认表。

客户端、访问和刷新模型分别在 `Model` 之外实现 `ClientModel`、`TokenModel`、`RefreshTokenModel`；授权码和设备码模型实现 `Model`。元数据保留默认模型的 casts、主键行为、guarded／hidden 和时间戳，并提供自己的表结构。例如自定义客户端仍应隐藏 `secret`，注册模型不会修补其序列化策略。模型观察者和取消保存仍生效，授权及令牌消费失败回滚事务；缺表或存储故障正常报错，修正装配后可重试。

自定义关系目标使用 `client.authCodesFor::<A>()`、`client.tokensFor::<T>()`、`code.clientFor::<C>()`、`token.clientFor::<C>()`、`token.refreshTokenFor::<R>()`、`refresh.accessTokenFor::<T>()`。默认辅助方法保留具体默认返回类型。PAT 结果使用 `getTokenFor::<T>()`，工厂通过 `ClientFactory::<C>::new()` 选择类型。

显式模型连接优先于 `passport.connection`，再回退到协议的父连接。同一次签发／消费事务的 OAuth 写入必须共享真实连接；跨连接身份会报错并回滚，即使两个别名指向同一数据库。为 OAuth 表统一设置 `PASSPORT_CONNECTION`。用户 provider 可独立查询另一用户存储。通过 `AuthManager::registerModel` 或自定义 `UserProvider` 注册编译用户模型，配置 guard 的 provider，并保留模型主键与认证标识的区别。Password 的 provider 钩子见前文。

## 覆写路由 {#overriding-routes}

通过 `passport.path` 设置多级 OAuth 前缀，通过 `passport.middleware` 添加共同中间件。需要替换包路由时，在应用装配阶段、载入路由前调用 `passport.ignoreRoutes()`，然后注册公开控制器。例如仅提供令牌端点：

```rust
use openrust::{routing::{RouteGroup, RoutingError}, support::facades::Route};

pub fn oauth_routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().prefix("integrations"), |router| {
        router.post("/token",
            openrust_passport::http::controllers::AccessTokenController::issueToken)?
            .name("passport.token")
            .withoutMiddleware("web")
            .middleware_name("throttle");
        Ok(())
    })
}
```

该无状态令牌路由接受独立 HTTP 客户端，返回标准 OAuth 响应及缓存头。浏览器同意及设备批准路由继续保留 web Session、CSRF、认证、阻塞中间件与包的命名路由。设备授权还要求存在 `passport.device` 命名路由。关闭内置路由不会自动安装这些浏览器端点，也不会修改资源 guard 的 provider。

## 与 Sanctum 共存 {#sanctum-coexistence}

同时启用 `passport`、`sanctum` 默认 Cargo features。分别运行 API 安装器且不传 `--force` 会保留已有 API 路由，为第二种 guard 显式添加资源路由并执行其发布迁移。Passport guard（例如 `api`）配置用户 provider，路由使用 `auth:api`；Sanctum 路由使用 `auth:sanctum`。默认用户可实现两包的 `HasApiTokens`，用完整路径区分同名方法：

```rust
let oauth = openrust_passport::HasApiTokens::createToken(&user, "OAuth", &[]).await?;
let personal = openrust::sanctum::HasApiTokens::createToken(
    &user, &sanctum, &database, "Sanctum", &["read"], None,
).await?;
```

除普通用户 provider 注册外，还通过 `AuthManager::registerSanctumModel` 注册 Sanctum 用户。OAuth JWT 与 Sanctum 个人 Bearer 令牌使用独立表和 guard，不能互换；撤销一种凭据不影响另一包的有效凭据。浏览器 Session 认证单独配置。测试章节的请求内 `actingAs`／`actingAsClient` 仍是指定请求／guard 的显式替身，不安装客户端、不生成持久数据库令牌。
