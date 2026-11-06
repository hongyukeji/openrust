//! SQLite 使用运行时 storage class，不能把 INTEGER 当 bool 或把 TEXT 自动解析 JSON。

use super::{Map, Result, Value, bytes, finite, invalid, unsupported};
use sea_orm::sqlx::{Column, Row, TypeInfo, ValueRef, sqlite::SqliteRow};

pub(super) fn attributes(row: &SqliteRow) -> Result<Map<String, Value>> {
    let mut attributes = Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        let raw = row.try_get_raw(index).map_err(|_| invalid())?;
        let value = if raw.is_null() {
            Value::Null
        } else {
            match raw.type_info().name() {
                "INTEGER" => Value::from(row.try_get::<i64, _>(index).map_err(|_| invalid())?),
                "REAL" => finite(row.try_get::<f64, _>(index).map_err(|_| invalid())?)?,
                "TEXT" => Value::from(row.try_get::<String, _>(index).map_err(|_| invalid())?),
                "BLOB" => bytes(row.try_get::<Vec<u8>, _>(index).map_err(|_| invalid())?),
                _ => return Err(unsupported()),
            }
        };
        attributes.insert(column.name().to_owned(), value);
    }
    Ok(attributes)
}
