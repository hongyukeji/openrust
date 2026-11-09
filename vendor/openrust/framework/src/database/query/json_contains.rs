//! 已支持后端的简单列 JSON 包含谓词。

use serde_json::Value as JsonValue;

use super::{
    Alias, DbBackend, Error, Expr, Result, Value, unsupported_backend, validate_identifier,
};

pub(super) fn predicate(backend: DbBackend, column: &str, value: JsonValue) -> Result<Expr> {
    validate_identifier(column)?;
    let binding = if backend == DbBackend::Sqlite {
        sqlite_binding(value)?
    } else {
        Value::Json(Some(Box::new(value)))
    };
    let expressions = [Expr::col(Alias::new(column)), Expr::val(binding)];
    match backend {
        // 模板索引指向两棵Expr；最终驱动只为JSON值分配一枚参数。
        DbBackend::Postgres => Ok(Expr::cust_with_exprs("($1)::jsonb @> $2", expressions)),
        DbBackend::MySql => Ok(Expr::cust_with_exprs("JSON_CONTAINS(?, ?)", expressions)),
        DbBackend::Sqlite => Ok(Expr::cust_with_exprs(
            r#"exists (select 1 from json_each(?) where "json_each"."value" is ?)"#,
            expressions,
        )),
        _ => Err(unsupported_backend()),
    }
}

fn sqlite_binding(value: JsonValue) -> Result<Value> {
    // Laravel 先把 bool 转成整数；PDO SQLite 对浮点用默认精度的文本绑定。
    match value {
        JsonValue::Null => Ok(Value::String(None)),
        JsonValue::Bool(flag) => Ok(i64::from(flag).into()),
        JsonValue::String(text) => Ok(text.into()),
        JsonValue::Number(number) => {
            if let Some(integer) = number.as_i64() {
                return Ok(integer.into());
            }
            let number = number.as_f64().ok_or_else(|| {
                Error::custom(
                    "query_sqlite_json_contains_number_invalid",
                    "SQLite JSON 包含条件的数值超出可表示范围",
                )
            })?;
            Ok(crate::support::scalars::double_string(number).into())
        }
        JsonValue::Array(_) | JsonValue::Object(_) => Err(Error::custom(
            "query_sqlite_json_contains_requires_scalar",
            "SQLite JSON 包含条件需要标量值",
        )),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/database/query/json_contains.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/database/query/sqlite_json_contains.rs"]
mod sqlite_tests;

#[cfg(all(test, feature = "db-postgres"))]
#[path = "json_contains_real_tests.rs"]
mod real_tests;
