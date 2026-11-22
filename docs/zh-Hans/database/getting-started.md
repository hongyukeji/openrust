# 数据库

## 配置

未设置 `DB_DATABASE` 时，MySQL、MariaDB、PostgreSQL 和 SQL Server 从应用名称派生默认库名。配置中的 `database_name` 方法复用 ASCII 转写，将字母转为小写，并将连续空格或特殊字符合并为下划线。去掉开头的数字及下划线，例如 `123 Store` 转为 `store`；结果为空时返回配置错误，提示显式设置 `DB_DATABASE`。显式 `DB_DATABASE` 或非空 `DB_URL` 优先于默认库名派生。结果最多保留 63 个 ASCII 字节，并移除尾部下划线。例如，`ExampleStore` 转为 `examplestore`，`My Shop-V2` 转为 `my_shop_v2`。显式 `DB_DATABASE` 原样使用。SQLite 继续使用文件路径默认值。连接前创建对应数据库，已有数据库可用 `DB_DATABASE` 明确指定。

应用从 `config/database.rs` 读取数据库配置，默认连接为 `sqlite`。请先创建 `database/database.sqlite`，也可设定 `DB_DATABASE=:memory:` 使用临时数据库。相对 SQLite 路径以应用根目录为基准。内存库使用单个物理连接，默认使用私有缓存，可在 URL 中显式启用共享缓存。

MySQL 使用 `DB_CONNECTION=mysql`，并配置 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。应用配置通过 `DB_URL` 设置连接的 `url`，由其提供驱动和连接凭据。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 启用证书与主机名验证，也可在连接配置中指定 `ssl_mode` 和 `ssl_ca`。 `verify_identity` 校验证书链和服务器名称。`verify_ca` 校验证书链。

### MariaDB {#mariadb}

设置 `DB_CONNECTION=mariadb`，主机、数据库、凭据及 TLS 设置与 MySQL 相同。通过 `DB_URL` 配置时使用 `mariadb://`。为校验服务端证书和名称，将 `MYSQL_ATTR_SSL_CA` 指向服务端 CA 证书，`DB_HOST` 使用证书所覆盖的名称或地址。

MariaDB 方言支持 JSON 列更新和从 JSON 生成列。`uuid()` 在 10.7 之前使用 `char(36)`，从 10.7 起使用原生 `uuid`。数据库队列从 10.6 起使用 `SKIP LOCKED`。实测版本见[发布矩阵](../prologue/releases.md)。

### SQL Server {#sql-server}

设置 `DB_CONNECTION=sqlsrv`，并配置 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。连接 URL 使用 `sqlsrv://`。OpenRust 使用 Rust TDS 客户端，要求 TLS 并校验服务端证书。使用私有 CA 时，将 `DB_SSL_CA` 指向签发证书，并使用服务端证书覆盖的主机名或地址。

实测安装使用 SQL Server 2017 或 2022 上已创建的数据库。在启动 HTTP 和数据库队列 Worker 前执行 `artisan migrate --force`。实测构建见[发布矩阵](../prologue/releases.md)。

### MySQL 5.7 TLS {#mysql-57-tls}

MySQL 5.7.44 需要能通过 ECDHE 协商 TLS 1.2 的服务端构建。Oracle 通用 Linux 归档使用 RSA 服务端证书和以下 `mysqld` 配置。通过 `ssl-ca`、`ssl-cert` 和 `ssl-key` 配置 CA、服务端证书及私钥。

```ini
[mysqld]
require_secure_transport=ON
tls_version=TLSv1.2
ssl_cipher=ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384
```

应用的 `MYSQL_ATTR_SSL_CA` 指向 CA 证书，`DB_HOST` 使用服务端证书中的名称或地址。

#### Oracle Linux 7 镜像 {#mysql-57-oracle-linux-7}

原版 `mysql:5.7` 镜像缺少 OpenRust 所需的服务端 ECDHE 设置。从仓库根目录构建派生镜像。它为固定的 MySQL 5.7.44 服务端启用临时曲线选择，并应用上面的 TLS 配置。

```sh
docker build --platform linux/amd64 -t openrust/mysql57-ecdhe docker/mysql57-ecdhe
```

使用 `openrust/mysql57-ecdhe` 时沿用原镜像的数据库挂载和环境变量。挂载 RSA 服务端证书和私钥，并为 `--ssl-ca`、`--ssl-cert` 和 `--ssl-key` 传入容器内路径。应用通过 `MYSQL_ATTR_SSL_CA` 和 `DB_HOST` 校验证书与服务端名称。见[发布矩阵](../prologue/releases.md)。

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

#### 直连别名 {#direct-connection-alias}

对 MySQL 和 SQLite，`DB::connection(Some("mysql::direct"))` 会让读写操作都使用该连接的写端点。将 `mysql` 替换为实际连接名。查询日志和事件记录为 `QueryRole::Direct`。Laravel 未配置独立直连端点时也回退到写端点。

