# 資料庫

## 設定

應用程式從 `config/database.rs` 讀取資料庫設定，預設連線為 `sqlite`。請先建立 `database/database.sqlite`，也可設定 `DB_DATABASE=:memory:` 使用臨時資料庫。SQLite 相對路徑以應用程式根目錄為基準。記憶體資料庫使用單一實體連線，預設使用私有快取，可在 URL 中明確啟用共用快取。

MySQL 使用 `DB_CONNECTION=mysql`，並設定 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。應用程式透過 `DB_URL` 設定連線的 `url`，由它提供驅動及連線憑證。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 啟用憑證及主機名稱驗證，也可在連線設定中指定 `ssl_mode` 和 `ssl_ca`。

### 讀寫連線

在 `config/database.rs` 的具名連線中設定 `read` 和 `write`。每個角色繼承共用設定，並覆寫自己的欄位。MySQL 在建立連線池設定時打亂主機清單，主機無法連線時會嘗試下一個。密碼由環境提供。

```json
{
  "driver": "mysql",
  "database": "app",
  "username": "app",
  "read": { "host": ["replica-1", "replica-2"] },
  "write": { "host": "primary" },
  "sticky": true
}
```

查詢預設走讀庫。寫入和交易走寫庫。開啟 `sticky: true` 後，同一次 HTTP 請求或 Artisan 執行中，修改資料後的查詢走寫庫。直接使用 `DatabaseManager` 時，連線複本共用路由狀態，可用 `forgetRecordModificationState` 清除。用 `selectFromWriteConnection` 將單次查詢送往寫庫。


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

### 使用多個資料庫連線

用 `DB::connection(Some("reporting"))` 按名稱選擇連線。用 `::read` 和 `::write` 後綴直接選擇對應角色，例如 `DB::connection(Some("reporting::read"))`。

## 資料庫交易

預設連線使用 `DB::transaction`，具名連線直接呼叫 `transaction`。在回呼中透過傳入的交易控制代碼執行 SQL。該控制代碼獨占一條寫庫實體連線，直到提交或復原。Rust 用裝箱的 Future 在非同步查詢期間借用交易控制代碼。第二個參數是總嘗試次數，傳 `1` 表示執行一次。並行錯誤可能重跑整個回呼，請將外部副作用放在交易之外，或確保可安全重複執行。

```rust
use openrust::database::{Connection, DatabaseResult, Value};

pub async fn add_note(connection: &Connection) -> DatabaseResult<Value> {
    connection
        .transaction(
            |transaction| {
                Box::pin(async {
                    transaction
                        .insert("INSERT INTO notes (body) VALUES (?)", &["hello".into()])
                        .await?;
                    transaction
                        .scalar("SELECT COUNT(*) FROM notes WHERE body = ?", &["hello".into()])
                        .await
                })
            },
            3,
        )
        .await
}
```

手動控制時呼叫 `beginTransaction().await?`，然後在傳回的控制代碼上呼叫 `commit` 或 `rollBack(None)`。在控制代碼上再次呼叫 `beginTransaction` 會建立儲存點。`rollBack(Some(level))` 復原到指定層級，層級零復原整個交易。捨棄活動的控制代碼會復原尚未提交的操作。MySQL 的 DDL 等陳述式可能隱含提交，請放在要求不可分割性的交易之外。
