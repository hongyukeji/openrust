# Socialite

Socialite 将第三方登录接入应用的 HTTP Client 与 Session。已验收范围现包括固定 Socialite 5.33.0 对应的 GitHub、GitLab、Bitbucket、LinkedIn（旧式／OpenID）、Slack（identity／OpenID）及 X／Twitter OAuth2。API-S01 整体仍在开发；Facebook、Google、OAuth1 以及完整 OIDC／本地身份整合待后续交付。

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

`x` 使用 `X_CLIENT_ID`、`X_CLIENT_SECRET`、`X_REDIRECT_URI`。`twitter` 使用对应的 `TWITTER_` 变量，并设置 `TWITTER_OAUTH=2` 选择 OAuth2；默认值 1 保留上游 OAuth1 选择，该路径尚未交付，明确失败。显式 `twitter-oauth-2` 驱动先读取 `services.twitter`，仅在对象缺失或为 null 时回退 `services.twitter-oauth-2`。`x` 同样从 `services.x` 回退 `services.x-oauth-2`；已存在但不完整的配置会失败，不静默改用另一组凭据。

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
            .redirect().map_err(|_| HttpError::InvalidRequest)?;
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

## 测试 {#testing}

通过 `GithubProvider::withEndpoints` 配合应用内 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供应商，替换授权／token／profile／email URL 并保留驱动行为。`ProviderDefinition` 支持编译型协议定制，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公开契约。验收执行真实 HTTP、两个独立 Session 应用、PKCE、state 消费、scope／参数映射、已验证邮箱、刷新和失败路径，固定 PHP 源码 SHA 保护对照夹具。

Socialite 使用共享客户端显式 `redactPayload()`：观察者与录制保留状态及路径，省略 query、authorization、请求体、响应体和响应头；实际网络与返回响应完整。供应商状态／非法响应错误不打印响应凭据。受控验收不冒称真实 GitHub 登录，后者仍需 OAuth 应用、凭据、回调域名和账户。


`ProviderEndpoints::new(authorize, token, user).email(email)` 可传给新增驱动的 `withEndpoints` 构造器，显式替换端点。各驱动均用真实受控 HTTP 验收 token 表单、认证头、资料查询参数、可选邮箱、Slack user／bot 凭据、refresh 与失效／撤销凭据拒绝。六个驱动的映射、scope 及 Slack 专属字段与真实固定 PHP 方法对照。受控结果不冒称已登录真实 GitLab、Bitbucket、LinkedIn 或 Slack 账户。

X／Twitter 测试使用真实 HTTP 与加密 Session Cookie，覆盖默认 PKCE、跨应用拒绝、state 重放、RFC3986 参数、Basic 交换／刷新、错误响应及远程撤销。映射、字段和 HTTP 选项以固定 PHP 方法提供对照。真实 X／Twitter 授权仍需供应商应用凭据、登记回调和账户。
