# Socialite

Socialite 将第三方登录接入应用的 HTTP Client 与 Session。已验收范围现包括固定 Socialite 5.33.0 对应的 GitHub、GitLab、Bitbucket、LinkedIn（旧式／OpenID）、Slack（identity／OpenID）、X／Twitter OAuth2、Google 及 Facebook。Twitter OAuth1 与下方本地账户登录示例同样可用。完整 OIDC RP 与 SSO 由 API-O01／02 独立交付。

## 安装 {#installation}

构建或运行应用时启用可选 Cargo feature `socialite`。包发现注册 `SocialiteServiceProvider` 与应用内的 `SocialiteManager`，不会自动创建路由或本地用户表。

```sh
cargo run --features socialite -- artisan serve
```

## 配置 {#configuration}

通过 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 配置 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 开头的回调拼接到合法绝对 HTTP(S) `app.url`，绝对回调保留原值。`services.github.scopes` 向驱动默认值追加 scope。缺失或非法配置明确失败。真实供应商验收需已注册的 GitHub OAuth 应用和对应回调。


以下驱动同样配置 client id、client secret 和 redirect；环境变量前缀后接 `_CLIENT_ID`、`_CLIENT_SECRET`、`_REDIRECT_URI`：

| 驱动／services 键 | 环境变量前缀 | 默认 scopes |
| --- | --- | --- |
| `gitlab` | `GITLAB` | `read_user` |
| `bitbucket` | `BITBUCKET` | `email` |
| `linkedin` | `LINKEDIN` | `r_liteprofile r_emailaddress` |
| `linkedin-openid` | `LINKEDIN_OPENID` | `openid profile email` |
| `slack` | `SLACK` | `identity.basic,identity.email,identity.team,identity.avatar` |
| `slack-openid` | `SLACK_OPENID` | `openid email profile` |

`services.gitlab.host`／`GITLAB_HOST` 默认 `https://gitlab.com`。`GitlabProvider::setHost` 保留自托管路径前缀并去掉末尾斜杠。固定上游资料路径为 `/api/v3/user`，本批保留此契约。Slack 登录配置与现有 `slack.notifications` 共处同一 services 对象；真实供应商仍需分别注册回调并获得权限。

`x` 使用 `X_CLIENT_ID`、`X_CLIENT_SECRET`、`X_REDIRECT_URI`。`twitter` 使用对应的 `TWITTER_` 变量，并设置 `TWITTER_OAUTH=2` 选择 OAuth2；默认值 1 选择 Twitter OAuth1；只有整数 2 选择 OAuth2，字符串 `"2"` 仍选择 OAuth1。显式 `twitter-oauth-2` 驱动先读取 `services.twitter`，仅在对象缺失或为 null 时回退 `services.twitter-oauth-2`。`x` 同样从 `services.x` 回退 `services.x-oauth-2`；已存在但不完整的配置会失败，不静默改用另一组凭据。

`google` 读取 `GOOGLE_CLIENT_ID`、`GOOGLE_CLIENT_SECRET`、`GOOGLE_REDIRECT_URI`，默认 scope 为 `openid profile email`。普通 access token 以 Bearer 调用 Google UserInfo，发送 `prettyPrint=false`。刷新响应仅在缺少 refresh token 字段时保留传入值；明确的 null 或轮换值按响应保留。

`facebook` 使用 `FACEBOOK_CLIENT_ID`、`FACEBOOK_CLIENT_SECRET`、`FACEBOOK_REDIRECT_URI`，默认 scope 为 `email`，固定 Graph 版本为 `v23.0`。`provider.usingGraphVersion("v99.0")` 修改当前请求的授权／token／profile 路径。`fields(vec!["id".into(), "name".into()])` 替换 Graph 字段；`asPopup()`、`reRequest()` 覆写相应可选授权参数。Graph 请求发送 `access_token`、fields，并在 client secret 非空且不等于 `"0"` 时发送 HMAC-SHA256 `appsecret_proof`。授权码交换不指定 Accept，返回保留 `expires`，仅在 `expires_in` 缺失或为 null 时补入，不覆盖已有值。

## 认证 {#authentication}

在应用 web 组注册跳转和回调 GET 路由。下方回调返回远程身份；映射应用用户及登录本地 Guard 由应用完成。远程账户按供应商和供应商 ID 识别。

```rust
use openrust::{
    contracts::container::Container as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_socialite::SocialiteManager;

pub fn redirect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let response = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .redirect().await.map_err(|_| HttpError::InvalidRequest)?;
        Ok(response.into())
    })
}

pub fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>().map_err(|_| HttpError::InvalidRequest)?;
        let user = manager.driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .user().await.map_err(|_| HttpError::InvalidRequest)?;
        Response::json(StatusCode::OK, serde_json::json!({
            "id": user.getId(), "name": user.getName(), "email": user.getEmail(),
        }))
    })
}
```

