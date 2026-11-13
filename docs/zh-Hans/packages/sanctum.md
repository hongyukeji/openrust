# Sanctum

## 令牌表迁移 {#token-table-migration}

使用 Cargo 的 `sanctum` 功能编译应用，注册 `personal_access_tokens` 迁移。迁移和回滚都使用带有该功能的二进制。

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

迁移创建多态归属字段、唯一的 64 字符令牌哈希列、可空的能力和时间字段，以及过期时间索引。该功能同时注册 Sanctum 守卫和 CSRF Cookie 路由。`install:api` 的 API 骨架另有交付批次。

## 签发个人访问令牌 {#issuing-api-tokens}

在应用数据库上使用 `PersonalAccessTokens` 创建令牌，传入归属类型、归属 ID、名称、能力和可选的过期时间。签发时将返回的 `plain_text_token` 交给客户端。数据库保存 SHA-256 摘要。`findToken` 查找凭据，`revokeToken` 按归属删除令牌。`can` 支持精确能力及通配符 `*`。HTTP 守卫在认证请求前检查过期时间。

```rust
use openrust::{auth::AuthResult, database::Connection, sanctum::{NewAccessToken, PersonalAccessTokens}};

async fn issue(connection: Connection, user_id: u64) -> AuthResult<NewAccessToken> {
    PersonalAccessTokens::new(connection, "")
        .createToken("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

## 保护路由与检查能力 {#protecting-routes}

在受保护的路由上加上 `auth:sanctum`，用 `Authorization: Bearer <token>` 传入凭据。凭据无效、过期或已撤销时返回 HTTP 401。`abilities:reports:read,reports:write` 要求具备两项能力，`ability:reports:read,reports:write` 要求具备任意一项。令牌已通过认证但缺少能力时返回 HTTP 403。

```rust
use openrust::{http::ApiResponse, routing::RoutingError, support::facades::Route};

fn register() -> Result<(), RoutingError> {
    Route::getWith("/reports", || ApiResponse::success(["quarterly"]), |route| {
        route.middleware_name("auth:sanctum").middleware_name("abilities:reports:read");
        Ok(())
    })
}
```

签发时将已注册的 Provider 模型名用作令牌归属类型。默认应用使用 `app::models::User`。可在 `auth.guards` 为自定义 Sanctum 守卫设置 `provider`，省略时使用 Web 守卫的 Provider。用 `AccessToken::currentAccessToken(request)` 查看当前请求凭据。`sanctum.expiration` 以分钟限制令牌最长有效期，与单个令牌的到期时间同时生效。Bearer 认证成功后派发 `TokenAuthenticated` 事件并更新 `last_used_at`，可设 `sanctum.last_used_at=false` 关闭。

## SPA 认证 {#spa-authentication}

在 `bootstrap/app.rs` 现有的 `withMiddleware` 闭包内调用 `middleware.statefulApi();`。用 `SANCTUM_STATEFUL_DOMAINS` 配置以逗号分隔的第一方前端主机，非默认端口须一并写入。使用默认路由配置时，先请求 `GET /sanctum/csrf-cookie`，再通过应用的 Web 会话登录入口登录。后续 API 请求携带 Cookie。需要 CSRF 校验时，把 URL 解码后的 `XSRF-TOKEN` Cookie 值放入 `X-XSRF-TOKEN` 请求头。

跨源第一方前端需要为具体来源开启带凭据的 CORS。匹配的 Referer 或 Origin 会启用 Session、Cookie 和 CSRF 中间件。Sanctum 优先检查已认证的会话，再检查 Bearer 令牌。第一方会话会通过令牌能力检查，用户操作权限由应用授权策略决定。