PostgreSQL 的 `direct` 配置非空时，`DB::connection(Some("pgsql::direct"))` 选择独立端点。按需设置 `DB_DIRECT_HOST`、`DB_DIRECT_PORT`、`DB_DIRECT_USERNAME`、`DB_DIRECT_PASSWORD` 和 `DB_DIRECT_SSLMODE`。具名连接的 `direct` 选项继承其基础配置。迁移和账本操作默认走该端点。显式的 `::read`、`::write`、`::direct` 后缀按调用方指定的角色执行。

使用 PgBouncer 事务池时，设置 `DB_POOLED=true` 或配置 PostgreSQL 的 `direct` 端点。池化的读写连接默认模拟预备语句，用 PostgreSQL 简单查询协议执行已编码的绑定值，支持 `max_prepared_statements=0`。此模式下，扩展类型和用户自定义类型按服务器返回的文本读取。直连默认使用原生预备语句。可在连接或其 `read`、`write`、`direct` 配置中设置 `emulate_prepares` 覆盖默认值。如果池化端点使用原生预备语句，启用 PgBouncer 的具名语句跟踪。保持应用和服务器的 `search_path` 一致，迁移使用直连端点。

#### 断开、重连及清除连接 {#reconnecting-database-connections}

`DB::disconnect` 退役当前读写连接池，下一次异步操作按当前配置建立新池。`DB::reconnect` 立即刷新连接池，保留逻辑连接和查询日志。`DB::purge` 同时清除缓存的连接，下次解析会创建新的逻辑连接。

进行中的事务继续使用原会话直到结束。已取得会话的操作可以完成，新操作使用替换后的连接池。

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

#### 恢复丢失的数据库连接 {#recovering-lost-connections}

事务外执行语句时，驱动报告连接丢失后会刷新连接池并重试一次语句。重试成功只派发一次 `QueryExecuted`。最外层 `BEGIN` 期间丢失连接也可重试一次。

活动事务的断线错误交给调用方。流式游标可在返回首行前重试。如果服务器在响应丢失前已完成写入，重试可能再次执行该写入。

### 预演查询 {#previewing-queries}

用 `DB::pretend` 或指定连接的 `pretend` 收集 SQL。预演只记录 SQL，读操作返回空结果，写操作按方法返回成功或零影响行。查询前回调和查询事件照常运行。返回的条目包含渲染后的 SQL 和原始绑定值，预演日志独立于实时查询日志。

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

预演作用于当前异步作用域中的指定连接。独立启动的任务进入自己的作用域。用 `pretending()` 检查当前是否在预演，用 `withoutPretending` 在预演中显式执行真实操作。事务的开始、提交和回滚需要真实执行作用域。

### 查询执行前回调 {#before-executing}

用 `DB::beforeExecuting` 向默认连接注册，也可以在指定连接上调用 `beforeExecuting`。回调按注册顺序、在数据库操作之前收到原始 SQL 和绑定值。回调返回错误会中止本次操作及后续回调。游标首次轮询时运行回调。驱动层重放同一次查询时跳过回调。

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

最后一个参数是当前事务句柄。回调需要在同一事务中执行操作时，使用该句柄。

### 连接建立事件 {#connection-established}

通过 `ConnectionEstablished` 初始化应用连接。连接解析返回惰性句柄，首次异步操作会在使用连接池前等待事件完成。也可通过 `establish().await` 显式触发。每个具名连接在每次创建连接池时派发一次。监听器可直接在事件携带的连接上等待 SQL 执行。 派发返回监听器错误时，由发起操作接收该错误。如果派发在监听器执行期间被取消，下一次操作会重试派发。

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

### 共用连接接口 {#connection-interface}

同一段 SQL 或构建器代码需要同时支持池化 `Connection` 和绑定物理会话的 `Transaction` 时，可使用 `&mut dyn ConnectionInterface`。显式传入命名或位置 `Bindings`。`select`、`selectOne`、`scalar` 和 `cursor` 的读角色参数传入 `false` 时，池化连接走写库。事务中的查询始终使用事务自己的会话。事务的开始、提交和回滚由具体的连接与事务句柄管理。

```rust
use openrust::database::{Bindings, ConnectionInterface, DatabaseResult, Value};

pub async fn answer(connection: &mut dyn ConnectionInterface) -> DatabaseResult<Value> {
    connection
        .scalar("SELECT :answer AS answer", Bindings::Named(&[("answer", 42.into())]), false)
        .await
}
```

### SQL Server 自动回滚 {#sql-server-automatic-rollback}

SQL Server 在语句失败时可能自动撤销整条事务。事务回调返回该错误时，OpenRust 会先检查服务端事务状态。服务端已回滚时，回调返回原始语句错误，事务句柄关闭。剩余事务无法回滚到保存点时，OpenRust 会回滚整条事务。确认回滚后，执行相应的 `afterRollBack()` 回调。

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

### 事务回调 {#transaction-callbacks}

`DB::beforeStartingTransaction` 在默认连接注册钩子，在最外层事务或保存点开始前运行。钩子收到连接，嵌套开始时还可借用活动事务。钩子返回错误会中止本次开始操作。

`Transaction::afterRollBack` 为当前层级注册回调。保存点提交后，回滚回调保留在父事务，直到最外层事务结束。确认回滚后执行被撤销层级的回调。最外层回滚回调在会话归还后运行。

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
