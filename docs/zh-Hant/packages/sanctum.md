# Sanctum

## 權杖資料表遷移 {#token-table-migration}

使用 Cargo 的 `sanctum` 功能編譯應用程式，註冊 `personal_access_tokens` 遷移。遷移和復原均使用含有此功能的執行檔。

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

遷移建立多型歸屬欄位、唯一的 64 字元權杖雜湊欄位、可為空值的能力和時間欄位，以及到期時間索引。目前功能負責註冊遷移。權杖驗證與 `install:api` 整合另有交付批次。

## 簽發個人存取權杖 {#issuing-api-tokens}

在應用程式資料庫上使用 `PersonalAccessTokens` 建立權杖，傳入歸屬型別、歸屬 ID、名稱、能力和選用的到期時間。簽發時將傳回的 `plain_text_token` 交給用戶端。資料庫保存 SHA-256 摘要。`findToken` 查找憑證，`revokeToken` 依歸屬刪除權杖。`can` 支援精確能力及萬用字元 `*`。HTTP 守衛將在驗證請求時檢查到期時間。

```rust
use openrust::{auth::AuthResult, database::Connection, sanctum::{NewAccessToken, PersonalAccessTokens}};

async fn issue(connection: Connection, user_id: u64) -> AuthResult<NewAccessToken> {
    PersonalAccessTokens::new(connection, "")
        .createToken("users", user_id, "browser", &["reports:read"], None)
        .await
}
```
