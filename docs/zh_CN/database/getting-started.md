# 数据库

## 配置

应用从 `config/database.rs` 读取数据库配置，默认连接为 `sqlite`。请先创建 `database/database.sqlite`，也可设定 `DB_DATABASE=:memory:` 使用临时数据库。相对 SQLite 路径以应用根目录为基准。内存库使用单个物理连接，默认使用私有缓存，可在 URL 中显式启用共享缓存。

MySQL 使用 `DB_CONNECTION=mysql`，并配置 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。应用配置通过 `DB_URL` 设置连接的 `url`，由其提供驱动和连接凭据。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 启用证书与主机名验证，也可在连接配置中指定 `ssl_mode` 和 `ssl_ca`。 `verify_identity` 校验证书链和服务器名称。`verify_ca` 校验证书链。

### 读写连接

在 `config/database.rs` 的具名连接中配置 `read` 和 `write`。每个角色继承公共配置，并覆盖自己的字段。MySQL 在建立池配置时打乱主机列表，主机不可用时尝试下一个。密码由环境提供。

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

查询默认走读库。写入和事务走写库。开启 `sticky: true` 后，同一次 HTTP 请求或 Artisan 执行中，修改数据后的查询走写库。直接使用 `DatabaseManager` 时，连接克隆共享路由状态，可用 `forgetRecordModificationState` 清除。用 `selectFromWriteConnection` 将单次查询发往写库。


## 执行 SQL

在应用生命周期内使用 `DB::select`、`DB::selectOne`、`DB::scalar`、`DB::insert`、`DB::update`、`DB::delete` 和 `DB::statement`。SQL 使用 `?` 占位符，参数单独传入。`selectOne` 返回可选行，`scalar` 在无匹配行时返回 SQL null，有结果时必须只有一列。`update` 和 `delete` 返回受影响的行数。`unprepared` 执行原始 SQL，应传入由应用控制的文本。

`DB::connection(Some("name"))` 解析具名连接并共享应用内的连接池。查询异步执行，参数使用带类型的值。MySQL 的无符号整数和定点小数保留完整精度，定点小数结果以文本表示。

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

### 查询多个结果集 {#selecting-multiple-result-sets}

使用 `DB::selectResultSets` 执行可返回多个结果集的 MySQL 预处理语句，例如存储过程调用。结果按返回顺序排列，并保留空结果集。MySQL 的 `CALL` 还会在最后返回一个空的状态结果集。

传入 `true` 时遵循正常的读连接路由，传入 `false` 时使用写连接。在事务中通过事务对象调用该方法，查询会在事务所在连接上执行。SQLite 对该接口返回 `DatabaseError::UnsupportedOperation`。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

async fn find_active_users(db: &Connection, active: bool) -> DatabaseResult<Vec<Vec<Row>>> {
    db.selectResultSets("CALL find_active_users(?)", &[active.into()], true).await
}
```

### 使用命名绑定 {#using-named-bindings}

使用 `:name` 占位符，并通过 `Bindings::Named` 传入对应的值。键可以带前导冒号，键的排列顺序无需与 SQL 一致。

为每个命名参数绑定值，每条语句使用一种占位符。SQLite 允许重复使用同一个名称。使用 MySQL 原生预处理时，每处占位符使用不同名称。

```rust
use openrust::{database::{Bindings, DatabaseResult, Row}, support::facades::DB};

async fn find_user(id: i64) -> DatabaseResult<Option<Row>> {
    DB::selectOne(
        "SELECT * FROM users WHERE id = :id",
        Bindings::Named(&[("id", id.into())]),
    ).await
}
```

### 使用多个数据库连接

用 `DB::connection(Some("reporting"))` 按名称选择连接。用 `::read` 和 `::write` 后缀直接选择对应角色，例如 `DB::connection(Some("reporting::read"))`。

### 连接建立事件 {#connection-established}

通过 `ConnectionEstablished` 初始化应用连接。连接解析返回惰性句柄，首次异步操作会在使用连接池前等待事件完成。也可通过 `establish().await` 显式触发。每个应用的具名连接各派发一次。监听器可直接在事件携带的连接上等待 SQL 执行。 派发返回监听器错误时，由发起操作接收该错误。如果派发在监听器执行期间被取消，下一次操作会重试派发。

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

### 监听查询事件 {#listening-for-query-events}

使用 `DB::listen` 监听应用连接的成功查询。`QueryExecuted` 包含原始 SQL、绑定参数、读写角色、连接和 `Duration` 耗时。游标在成功耗尽后派发，计时排除调用方在两行之间处理数据的时间。

### 监控累计查询耗时 {#monitoring-cumulative-query-time}

使用 `DB::whenQueryingForLongerThan` 监控默认连接，也可在具名连接上注册。累计耗时超过阈值后，每个处理器执行一次。`resetTotalQueryDuration` 清零耗时。`allowQueryDurationHandlersToRunAgain` 允许处理器再次执行。

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

### 查看查询日志 {#inspecting-query-logs}

`QueryExecuted::toRawSql()` 将原始语句及绑定值渲染为诊断 SQL。渲染保留引号文本、标识符及注释里的占位符。数据库仍用预处理语句和独立绑定执行查询。

查询日志默认关闭。可在连接上开启，也可通过 `DB` 为默认连接开启。`getQueryLog` 返回 SQL 和绑定值，`getRawQueryLog` 返回嵌入值的 SQL。`disableQueryLog` 停止记录，`flushQueryLog` 清除已记录的条目。

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

## 数据库事务

默认连接使用 `DB::transaction`，具名连接直接调用 `transaction`。在回调中通过传入的事务句柄执行 SQL。该句柄独占一条写库物理连接，直到提交或回滚。Rust 用装箱的 Future 在异步查询期间借用事务句柄。第二个参数是总尝试次数，传 `1` 表示执行一次。并发错误可能重跑整个回调，把外部副作用放在事务之外，或保证可以安全重复执行。

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

手动控制时调用 `beginTransaction().await?`，然后在返回的句柄上调用 `commit` 或 `rollBack(None)`。在句柄上再次调用 `beginTransaction` 会创建保存点。`rollBack(Some(level))` 回滚至指定层级，层级零回滚整个事务。丢弃活动句柄会回滚尚未提交的操作。MySQL 的 DDL 等语句可能隐式提交，请放在要求原子性的事务之外。

### 事务事件 {#transaction-events}

通过应用事件分发器订阅。`TransactionBeginning` 在事务或保存点建立后触发。`TransactionCommitting` 在最外层提交前触发。`TransactionCommitted` 在事务层级下降并执行该层应运行的提交回调后触发。`TransactionRolledBack` 在显式回滚或事务回调失败引发的回滚完成后触发一次。

仍有活动层级时，监听器可通过事件上下文使用当前事务。最外层提交或回滚后，事件携带连接及层级 0。最外层提交后监听失败时返回 `DatabaseError::AfterCommit`，已提交数据保留。

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
