# 資料庫

## 設定

應用程式從 `config/database.rs` 讀取資料庫設定，預設連線為 `sqlite`。請先建立 `database/database.sqlite`，也可設定 `DB_DATABASE=:memory:` 使用臨時資料庫。SQLite 相對路徑以應用程式根目錄為基準。記憶體資料庫使用單一實體連線，預設使用私有快取，可在 URL 中明確啟用共用快取。

MySQL 使用 `DB_CONNECTION=mysql`，並設定 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。應用程式透過 `DB_URL` 設定連線的 `url`，由它提供驅動及連線憑證。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 啟用憑證及主機名稱驗證，也可在連線設定中指定 `ssl_mode` 和 `ssl_ca`。

## 執行 SQL

在應用程式生命週期內使用 `DB::select`、`DB::selectOne`、`DB::scalar`、`DB::insert`、`DB::update`、`DB::delete` 和 `DB::statement`。SQL 使用 `?` 預留位置，參數另行傳入。`selectOne` 傳回可選資料列，`scalar` 在無相符資料列時傳回 SQL null，有結果時須只有一個欄位。`update` 和 `delete` 傳回受影響的列數。`unprepared` 執行原始 SQL，應傳入由應用程式控制的文字。

`DB::connection(Some("name"))` 解析具名連線並共用應用程式內的連線池。查詢非同步執行，參數使用具型別的值。MySQL 無號整數和定點小數保留完整精確度，定點小數結果以文字表示。

```rust
use openrust::{config::Repository, database::{DatabaseManager, Value}};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = DatabaseManager::new(
        Arc::new(Repository::new(Default::default())),
        std::env::current_dir()?,
    );
    let connection = manager.connection(None)?;
    connection.statement("CREATE TABLE IF NOT EXISTS notes (body TEXT)", &[]).await?;
    connection.insert("INSERT INTO notes VALUES (?)", &["A bound value".into()]).await?;
    let value = connection.scalar("SELECT body FROM notes WHERE body = ?", &["A bound value".into()]).await?;
    assert_eq!(value, Value::from("A bound value"));
    Ok(())
}
```