`driver("github", request)` 与 `with("github", request)` 借用当前请求，用户缓存仅存于该借用期，不在 manager 全局缓存。`redirect` 返回原生跳转响应。Session 保存随机 40 字符 state，回调先消费它再交换 code；缺失、不匹配、数组、空值及重放均失败。`enablePKCE()` 可选；在跳转与回调两次 provider 上同时启用，保存 96 字符 verifier 并发送 S256 challenge 与 verifier。

manager 与 facade 返回 `Provider`，共同入口为 `redirect().await?` 和 `user().await?`。OAuth1 跳转前需通过 HTTP 获取临时凭据，因此统一跳转入口异步。使用 `scopes`、`stateless`、`refreshToken` 等 OAuth2 方法前，执行 `manager.driver("github", request)?.oauth2()?`；Twitter 的 `scope` 和 `userFromTokenAndSecret` 则使用 `.oauth1()?`。协议不符明确返回错误。具体 OAuth2 provider 仍保留同步 `redirect()`。

Twitter OAuth1 使用可选 `services.twitter.scope`／`TWITTER_SCOPE` 或 `.scope("write")`，与 OAuth2 的 `scopes` 分开。scope 非空且不等于 `"0"` 时，临时凭据请求通过 query 发送 `x_auth_access_type`。Session `oauth.temp` 保存临时凭据；回调要求同时存在 `oauth_token` 和 `oauth_verifier`，HTTP 交换前严格核对临时标识，读取但不消费保存的凭据。临时 token 的单次使用由远端服务器执行。此跳转／回调流程要求 Session，OAuth1 不提供 `stateless()`。

## 可选参数 {#optional-parameters}

跳转或交换 code 前调用 `provider.with(serde_json::json!({"prompt":"select_account","login_hint":"alice@example.test"}).as_object().unwrap().clone())`。参数按固定上游顺序覆盖普通字段，支持字符串、布尔和数字标量。`redirectUrl` 修改当前 provider 的回调。


Slack identity 默认使用 `user_scope`，同时将 `scope` 设为空。在跳转和回调的 provider 都执行 `provider.asBotUser()` 后，使用 `scope` 及 token 响应顶层凭据；普通用户凭据取自 `authed_user`。默认 user-scope 字段在 `with` 参数后应用，与上游一致。`BitbucketProvider` 另有显式 `provider.getAccessToken(code).await?` 路径，会追加 HTTP Basic 客户端认证；普通 `user()` 使用继承的 token 响应路径。

## 访问范围 {#access-scopes}

GitHub 默认 `user:email`。`scopes(["read:user".into()])` 去重合并，`setScopes(vec!["read:user".into()])` 替换；`getScopes()` 读取列表。GitHub 使用逗号分隔。包含 `user:email` 时查询 `/user/emails` 并选择 primary 且 verified 的邮箱，失败或无匹配返回 `None`；移除该 scope 则保留 profile 的邮箱。


GitLab、Bitbucket、LinkedIn 两个驱动用空格分隔 scopes；Slack identity 用逗号，Slack OpenID 用空格。Bitbucket 选择类型为 `email` 且 primary、confirmed 的邮箱，邮箱接口失败返回 `None`。旧式 LinkedIn 分别加载本地化资料和邮箱，含 `r_liteprofile` 时请求 `vanityName`，邮箱请求失败正常传播。

## 无状态认证 {#stateless-authentication}

调用 `user()` 前执行 `provider.stateless()`，适用于调用方明确在 Session 之外管理认证状态的场景。它关闭 state 校验，不会创建本地用户或 Session。启用 PKCE 后仍需 Session，与上游两个独立开关一致。

X／Twitter OAuth2 在跳转和回调默认启用 PKCE，因此即使 `stateless()` 也需要 Session。默认 scope 为 `users.read users.email tweet.read`，授权查询使用 RFC3986 编码。stateless 模式在可选参数之后写入字面 `state=state`，回调仍关闭 state 检查。授权码交换同时使用 HTTP Basic 与表单 client secret；刷新使用 Basic，表单只有 `grant_type`、`refresh_token`、`client_id`，不含表单 client secret。用户映射读取 `data.confirmed_email`，不采用未验证邮箱字段。

## 读取用户详情 {#retrieving-user-details}

用户提供 `getId`、`getNickname`、`getName`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 数字／字符串类型。`userFromToken(token)` 使用已有 token 读取身份，`refreshToken(token)` 交换刷新令牌。凭据显式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字符串使用 `Zeroizing<String>`，不会自动序列化或由 Debug 输出。远程 access token 不是 Passport／Sanctum 凭据。


