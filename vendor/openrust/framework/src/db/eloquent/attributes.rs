//! 数据库行到模型原始属性的逐列适配，不经便利 JSON 转换或模型 cast。

use crate::{Error, Result, db::sea_orm::QueryResult};
use serde_json::{Map, Value};

#[cfg(feature = "db-mysql")]
#[path = "attributes_backend_mysql.rs"]
mod mysql;
#[cfg(any(
    feature = "db-postgres",
    all(test, any(feature = "db-mysql", feature = "db-sqlite"))
))]
#[path = "attributes_numeric.rs"]
mod numeric;
#[cfg(feature = "db-postgres")]
#[path = "attributes_backend_postgres.rs"]
mod postgres;
#[cfg(feature = "db-sqlite")]
#[path = "attributes_backend_sqlite.rs"]
mod sqlite;

/// 从同一次查询结果读取全部原始属性，不执行模型 getter、setter 或 hashed cast。
///
/// 数值和文本按 PDO 标量类别读取；大 unsigned 与 NUMERIC 保持精确文本。
/// JSON 列保持数据库文本，SQL NULL 保持 null。二进制值以字节数组表示，
/// 这是 JSON 属性容器对 PHP 二进制字符串或资源的显式 Rust 适配。
/// PostgreSQL 二进制日期采用 ISO 文本，timestamptz 以 UTC 表示同一瞬间；
/// 行结果不携带会话 TimeZone/DateStyle，不能据此还原自定义会话的显示格式。
/// 重名列按查询序号后项覆盖，与 PDO 的属性对象相同；未知或不可表示值返回错误。
pub fn attributes_from_row(row: &QueryResult) -> Result<Map<String, Value>> {
    #[cfg(feature = "db-mysql")]
    if let Some(raw) = row.try_as_mysql_row() {
        return mysql::attributes(raw);
    }
    #[cfg(feature = "db-postgres")]
    if let Some(raw) = row.try_as_pg_row() {
        return postgres::attributes(raw);
    }
    #[cfg(feature = "db-sqlite")]
    if let Some(raw) = row.try_as_sqlite_row() {
        return sqlite::attributes(raw);
    }
    let _ = row;
    Err(unsupported())
}

fn unsupported() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "model_database_attribute_unsupported",
        "模型查询包含尚不支持的数据库字段类型",
    )
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
fn invalid() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "model_database_attribute_invalid",
        "模型数据库字段无法按其原始类型解码",
    )
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
fn finite(value: f64) -> Result<Value> {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "model_database_attribute_non_finite",
                "JSON 模型属性不能无损表示非有限浮点数",
            )
        })
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
fn bytes(value: Vec<u8>) -> Value {
    Value::Array(value.into_iter().map(Value::from).collect())
}

#[cfg(all(
    test,
    any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite")
))]
#[path = "attributes_tests.rs"]
mod tests;
