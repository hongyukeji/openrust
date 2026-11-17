# 資料庫

## 設定

應用程式從 `config/database.rs` 讀取資料庫設定，預設連線為 `sqlite`。請先建立 `database/database.sqlite`，也可設定 `DB_DATABASE=:memory:` 使用臨時資料庫。SQLite 相對路徑以應用程式根目錄為基準。記憶體資料庫使用單一實體連線，預設使用私有快取，可在 URL 中明確啟用共用快取。

MySQL 使用 `DB_CONNECTION=mysql`，並設定 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。應用程式透過 `DB_URL` 設定連線的 `url`，由它提供驅動及連線憑證。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 啟用憑證及主機名稱驗證，也可在連線設定中指定 `ssl_mode` 和 `ssl_ca`。 `verify_identity` 驗證憑證鏈及伺服器名稱。`verify_ca` 驗證憑證鏈。

### MariaDB {#mariadb}

設定 `DB_CONNECTION=mariadb`，主機、資料庫、憑證及 TLS 設定與 MySQL 相同。透過 `DB_URL` 設定時使用 `mariadb://`。若要驗證伺服器憑證及名稱，請將 `MYSQL_ATTR_SSL_CA` 指向伺服器 CA 憑證，並讓 `DB_HOST` 使用憑證所涵蓋的名稱或位址。

MariaDB 方言支援 JSON 欄位更新與由 JSON 產生欄位。`uuid()` 在 10.7 之前使用 `char(36)`，從 10.7 起使用原生 `uuid`。資料庫佇列從 10.6 起使用 `SKIP LOCKED`。實測版本請參閱[發佈矩陣](../prologue/releases.md)。

### SQL Server {#sql-server}

設定 `DB_CONNECTION=sqlsrv`，並設定 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。連線 URL 使用 `sqlsrv://`。OpenRust 使用 Rust TDS 用戶端，需要 TLS 並驗證伺服器憑證。使用私有 CA 時，將 `DB_SSL_CA` 指向簽發憑證，並使用伺服器憑證涵蓋的主機名稱或位址。

實測安裝使用 SQL Server 2017 或 2022 上已建立的資料庫。啟動 HTTP 和資料庫佇列 Worker 前執行 `artisan migrate --force`。實測建置請參閱[發佈矩陣](../prologue/releases.md)。

### MySQL 5.7 TLS {#mysql-57-tls}

MySQL 5.7.44 需要能透過 ECDHE 協商 TLS 1.2 的伺服器建置。Oracle 通用 Linux 封存檔使用 RSA 伺服器憑證及下列 `mysqld` 設定通過驗收。透過 `ssl-ca`、`ssl-cert` 和 `ssl-key` 設定 CA、伺服器憑證及私密金鑰。

```ini
[mysqld]
require_secure_transport=ON
tls_version=TLSv1.2
ssl_cipher=ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384
```

應用程式的 `MYSQL_ATTR_SSL_CA` 指向 CA 憑證，`DB_HOST` 使用伺服器憑證中的名稱或位址。

#### Oracle Linux 7 映像 {#mysql-57-oracle-linux-7}

原版 `mysql:5.7` 映像缺少 OpenRust 所需的伺服器端 ECDHE 設定。從儲存庫根目錄建置衍生映像。它為固定的 MySQL 5.7.44 伺服器啟用臨時曲線選擇，並套用上方的 TLS 設定。

```sh
docker build --platform linux/amd64 -t openrust/mysql57-ecdhe docker/mysql57-ecdhe
```

使用 `openrust/mysql57-ecdhe` 時沿用原映像的資料庫掛載與環境變數。掛載 RSA 伺服器憑證和私密金鑰，並將容器內的路徑傳給 `--ssl-ca`、`--ssl-cert` 和 `--ssl-key`。應用程式透過 `MYSQL_ATTR_SSL_CA` 和 `DB_HOST` 驗證憑證與伺服器名稱。請參閱[發佈矩陣](../prologue/releases.md)。

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

#### 直連別名 {#direct-connection-alias}

對 MySQL 和 SQLite，`DB::connection(Some("mysql::direct"))` 會讓讀寫操作都使用該連線的寫入端點。將 `mysql` 換成實際連線名稱。查詢記錄和事件會以 `QueryRole::Direct` 標示。Laravel 未設定獨立直連端點時也會回退到寫入端點。

PostgreSQL 的 `direct` 設定非空時，`DB::connection(Some("pgsql::direct"))` 會選擇獨立端點。依需要設定 `DB_DIRECT_HOST`、`DB_DIRECT_PORT`、`DB_DIRECT_USERNAME`、`DB_DIRECT_PASSWORD` 與 `DB_DIRECT_SSLMODE`。具名連線的 `direct` 選項繼承其基礎設定。遷移和帳本操作預設使用該端點。明確指定 `::read`、`::write`、`::direct` 後綴時，依呼叫端指定的角色執行。

使用 PgBouncer 交易池模式時，將 `max_prepared_statements` 設為非零值以追蹤具名預備陳述式。OpenRust 使用 SQLx 預備陳述式。讓應用程式與資料庫伺服器的 `search_path` 保持一致，因為連線池可能忽略用戶端啟動選項。遷移繼續使用直連。

#### 中斷、重連及清除連線 {#reconnecting-database-connections}

`DB::disconnect` 退役目前的讀寫連線池，下一次非同步操作依目前設定建立新池。`DB::reconnect` 立即更新連線池，保留邏輯連線與查詢日誌。`DB::purge` 同時清除快取的連線，下次解析會建立新的邏輯連線。

進行中的交易繼續使用原工作階段直到結束。已取得工作階段的操作可以完成，新操作使用替換後的連線池。

