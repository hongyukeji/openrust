//! 同一次 INSERT 的自增结果或 PostgreSQL RETURNING；不另取连接查询 last ID。
//! 数值返回对照固定 Laravel `Query/Processors` 的 `is_numeric(id) ? (int) id : id`，
//! 此转换不用于 Eloquent 模型原始主键；BYTEA 保留 Bytes 是 Rust 的类型适配。

use super::{ConnectionTrait, DbBackend, Error, QueryResult, QueryState, Result, Statement, Value};

impl QueryState<'_, '_> {
    pub(super) async fn insert_get_id(
        &self,
        values: Vec<(&str, Value)>,
        sequence: Option<&str>,
    ) -> Result<Value> {
        self.validate()?;
        match self.db.backend()? {
            DbBackend::MySql | DbBackend::Sqlite => {
                let result = self
                    .execute_result(self.compile_insert(values, None)?)
                    .await?;
                result
                    .try_last_insert_id()
                    .map_err(Error::internal)
                    .and_then(processor_id)
            }
            DbBackend::Postgres => {
                let column = sequence
                    .filter(|value| !value.is_empty() && *value != "0")
                    .unwrap_or("id");
                let (sql, values) = self.compile_insert(values, Some(column))?;
                let statement = Statement::from_sql_and_values(DbBackend::Postgres, sql, values.0);
                let row = match self.transaction {
                    Some(transaction) => transaction.query_one_raw(statement).await,
                    None => self.db.primary_connection()?.query_one_raw(statement).await,
                }
                .map_err(Error::internal)?
                .ok_or_else(|| Error::custom("query_insert_id_missing", "插入未返回主键值"))?;
                postgres_id(&row)
            }
            _ => Err(super::unsupported_backend()),
        }
    }
}

// PDO 对 MySQL 超出有符号范围的原生 u64 使用字符串；字符串整数溢出封顶，
// 与实际浮点值的越界 warning 规则不同，不能先统一转 f64。
fn processor_id(value: Value) -> Result<Value> {
    match value {
        Value::BigUnsigned(Some(value)) => Ok(text_id(value.to_string())),
        Value::String(Some(value)) => Ok(text_id(value)),
        Value::Float(Some(value)) => float_id(f64::from(value)),
        Value::Double(Some(value)) => float_id(value),
        other => Ok(other),
    }
}

fn text_id(value: String) -> Value {
    if crate::support::InputNumber::numeric_string(&value).is_some() {
        Value::BigInt(Some(crate::support::scalars::integer(
            &serde_json::Value::String(value),
        )))
    } else {
        Value::from(value)
    }
}

fn float_id(value: f64) -> Result<Value> {
    // PHP builtin 仍返回 wrapped 值，但会产生 E_WARNING；固定 Laravel
    // HandleExceptions::bootstrap 开启 error_reporting(-1)，handleError 将其升级为异常。
    // INSERT 在 Processor 转换前已执行；这里不擅自开启事务或回滚已完成的写入。
    if !value.is_finite()
        || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
    {
        return Err(Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "query_insert_id_numeric_cast_failed",
            "插入主键数值无法按运行时规则转换",
        ));
    }
    Ok(Value::BigInt(Some(crate::support::scalars::integer(
        &serde_json::Value::from(value),
    ))))
}

#[cfg(feature = "db-postgres")]
fn postgres_id(row: &QueryResult) -> Result<Value> {
    use sea_orm::sqlx::{Column, Row, TypeInfo};
    let raw = row.try_as_pg_row().ok_or_else(|| {
        Error::custom(
            "query_insert_id_type_unsupported",
            "插入主键不是 PostgreSQL 原生行",
        )
    })?;
    let field = raw
        .columns()
        .first()
        .ok_or_else(|| Error::custom("query_insert_id_missing", "插入未返回主键列"))?;
    // 原列决定 PDO 标量类别；同一 RETURNING 的文本副本保全 NUMERIC 精度及其它文本类型。
    // 按序号读取，避免真实主键恰好等于内部别名时发生同名列歧义。
    macro_rules! native_integer {
        ($type:ty) => {
            row.try_get_by_index::<Option<$type>>(0)
                .map(|value| Value::BigInt(value.map(i64::from)))
                .map_err(Error::internal)
        };
    }
    match field.type_info().name() {
        "INT2" => native_integer!(i16),
        "INT4" => native_integer!(i32),
        "INT8" => row
            .try_get_by_index::<Option<i64>>(0)
            .map(Value::BigInt)
            .map_err(Error::internal),
        "OID" => native_integer!(u32),
        "FLOAT4" => row
            .try_get_by_index::<Option<f32>>(0)
            .map_err(Error::internal)?
            .map_or(Ok(Value::Float(None)), |value| float_id(f64::from(value))),
        "FLOAT8" => row
            .try_get_by_index::<Option<f64>>(0)
            .map_err(Error::internal)?
            .map_or(Ok(Value::Double(None)), float_id),
        "BOOL" => row
            .try_get_by_index::<Option<bool>>(0)
            .map(Value::from)
            .map_err(Error::internal),
        "BYTEA" => row
            .try_get_by_index::<Option<Vec<u8>>>(0)
            .map(Value::from)
            .map_err(Error::internal),
        _ => row
            .try_get_by_index::<Option<String>>(1)
            .map(|value| value.map_or(Value::String(None), text_id))
            .map_err(Error::internal),
    }
}

#[cfg(not(feature = "db-postgres"))]
fn postgres_id(_: &QueryResult) -> Result<Value> {
    Err(super::unsupported_backend())
}

#[cfg(all(
    test,
    any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres")
))]
#[path = "insert_id_tests.rs"]
mod tests;
