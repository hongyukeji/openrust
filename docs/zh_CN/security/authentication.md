# 身份认证

## Guard 与用户提供器

`auth.guards` 配置选择 Guard 及其提供器。`session` Guard 使用当前 Web 请求的 Session。受保护路由使用 `auth` 中间件别名，访客路由使用 `guest`。通过 `Auth::guard(request, None)?.user().await?` 读取身份。每个请求独立保存已解析身份与选中的 Guard。

## 手动认证用户

用应用选定的查询列和密码创建 `Credentials`。`attempt` 验证凭据，按配置升级密码哈希，成功后轮换 Session ID 和 CSRF 令牌。登录处理器可使用 `attemptThrottled`，限流键由规范化账号和可信客户端地址生成。限流器原子记账，达到上限返回带 `Retry-After` 的 429，验证通过后清除计数。

```rust
use openrust::{
    auth::{AuthResult, Credentials},
    http::Request,
    support::facades::Auth,
};

async fn sign_in(
    request: &mut Request,
    credentials: &Credentials,
    throttle_key: &str,
    remember: bool,
) -> AuthResult<bool> {
    Auth::sessionGuard(request, None)?
        .attemptThrottled(credentials, remember, throttle_key, 5, 60)
        .await
}
```

## 记住用户与登出

将 remember 参数设为 `true` 会签发加密、HTTP-only 的 Cookie。Guard 使用持久化 remember 令牌和当前密码哈希的指纹恢复会话。`logout` 清除当前 Guard 并轮换令牌。若应用还需要丢弃整个会话并轮换 CSRF 令牌，使用 `logoutAndInvalidate`。

## 自定义 Guard 和提供器

默认应用注册编译后的 Eloquent `User`。用 `AuthManager::registerModel` 注册其他模型，或为 `database` 提供器配置表名和列名。用 `provider` 注册提供器，用 `viaRequest` 注册无状态请求解析器。内置 `token` Guard 读取 Bearer 请求头，支持对令牌哈希后再查库。