旧式 LinkedIn 增加 `first_name`、`last_name`、`avatar_original`，按首选 locale 和上游严格的 100／800 像素选择图片。LinkedIn OpenID 增加 given／family name、`email_verified` 和原始图片；Slack 两种驱动暴露 `organization_id`。`linkedin-openid`、`slack-openid` 按固定包使用 access token 访问固定 UserInfo 接口，不验证 ID Token、不执行 Discovery，也不是完整 OIDC RP；这些属于 API-O01。

Google 按固定驱动将长度大于 100 字节且恰有两个句点的 token 识别为 JWT。它读取固定 Google JWKS 端点，通过采用 AWS-LC 的 `jsonwebtoken` 11.1.0 校验密钥算法和签名，要求 issuer 严格等于 `https://accounts.google.com`，audience 为与 client ID 相同的单一字符串。存在的 `exp`／`nbf` 与 `iat` 回退按固定 PHP NumericDate 行为验证。JWT 验证失败不会退回 Bearer UserInfo。此 Socialite 路径按上游接受缺少 `exp`、数字日期字符串与 null 日期，不执行 Discovery、不要求 nonce，也不是完整 OIDC RP；API-O01 有独立协议范围。Google raw 数据还补入 `id`、`verified_email`、`link`，并映射 `avatar_original`。

Facebook Limited Login 从未验证的 token header 检测非 null 的 `kid`，随后要求所选 Facebook JWKS 密钥的 RS256 签名、严格的 `https://www.facebook.com` issuer、单一 client audience 与匹配 nonce。在 `userFromToken(token).await?` 或回调 `user()` 前调用 `provider.withNonce(expected)`；未显式指定时，从 `with` 参数的标量 `nonce` 读取期待值。显式 `withNonce` 优先，nonce 必须来自应用保存的授权尝试。`withNonce` 只设置期待值，不自动生成、存储或添加授权 nonce。签名／声明／nonce 失败不会退回 Graph。请求选项与 `lastToken()` 留在当前 provider，保留的 token 使用 zeroize 清理。此行为对应固定 Socialite Limited Login（日期可选），不替代 API-O01 的完整 RP 协议。

### 登录已绑定的本地账户

以稳定的 `(provider, provider_user_id)` 查找已有本地账户。namespace 来自可信应用配置，标识供应商／issuer；不同安装或租户使用不同 namespace。远端 ID 作为不透明标识保留，`007`、`7`、`ABC` 和 `abc` 可以代表不同账户。已验证邮箱本身不授权绑定账户。此示例拒绝未知绑定，应用通过独立的认证与验证流程处理注册或绑定。

建立二进制唯一键与外键表。`user_id` 类型须与本地 `users.id` 相同，以下采用默认无符号 BIGINT 主键：

```sql
CREATE TABLE social_identities (
    provider VARBINARY(128) NOT NULL,
    provider_user_id VARBINARY(255) NOT NULL,
    user_id BIGINT UNSIGNED NOT NULL,
    UNIQUE (provider, provider_user_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
```

将以下动作放入应用动作目录，在回调 `user().await?` 成功后调用 `login_bound_user(request, "github", &user).await?`。返回 false 时响应 403，否则跳转到固定本地账户页。向 `AuthManager` 注册已有用户模型，并为 `auth.guards.web` 配置其 Eloquent provider。动作使用原生 Session Guard，登录时轮换 Session 标识并替换 CSRF token。绑定查询不创建账户、不更新邮箱、不保存远端 access token。

```rust
use openrust::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::DB,
    http::{HttpError, Request},
};
use openrust_socialite::two::User;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Application recipe: existing bindings only; namespace comes from trusted application configuration.
pub async fn login_bound_user(
    request: &mut Request,
    namespace: &str,
    remote: &User,
) -> Result<bool, HttpError> {
    let subject = match remote.getId() {
        serde_json::Value::String(id) if !id.is_empty() => id.clone(),
        serde_json::Value::Number(id) if id.is_i64() || id.is_u64() => id.to_string(),
        _ => return Ok(false),
    };
    if subject.len() > 255 {
        return Ok(false);
    }
    let Some(binding) = DB::table("social_identities")
        .map_err(internal)?
        .whereExactText("provider", namespace)
        .whereExactText("provider_user_id", &subject)
        .first()
        .await
        .map_err(internal)?
    else {
        return Ok(false);
    };
    let id = AuthIdentifier::try_from(binding.get("user_id").ok_or(HttpError::InvalidRequest)?)
        .map_err(internal)?;
    {
        let mut guard = Auth::sessionGuard(request, Some("web")).map_err(internal)?;
        if guard
            .loginUsingId(&id, false)
            .await
            .map_err(internal)?
            .is_none()
        {
            return Ok(false);
        }
    }
    request.session_mut()?.regenerateToken()?;
    Ok(true)
}
```

