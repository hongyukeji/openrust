use sqlx::{
    Column as _, Row as _, TypeInfo as _, ValueRef as _, mysql::MySqlRow, sqlite::SqliteRow,
};

use super::{DatabaseError, DatabaseResult};

/// An SQL parameter or result value. Exact decimals are represented as text.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Real(f64),
    Text(String),
    Bytes(Vec<u8>),
    Json(serde_json::Value),
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}
impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// Ordered columns from a query. Repeated column names are preserved.
#[derive(Clone, Debug, PartialEq)]
pub struct Row(Vec<(String, Value)>);

impl Row {
    pub(crate) fn into_columns(self) -> Vec<(String, Value)> {
        self.0
    }

    pub(super) fn remove_last_column(&mut self) {
        self.0.pop();
    }

    pub fn columns(&self) -> &[(String, Value)] {
        &self.0
    }
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(name, _)| name == column)
            .map(|(_, value)| value)
    }
    pub fn get_at(&self, index: usize) -> Option<&Value> {
        self.0.get(index).map(|(_, value)| value)
    }
    pub(super) fn scalar(mut self) -> DatabaseResult<Value> {
        if self.0.len() != 1 {
            return Err(DatabaseError::InvalidScalar);
        }
        Ok(self.0.pop().expect("checked column count").1)
    }
}

macro_rules! decode_row {
    ($function:ident, $row:ty, $decode:expr) => {
        pub(super) fn $function(row: $row) -> DatabaseResult<Row> {
            let mut values = Vec::with_capacity(row.len());
            for (index, column) in row.columns().iter().enumerate() {
                let raw = row
                    .try_get_raw(index)
                    .map_err(DatabaseError::driver("decode"))?;
                let value = if raw.is_null() {
                    Value::Null
                } else {
                    $decode(&row, index, raw.type_info().name())?
                };
                values.push((column.name().to_owned(), value));
            }
            Ok(Row(values))
        }
    };
}

decode_row!(sqlite, SqliteRow, sqlite_value);
decode_row!(mysql, MySqlRow, mysql_value);

fn sqlite_value(row: &SqliteRow, index: usize, kind: &str) -> DatabaseResult<Value> {
    let value = match kind {
        "INTEGER" => row.try_get_unchecked::<i64, _>(index).map(Value::Integer),
        "BOOLEAN" => row.try_get_unchecked::<bool, _>(index).map(Value::Bool),
        "REAL" => row.try_get_unchecked::<f64, _>(index).map(Value::Real),
        "TEXT" | "DATE" | "TIME" | "DATETIME" => {
            row.try_get_unchecked::<String, _>(index).map(Value::Text)
        }
        "BLOB" => row.try_get_unchecked::<Vec<u8>, _>(index).map(Value::Bytes),
        _ => return Err(DatabaseError::UnsupportedColumnType(kind.to_owned())),
    };
    value.map_err(DatabaseError::driver("decode"))
}

fn mysql_value(row: &MySqlRow, index: usize, kind: &str) -> DatabaseResult<Value> {
    let value = match kind {
        "BOOLEAN" => row.try_get_unchecked::<bool, _>(index).map(Value::Bool),
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" => {
            row.try_get_unchecked::<i64, _>(index).map(Value::Integer)
        }
        "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" | "INT UNSIGNED"
        | "BIGINT UNSIGNED" | "BIT" | "YEAR" => {
            row.try_get_unchecked::<u64, _>(index).map(Value::Unsigned)
        }
        "FLOAT" => row
            .try_get_unchecked::<f32, _>(index)
            .map(|value| Value::Real(f64::from(value))),
        "DOUBLE" => row.try_get_unchecked::<f64, _>(index).map(Value::Real),
        "DECIMAL" | "CHAR" | "VARCHAR" | "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT"
        | "ENUM" | "SET" => row.try_get_unchecked::<String, _>(index).map(Value::Text),
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "GEOMETRY" => {
            row.try_get_unchecked::<Vec<u8>, _>(index).map(Value::Bytes)
        }
        "JSON" => row
            .try_get_unchecked::<sqlx::types::Json<serde_json::Value>, _>(index)
            .map(|value| Value::Json(value.0)),
        "DATE" => row
            .try_get_unchecked::<chrono::NaiveDate, _>(index)
            .map(|value| Value::Text(value.to_string())),
        "DATETIME" | "TIMESTAMP" => row
            .try_get_unchecked::<chrono::NaiveDateTime, _>(index)
            .map(|value| Value::Text(value.to_string())),
        "TIME" => row
            .try_get_unchecked::<sqlx::mysql::types::MySqlTime, _>(index)
            .map(|value| Value::Text(value.to_string())),
        _ => return Err(DatabaseError::UnsupportedColumnType(kind.to_owned())),
    };
    value.map_err(DatabaseError::driver("decode"))
}
