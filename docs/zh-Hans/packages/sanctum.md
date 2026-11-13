# Sanctum

## 令牌表迁移 {#token-table-migration}

使用 Cargo 的 `sanctum` 功能编译应用，注册 `personal_access_tokens` 迁移。迁移和回滚都使用带有该功能的二进制。

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

迁移创建多态归属字段、唯一的 64 字符令牌哈希列、可空的能力和时间字段，以及过期时间索引。当前功能负责注册迁移。令牌认证和 `install:api` 集成另有交付批次。

## 签发个人访问令牌 {#issuing-api-tokens}

在应用数据库上使用 `PersonalAccessTokens` 创建令牌，传入归属类型、归属 ID、名称、能力和可选的过期时间。签发时将返回的 `plain_text_token` 交给客户端。数据库保存 SHA-256 摘要。`findToken` 查找凭据，`revokeToken` 按归属删除令牌。`can` 支持精确能力及通配符 `*`。HTTP 守卫将在认证请求时检查过期。

```rust
use openrust::{auth::AuthResult, database::Connection, sanctum::{NewAccessToken, PersonalAccessTokens}};

async fn issue(connection: Connection, user_id: u64) -> AuthResult<NewAccessToken> {
    PersonalAccessTokens::new(connection, "")
        .createToken("users", user_id, "browser", &["reports:read"], None)
        .await
}
```
