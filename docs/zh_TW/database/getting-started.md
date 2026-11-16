# 資料庫

## 設定

應用程式從 `config/database.rs` 讀取資料庫設定，預設連線為 `sqlite`。請先建立 `database/database.sqlite`，也可設定 `DB_DATABASE=:memory:` 使用臨時資料庫。SQLite 相對路徑以應用程式根目錄為基準。記憶體資料庫使用單一實體連線，預設使用私有快取，可在 URL 中明確啟用共用快取。

MySQL 使用 `DB_CONNECTION=mysql`，並設定 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。應用程式透過 `DB_URL` 設定連線的 `url`，由它提供驅動及連線憑證。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 啟用憑證及主機名稱驗證，也可在連線設定中指定 `ssl_mode` 和 `ssl_ca`。 `verify_identity` 驗證憑證鏈及伺服器名稱。`verify_ca` 驗證憑證鏈。

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

### 查詢多個結果集 {#selecting-multiple-result-sets}

使用 `DB::selectResultSets` 執行可傳回多個結果集的 MySQL 預備陳述式，例如預存程序呼叫。結果依傳回順序排列，並保留空結果集。MySQL 的 `CALL` 還會在最後傳回一個空的狀態結果集。

傳入 `true` 時遵循正常的讀取連線路由，傳入 `false` 時使用寫入連線。在交易中透過交易物件呼叫此方法，查詢會在交易所在連線上執行。SQLite 對此介面傳回 `DatabaseError::UnsupportedOperation`。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

async fn find_active_users(db: &Connection, active: bool) -> DatabaseResult<Vec<Vec<Row>>> {
    db.selectResultSets("CALL find_active_users(?)", &[active.into()], true).await
}
```

### 使用具名繫結 {#using-named-bindings}

使用 `:name` 預留位置，並透過 `Bindings::Named` 傳入對應的值。鍵可以帶前導冒號，鍵的排列順序無須與 SQL 一致。

為每個具名參數繫結值，每條陳述式使用一種預留位置。SQLite 允許重複使用同一個名稱。使用 MySQL 原生預備陳述式時，每個預留位置使用不同名稱。

```rust
use openrust::{database::{Bindings, DatabaseResult, Row}, support::facades::DB};

async fn find_user(id: i64) -> DatabaseResult<Option<Row>> {
    DB::selectOne(
        "SELECT * FROM users WHERE id = :id",
        Bindings::Named(&[("id", id.into())]),
    ).await
}
```

### 使用多個資料庫連線

用 `DB::connection(Some("reporting"))` 按名稱選擇連線。用 `::read` 和 `::write` 後綴直接選擇對應角色，例如 `DB::connection(Some("reporting::read"))`。

### 連線建立事件 {#connection-established}

透過 `ConnectionEstablished` 初始化應用程式連線。連線解析傳回延遲開啟的控制代碼，第一次非同步操作會在使用連線池前等待事件完成。也可透過 `establish().await` 明確觸發。每個應用程式的具名連線各派發一次。監聽器可直接在事件提供的連線上等待 SQL 執行。 派送傳回監聽器錯誤時，由發起操作接收該錯誤。如果派送在監聽器執行期間被取消，下一次操作會重試派送。

```rust
use std::{error::Error, sync::Arc};
use openrust::{
    contracts::events::Event as EventPayload,
    database::events::ConnectionEstablished,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::{DB, Event},
};

fn connected<'a>(
    payload: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        let event = payload.downcast_ref::<ConnectionEstablished>()
            .ok_or(EventError::InvalidPayload)?;
        println!("Established {}", event.connection.getName());
        Ok(None)
    })
}

async fn subscribe() -> Result<(), Box<dyn Error + Send + Sync>> {
    Event::listen(ConnectionEstablished::NAME, Arc::new(connected), Delivery::Sync)?;
    DB::connection(None)?.establish().await?;
    Ok(())
}
```

### 監聽查詢事件 {#listening-for-query-events}

使用 `DB::listen` 監聽應用程式連線的成功查詢。`QueryExecuted` 包含原始 SQL、繫結參數、讀寫角色、連線和 `Duration` 耗時。資料游標在成功耗盡後派送，計時排除呼叫端在兩列之間處理資料的時間。

### 監控累計查詢耗時 {#monitoring-cumulative-query-time}

使用 `DB::whenQueryingForLongerThan` 監控預設連線，也可在具名連線上註冊。累計耗時超過門檻後，每個處理器執行一次。`resetTotalQueryDuration` 清除耗時。`allowQueryDurationHandlersToRunAgain` 允許處理器再次執行。

```rust
use std::{sync::Arc, time::Duration};
use openrust::{
    contracts::events::Event as EventPayload,
    database::{DatabaseResult, events::QueryExecuted},
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::DB,
};

fn report_query<'a>(
    payload: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        let query = payload.downcast_ref::<QueryExecuted>()
            .ok_or(EventError::InvalidPayload)?;
        println!("Query on {} took {:?}", query.connection.getName(), query.time);
        Ok(None)
    })
}

fn subscribe() -> DatabaseResult<()> {
    DB::listen(Arc::new(report_query))
}

fn monitor_cumulative_time() -> DatabaseResult<()> {
    DB::whenQueryingForLongerThan(Duration::from_millis(500), Arc::new(report_query))
}
```

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

### 交易事件 {#transaction-events}

透過應用程式事件分派器訂閱。`TransactionBeginning` 在交易或儲存點建立後觸發。`TransactionCommitting` 在最外層提交前觸發。`TransactionCommitted` 在交易層級下降並執行該層應執行的提交回呼後觸發。`TransactionRolledBack` 在明確復原或交易回呼失敗引發的復原完成後觸發一次。

仍有作用中層級時，監聽器可透過事件上下文使用目前的交易。最外層提交或復原後，事件包含連線及層級 0。最外層提交後監聽失敗時傳回 `DatabaseError::AfterCommit`，已提交的資料保留。

```rust
use std::sync::Arc;
use openrust::{
    contracts::events::{Event as EventPayload, Listener},
    database::events::TransactionCommitted,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::Event,
};

struct ReportCommit;

impl Listener for ReportCommit {
    fn handle<'a>(
        &'a self,
        payload: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = payload.downcast_ref::<TransactionCommitted>()
                .ok_or(EventError::InvalidPayload)?;
            if event.level == 0 {
                println!("Committed on {}", event.connection.getName());
            }
            Ok(None)
        })
    }
}

fn register() -> EventResult<()> {
    Event::listen(TransactionCommitted::NAME, Arc::new(ReportCommit), Delivery::Sync)
}
```