接入多个供应商时，在跳转时保存所选 namespace，回调前消费并核对；为各供应商注册固定路由，让应用控制回调 namespace。保留正常 state 校验，不接受回调传入本地用户 ID。本地退出使用受 CSRF 保护的 POST 路由，执行 `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`，结束当前应用 Session；供应商 token 撤销与联邦退出遵循各自协议。

MySQL 8.4 验收应用使用上述动作、真实 Eloquent 用户、数据库 Session、HTTP token／profile 端点和两个隔离 schema。覆盖同邮箱不合并、供应商／subject 大小写及数值区别、并发登录、唯一绑定、固定 Session 拒绝、恢复、CSRF／退出轮换、跨应用 Cookie、缺失／删除账户、错误／重放 state、远端撤销及数据库失败。此为受控供应商应用整合验收，真实供应商账户授权仍需凭据与登记回调。

## 测试 {#testing}

通过 `GithubProvider::withEndpoints` 配合应用内 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供应商，替换授权／token／profile／email URL 并保留驱动行为。`ProviderDefinition` 支持编译型协议定制，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公开契约。验收执行真实 HTTP、两个独立 Session 应用、PKCE、state 消费、scope／参数映射、已验证邮箱、刷新和失败路径，固定 PHP 源码 SHA 保护对照夹具。

Socialite 使用共享客户端显式 `redactPayload()`：观察者与录制保留状态及路径，省略 query、authorization、请求体、响应体和响应头；实际网络与返回响应完整。供应商状态／非法响应错误不打印响应凭据。受控验收不冒称真实 GitHub 登录，后者仍需 OAuth 应用、凭据、回调域名和账户。


`ProviderEndpoints::new(authorize, token, user).email(email)` 可传给新增驱动的 `withEndpoints` 构造器，显式替换端点。各驱动均用真实受控 HTTP 验收 token 表单、认证头、资料查询参数、可选邮箱、Slack user／bot 凭据、refresh 与失效／撤销凭据拒绝。六个驱动的映射、scope 及 Slack 专属字段与真实固定 PHP 方法对照。受控结果不冒称已登录真实 GitLab、Bitbucket、LinkedIn 或 Slack 账户。

X／Twitter 测试使用真实 HTTP 与加密 Session Cookie，覆盖默认 PKCE、跨应用拒绝、state 重放、RFC3986 参数、Basic 交换／刷新、错误响应及远程撤销。映射、字段和 HTTP 选项以固定 PHP 方法提供对照。真实 X／Twitter 授权仍需供应商应用凭据、登记回调和账户。

`GoogleProvider::withEndpoints(endpoints, jwks)` 用于受控 UserInfo 与 JWKS 服务。真实 HTTP 测试使用原 PHP Socialite／Firebase JWT 方法对照 25 组签名 JWT，包括错误签名、issuer／audience、缺失或错误声明、未来／过期时间、算法混淆、密钥轮换和跨 client 隔离。PEM 测试密钥仅属于这些夹具。真实 Google 账户授权仍需测试应用和回调。

`FacebookProvider::withEndpoints(endpoints, jwks)` 为受控测试替换三个 Graph／授权端点与 JWKS；包含 `/v23.0/` 的路径随 `usingGraphVersion` 变化。21 组原 PHP Limited Login 对照覆盖 nonce 优先级、issuer／audience、签名／过期、缺失密钥及轮换；实际 Graph HTTP 验证 HMAC proof、字段、过期转换、请求隔离及脱敏观察。真实 Facebook 授权仍需应用、凭据、登记回调及账户。

Twitter OAuth1 发送 HMAC-SHA1 签名请求，访问凭据另含显式 `user.credentials.tokenSecret`。`userFromTokenAndSecret(token, secret).await?` 直接调用 profile。回调映射包含 URL 字段、location、description 与 `avatar_original`；凭据直取仅保留 League extra 字段，与上游一致。成功 profile 按凭据对缓存在当前请求内，任一凭据变化就重新获取。`one::TwitterProvider::withEndpoints` 与 `manager.extendOAuth1` 提供受控端点。固定 League 1.12.0／Socialite PHP 对照及真实 HTTP／Session 测试覆盖签名、空 verifier、scope、凭据缓存、跨应用拒绝、远端重放拒绝和失败脱敏；真实 Twitter 授权仍需应用、凭据、登记回调及账户。
