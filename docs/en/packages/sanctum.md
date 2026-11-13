# Sanctum

## Token table migration {#token-table-migration}

Build the application with the `sanctum` Cargo feature to register the compiled `personal_access_tokens` migration. Use the resulting binary for migration and rollback.

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

The migration creates a polymorphic owner, a unique 64-character token hash column, nullable abilities and timestamps, and an expiration index. The feature currently registers the migration. Token authentication and `install:api` integration are tracked separately.

## Issuing personal access tokens {#issuing-api-tokens}

Create a token with `PersonalAccessTokens` on the application database. Supply the owner type, owner ID, name, abilities and optional expiration. Give the client the returned `plain_text_token` once. The database stores its SHA-256 digest. `findToken` resolves credentials, and `revokeToken` deletes a token scoped to its owner. `can` accepts an exact ability or the wildcard `*`. The HTTP guard will check expiration when it authenticates the request.

```rust
use openrust::{auth::AuthResult, database::Connection, sanctum::{NewAccessToken, PersonalAccessTokens}};

async fn issue(connection: Connection, user_id: u64) -> AuthResult<NewAccessToken> {
    PersonalAccessTokens::new(connection, "")
        .createToken("users", user_id, "browser", &["reports:read"], None)
        .await
}
```