```rust
use openrust::{database::DatabaseResult, support::facades::DB};

async fn refresh_database() -> DatabaseResult<()> {
    DB::disconnect(None)?;
    let connection = DB::reconnect(None).await?;
    assert_eq!(connection.scalar("SELECT 1", &[]).await?, 1.into());
    DB::purge(None)?;
    Ok(())
}
```

#### 復原中斷的資料庫連線 {#recovering-lost-connections}

在交易外執行陳述式時，驅動程式回報連線中斷後會更新連線池並重試一次。重試成功只派送一次 `QueryExecuted`。最外層 `BEGIN` 期間中斷連線也可重試一次。

作用中交易的斷線錯誤交給呼叫端。串流游標可在傳回第一列前重試。如果伺服器在回應遺失前已完成寫入，重試可能再次執行該寫入。

### 預演查詢 {#previewing-queries}

用 `DB::pretend` 或指定連線的 `pretend` 收集 SQL。預演只記錄 SQL，讀取操作傳回空結果，寫入操作依方法傳回成功或零影響列。查詢前回呼和查詢事件仍會執行。傳回的項目包含轉換後的 SQL 和原始繫結值，預演記錄獨立於即時查詢記錄。

```rust
use openrust::{
    database::{DatabaseResult, QueryLogEntry},
    support::facades::DB,
};

async fn preview_settings_update() -> DatabaseResult<Vec<QueryLogEntry>> {
    DB::pretend(|connection| {
        Box::pin(async move {
            connection
                .update("UPDATE settings SET enabled = ? WHERE id = ?", &[1.into(), 42.into()])
                .await?;
            Ok(())
        })
    })
    .await
}
```

預演作用於目前非同步範圍中的指定連線。獨立啟動的工作會進入自己的範圍。用 `pretending()` 檢查目前是否在預演，用 `withoutPretending` 在預演中明確執行實際操作。交易的開始、提交及回復需要實際執行範圍。

### 查詢執行前回呼 {#before-executing}

用 `DB::beforeExecuting` 向預設連線註冊，也可以在指定連線上呼叫 `beforeExecuting`。回呼依註冊順序、在資料庫操作之前收到原始 SQL 和繫結值。回呼傳回錯誤會中止本次操作及後續回呼。游標第一次輪詢時執行回呼。驅動程式重播同一次查詢時會略過回呼。

```rust
use openrust::{database::{Bindings, DatabaseResult, Value}, support::facades::DB};

async fn read_value() -> DatabaseResult<Value> {
    DB::beforeExecuting(|sql, bindings, _, _| {
        Box::pin(async move {
            if sql == "SELECT ? AS n" {
                assert!(matches!(bindings, Bindings::Positional([Value::Integer(42)])));
            }
            Ok(())
        })
    })?;

    DB::scalar("SELECT ? AS n", &[42.into()]).await
}
```

最後一個參數是目前的交易控制代碼。回呼需要在同一筆交易內執行操作時，請使用該控制代碼。

### 連線建立事件 {#connection-established}

透過 `ConnectionEstablished` 初始化應用程式連線。連線解析傳回延遲開啟的控制代碼，第一次非同步操作會在使用連線池前等待事件完成。也可透過 `establish().await` 明確觸發。每個具名連線在每次建立連線池時派發一次。監聽器可直接在事件提供的連線上等待 SQL 執行。 派送傳回監聽器錯誤時，由發起操作接收該錯誤。如果派送在監聽器執行期間被取消，下一次操作會重試派送。

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

### 檢視查詢日誌 {#inspecting-query-logs}

`QueryExecuted::toRawSql()` 將原始陳述式及繫結值呈現為診斷 SQL。呈現時保留引號文字、識別字及註解裡的預留位置。資料庫仍使用預備陳述式與獨立繫結執行查詢。

查詢日誌預設關閉。可在連線上開啟，也可透過 `DB` 為預設連線開啟。`getQueryLog` 傳回 SQL 及繫結值，`getRawQueryLog` 傳回嵌入值的 SQL。`disableQueryLog` 停止記錄，`flushQueryLog` 清除已記錄的項目。

```rust
use openrust::{database::{DatabaseResult, RawQueryLogEntry}, support::facades::DB};

async fn inspect_value() -> DatabaseResult<Vec<RawQueryLogEntry>> {
    DB::enableQueryLog()?;
    let result = DB::select("SELECT ? AS inspected", &[42.into()]).await;
    DB::disableQueryLog()?;
    result?;
    let log = DB::getRawQueryLog()?;
    DB::flushQueryLog()?;
    Ok(log)
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

### 交易回呼 {#transaction-callbacks}

`DB::beforeStartingTransaction` 在預設連線註冊掛鉤，在最外層交易或儲存點開始前執行。掛鉤收到連線，巢狀開始時還可借用作用中的交易。掛鉤傳回錯誤會中止本次開始操作。

`Transaction::afterRollBack` 為目前層級註冊回呼。儲存點提交後，復原回呼保留在父交易，直到最外層交易結束。確認復原後執行已撤銷層級的回呼。最外層復原回呼在工作階段歸還後執行。

```rust
use openrust::{database::DatabaseResult, support::facades::DB};

async fn run_with_callbacks() -> DatabaseResult<()> {
    DB::beforeStartingTransaction(|connection, active| {
        Box::pin(async move {
            match active {
                Some(transaction) => transaction.scalar("SELECT 1", &[]).await?,
                None => connection.scalar("SELECT 1", &[]).await?,
            };
            Ok(())
        })
    })?;

    let mut transaction = DB::beginTransaction().await?;
    transaction.afterRollBack(|| Box::pin(async {
        println!("Transaction rolled back");
        Ok(())
    }))?;
    transaction.rollBack(None).await
}
```
