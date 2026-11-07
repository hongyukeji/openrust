//! 简单列JSON包含谓词；只修复既有PG模板，不扩展路径、OR/NOT或后端范围。

use super::{Alias, DbBackend, Expr, Result, Value, unsupported_backend, validate_identifier};

pub(super) fn predicate(
    backend: DbBackend,
    column: &str,
    value: serde_json::Value,
) -> Result<Expr> {
    validate_identifier(column)?;
    let expressions = [
        Expr::col(Alias::new(column)),
        Expr::val(Value::Json(Some(Box::new(value)))),
    ];
    match backend {
        // 模板索引指向两棵Expr；最终驱动只为JSON值分配一枚参数。
        DbBackend::Postgres => Ok(Expr::cust_with_exprs("($1)::jsonb @> $2", expressions)),
        DbBackend::MySql => Ok(Expr::cust_with_exprs("JSON_CONTAINS(?, ?)", expressions)),
        _ => Err(unsupported_backend()),
    }
}

#[cfg(test)]
#[path = "json_contains_tests.rs"]
mod tests;

#[cfg(all(test, feature = "db-postgres"))]
#[path = "json_contains_real_tests.rs"]
mod real_tests;
