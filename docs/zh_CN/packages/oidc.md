# OpenID Connect（扩展）

## 安装 {#installation}

可选 `openrust-oidc` 包提供 OpenID Connect RP，支持授权码＋PKCE S256、Discovery、JWKS 与 UserInfo。启用根 Cargo 特性 `oidc` 并重建，包发现会注册 `OidcManager` 与 `Oidc` facade。应用自行注册固定 Web 登录与回调路由，默认不开放端点。Passport 保持 OAuth2 授权服务器范围，不自动成为 OIDC Provider。

```sh
cargo +1.94.0 build --locked --features oidc --bin main
```

## 供应商配置 {#configuration}

`config/oidc.rs` 提供 `oidc.providers.work`。设置 `OIDC_ISSUER`、`OIDC_CLIENT_ID`、`OIDC_CLIENT_SECRET` 与 `OIDC_REDIRECT_URI`；`OIDC_REPLAY_STORE` 选择重放缓存。不设置 secret 时使用公共客户端认证，否则默认 `client_secret_basic`。需要时显式将 `client_auth` 设为 `client_secret_post` 或 `none`。登记回调 URL 必须精确一致。配置示例：

```json
{
"providers": {
    "work": {
        "issuer": "https://identity.example.com",
        "client_id": "registered-client",
        "client_secret": "read-from-environment",
        "redirect": "https://app.example.com/oidc/callback",
        "scopes": ["openid", "profile", "email"],
        "algorithms": ["RS256"],
        "replay_store": "database"
    }
}
}
```

## 授权与回调 {#authorization}

登录与固定回调路由使用同一配置名。`redirect()` 与 `user()` 均为异步方法；provider 持有请求可变借用，每次回调新建 provider。`prompt("login")` 请求重新认证，`maxAge(60)` 要求足够近期的 `auth_time`。同一 Session 为同名 provider 新建授权会替换上一次尝试。

```rust,ignore
use openrust_oidc::facades::Oidc;

// Inside an async handler; convert errors to your application's HTTP errors.
let response = Oidc::provider("work", request)?.redirect().await?;

// Inside the fixed callback handler.
let authentication = Oidc::provider("work", request)?.user().await?;
let identity = &authentication.identity;
```

## 已验证身份 {#identity}

`Authentication.identity` 是经过签名及声明校验后才能构造的 `VerifiedIdentity`。`issuer()` 与 `subject()` 构成稳定身份对，另提供 `email()`、`emailVerified()`、`claims()` 和 `userInfo()`。UserInfo 的 subject 必须精确等于已验证 subject。邮箱改变时不继承 ID Token 的验证标记，须由本次 UserInfo 自己给出布尔 `email_verified: true`。`Authentication.tokens` 显式暴露零化的 access／ID／可选 refresh token 与可选过期秒数，不实现 Debug／Serialize。refresh-token grant 不属于此 RP 客户端 API。

## 本地账户绑定 {#local-binding}

按二进制 `(issuer, subject)` 查找已有本地账户，不按邮箱自动合并，不接受回调传入本地用户 ID。示例使用默认无符号 BIGINT 用户主键；未知绑定返回 false，注册与绑定采用独立的已认证流程。将以下动作放入应用，向 `AuthManager` 注册已有 Eloquent 用户模型，在 `user()` 成功后执行 `login_bound_user(request, &authentication.identity).await?`，false 响应 403。登录使用原生 Session Guard，轮换 Session 标识和 CSRF token。

```sql
CREATE TABLE oidc_identities (
    issuer VARBINARY(2048) NOT NULL,
    subject VARBINARY(255) NOT NULL,
    user_id BIGINT UNSIGNED NOT NULL,
    UNIQUE (issuer, subject),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
```

