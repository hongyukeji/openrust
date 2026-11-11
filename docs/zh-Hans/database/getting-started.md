# 数据库

## 配置

应用从 `config/database.rs` 读取数据库配置，默认连接为 `sqlite`。请先创建 `database/database.sqlite`，也可设定 `DB_DATABASE=:memory:` 使用临时数据库。相对 SQLite 路径以应用根目录为基准。内存库使用单个物理连接，默认使用私有缓存，可在 URL 中显式启用共享缓存。

MySQL 使用 `DB_CONNECTION=mysql`，并配置 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`。应用配置通过 `DB_URL` 设置连接的 `url`，由其提供驱动和连接凭据。MySQL 要求 TLS。可用 `MYSQL_ATTR_SSL_CA` 启用证书与主机名验证，也可在连接配置中指定 `ssl_mode` 和 `ssl_ca`。

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
