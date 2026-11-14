# 数据库

## 配置

应用从 `config/database.rs` 读取数据库配置，默认连接为 `sqlite`。请先创建 `database/database.sqlite`，也可设定 `DB_DATABASE=:memory:` 使用临时数据库。相对 SQLite 路径以应用根目录为基准。内存库使用单个物理连接，默认使用私有缓存，可在 URL 中显式启用共享缓存。

MySQL 使用 `DB_CONNECTION=mysql`，并配置 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。应用配置通过 `DB_URL` 设置连接的 `url`，由其提供驱动和连接凭据。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 启用证书与主机名验证，也可在连接配置中指定 `ssl_mode` 和 `ssl_ca`。

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

### 使用多个数据库连接

用 `DB::connection(Some("reporting"))` 按名称选择连接。用 `::read` 和 `::write` 后缀直接选择对应角色，例如 `DB::connection(Some("reporting::read"))`。

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