```rust
use openrust::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::DB,
    http::{HttpError, Request},
};
use openrust_oidc::VerifiedIdentity;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Application recipe: existing bindings only; issuer and subject come only from the verified identity.
pub async fn login_bound_user(
    request: &mut Request,
    remote: &VerifiedIdentity,
) -> Result<bool, HttpError> {
    let Some(binding) = DB::table("oidc_identities")
        .map_err(internal)?
        .whereExactText("issuer", remote.issuer())
        .whereExactText("subject", remote.subject())
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

## 校验边界 {#validation}

RP 要求 HTTPS、精确 issuer、客户端 audience、非空且不超过 255 字节的 ASCII subject、nonce、`exp` 和 `iat`。多个 audience 要求匹配 `azp`，只要存在 `azp` 就必须匹配。校验 `nbf`、可选 `at_hash`／`c_hash` 及请求的 `auth_time`。要求非空且唯一的 `kid`，核对 JWK 用途与算法，仅接受配置的非对称算法。默认 RS256，可显式选 RS384/512、PS256/384/512、ES256/384；拒绝 HMAC、无签名 token、token 携带的密钥或远端密钥 URL。

Discovery 端点默认限于 issuer 同源。确需跨源时在 `allowed_origins` 显式列出所需 HTTPS origin，最多额外 16 项。拒绝带用户凭据／fragment 的 URL，以及预含保留查询参数的授权端点。不跟随 HTTP 重定向，响应上限 1 MiB。`allow_loopback_http: true` 仅为本地测试放行字面 IPv4／IPv6 回环 HTTP，不放行主机名 HTTP。默认授权 TTL 600 秒（1–3600）、单次请求超时 10 秒（1–60）、ID Token 最大年龄 600 秒（0–86400）、时钟宽限 0 秒（0–60）、启用 UserInfo。多进程／副本必须选择共享原子数据库或 Redis 缓存；array 仅保护单进程。失败回调也会消费尝试，重试须重新登录。本地 Session 退出不会撤销 OP token 或结束 OP Session。

## 浏览器 SSO 与退出 {#sso-logout}

API-O02 验证两个独立 RP 应用连接同一受控 OP：客户端 ID、应用密钥、数据库和本地用户各自隔离。首次在 OP 登录后，第二个 RP 可借助已有 OP 浏览器 Session 授权；每个 RP 仍拥有独立本地 Session，并分别解析 `(issuer, subject)`。`prompt("login")` 请求 OP 重新认证，`maxAge` 另行检查返回的 `auth_time`。

RP 发起退出遵循 OpenID Connect RP-Initiated Logout 1.0（2022-09-12）。将 `OIDC_POST_LOGOUT_REDIRECT_URI`（`post_logout_redirect`）设为 OP 已登记的固定回调。在 `user()` 及本地账户绑定成功后，调用 `Oidc::provider("work", request)?.rememberLogout(&authentication)?`，显式将已验证的 ID Token 保存到服务端 Session。`LogoutHint` 是不透明类型，修改公开 token 字段不能替换捕获的已验证提示。Session 存储和发出的退出 URL 均按敏感数据处理。

将以下动作放入应用，并注册为原生 web 组中受 CSRF 保护的 POST 路由。传入 `false` 仅本地退出，`true` 尝试 OP 退出：

```rust
use openrust::{
    auth::Auth,
    http::{HttpError, Request, Response, StatusCode},
};
use openrust_oidc::facades::Oidc;
use serde_json::json;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Register on a POST route in the native web group with CSRF protection.
pub async fn logout(request: &mut Request, federated: bool) -> Result<Response, HttpError> {
    let hint = Oidc::provider("work", request)
        .map_err(internal)?
        .takeLogoutHint()
        .ok();
    Auth::sessionGuard(request, Some("web"))
        .map_err(internal)?
        .logoutAndInvalidate()
        .await
        .map_err(internal)?;
    if federated
        && let Some(hint) = hint
        && let Ok(url) = Oidc::provider("work", request)
            .map_err(internal)?
            .logoutUrl(&hint)
            .await
    {
        return Ok(
            openrust::support::facades::Redirect::away(&url, Default::default())
                .map_err(internal)?
                .into(),
        );
    }
    Response::json(StatusCode::OK, json!({"local":true,"federated":false}))
}
```

`takeLogoutHint()` 先消费提示，原生 `logoutAndInvalidate()` 再清空本地 Session 并更新 CSRF。`logoutUrl(&hint)` 发现并校验获准的 `end_session_endpoint`，附加 ID Token 提示、客户端 ID、固定返回地址和一次性 state，将状态保存到新的匿名 Session。该动作在 OP 不支持、Discovery 不可用或元数据非法时仍完成本地退出。不得从请求参数读取提示或返回目标。

固定返回处理器调用 `Oidc::provider("work", request)?.completeLogout().await?`，再返回本地已退出页面。完成时校验 Session 绑定、配置、目标、期限与共享原子重放声明；失败同样消费本次尝试，且不会登录任何用户。此结果确认返回状态，不证明全部远端凭据均已撤销。

OP 浏览器 Session 结束后，另一个 RP 的既有本地 Session 仍有效。已有 access／refresh token 遵循 OP 独立的撤销策略。本实现不提供 front-channel／back-channel 退出通知及全局单点退出。浏览器验收覆盖并发授权、强制重新认证、issuer／绑定拒绝、CSRF、本地／OP 退出、并发／过期／重放返回状态、OP 不支持及 bearer token 边界。夹具为受控测试 OP，不是生产身份提供方。


## 测试与部署 {#testing}

受控供应商测试使用真实 TCP HTTP、RSA 签名、同 kid 轮换、Session 中间件和 MySQL 8.4。两个 RP 副本在加载同一回调 Session 后，通过共享数据库缓存原子认领 state，只有一个请求调用 token 端点。另覆盖 PKCE／客户端认证、声明／Discovery／UserInfo 失败、过期或替换尝试、跨应用 Cookie、本地绑定和同邮箱不合并。真实第三方授权需供应商 issuer、已登记测试应用、客户端凭据、获准回调与测试账户。API-O02 浏览器结果与退出边界见上文。
