# Socialite

Socialite 将第三方登录接入应用的 HTTP Client 与 Session。目前已验收固定 Socialite 5.33.0 对应的 GitHub 驱动和共享 OAuth2 流程。API-S01 整体仍在开发；其他供应商及 OAuth1／OIDC 专属路径待后续交付。

## 安装 {#installation}

构建或运行应用时启用可选 Cargo feature `socialite`。包发现注册 `SocialiteServiceProvider` 与应用内的 `SocialiteManager`，不会自动创建路由或本地用户表。

```sh
cargo run --features socialite -- artisan serve
```

## 配置 {#configuration}

通过 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 配置 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 开头的回调拼接到合法绝对 HTTP(S) `app.url`，绝对回调保留原值。`services.github.scopes` 向驱动默认值追加 scope。缺失或非法配置明确失败。真实供应商验收需已注册的 GitHub OAuth 应用和对应回调。

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

## 访问范围 {#access-scopes}

GitHub 默认 `user:email`。`scopes(["read:user".into()])` 去重合并，`setScopes(vec!["read:user".into()])` 替换；`getScopes()` 读取列表。GitHub 使用逗号分隔。包含 `user:email` 时查询 `/user/emails` 并选择 primary 且 verified 的邮箱，失败或无匹配返回 `None`；移除该 scope 则保留 profile 的邮箱。

## 无状态认证 {#stateless-authentication}

调用 `user()` 前执行 `provider.stateless()`，适用于调用方明确在 Session 之外管理认证状态的场景。它关闭 state 校验，不会创建本地用户或 Session。启用 PKCE 后仍需 Session，与上游两个独立开关一致。

## 读取用户详情 {#retrieving-user-details}

用户提供 `getId`、`getNickname`、`getName`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 数字／字符串类型。`userFromToken(token)` 使用已有 token 读取身份，`refreshToken(token)` 交换刷新令牌。凭据显式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字符串使用 `Zeroizing<String>`，不会自动序列化或由 Debug 输出。远程 access token 不是 Passport／Sanctum 凭据。

## 测试 {#testing}

通过 `GithubProvider::withEndpoints` 配合应用内 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供应商，替换授权／token／profile／email URL 并保留驱动行为。`ProviderDefinition` 支持编译型协议定制，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公开契约。验收执行真实 HTTP、两个独立 Session 应用、PKCE、state 消费、scope／参数映射、已验证邮箱、刷新和失败路径，固定 PHP 源码 SHA 保护对照夹具。

Socialite 使用共享客户端显式 `redactPayload()`：观察者与录制保留状态及路径，省略 query、authorization、请求体、响应体和响应头；实际网络与返回响应完整。供应商状态／非法响应错误不打印响应凭据。受控验收不冒称真实 GitHub 登录，后者仍需 OAuth 应用、凭据、回调域名和账户。
