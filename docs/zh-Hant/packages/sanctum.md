# Sanctum

## 權杖資料表遷移 {#token-table-migration}

使用 Cargo 的 `sanctum` 功能編譯應用程式，註冊 `personal_access_tokens` 遷移。遷移和復原均使用含有此功能的執行檔。

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

遷移建立多型歸屬欄位、唯一的 64 字元權杖雜湊欄位、可為空值的能力和時間欄位，以及到期時間索引。目前功能負責註冊遷移。權杖驗證與 `install:api` 整合另有交付批次。
