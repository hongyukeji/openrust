//! JSON 包含及取反沿同一条件树处理。

use serde_json::Value as JsonValue;

use super::comparison::{json_field_and_path, native_json_path_literal, pg_json_path_template};
use super::{
    DbBackend, Error, Expr, ExprTrait, PredicateChain, QueryBuilder, QueryContext, Result, Value,
    WhereGroup, unsupported_backend,
};

fn predicate(
    backend: DbBackend,
    mut field: Expr,
    path: Option<&str>,
    value: JsonValue,
    not: bool,
) -> Result<Expr> {
    if path.is_some_and(|path| path.contains('\0')) {
        return Err(Error::custom(
            "query_json_path_invalid",
            "JSON 查询路径不能包含 NUL",
        ));
    }
    if backend == DbBackend::Postgres && path.is_some() {
        field = Expr::cust_with_expr(pg_json_path_template(path, false), field);
    }
    let binding = if backend == DbBackend::Sqlite {
        sqlite_binding(value)?
    } else {
        Value::Json(Some(Box::new(value)))
    };
    let expressions = [field, Expr::val(binding)];
    let predicate = match backend {
        // 模板索引指向两棵Expr；最终驱动只为JSON值分配一枚参数。
        DbBackend::Postgres => Expr::cust_with_exprs("($1)::jsonb @> $2", expressions),
        DbBackend::MySql => {
            let template = if path.is_some() {
                format!("JSON_CONTAINS(?, ?, {})", native_json_path_literal(path))
            } else {
                "JSON_CONTAINS(?, ?)".to_owned()
            };
            Expr::cust_with_exprs(template, expressions)
        }
        DbBackend::Sqlite => {
            let path = if path.is_some() {
                format!(", {}", native_json_path_literal(path))
            } else {
                String::new()
            };
            Expr::cust_with_exprs(
                format!(
                    r#"exists (select 1 from json_each(?{path}) where "json_each"."value" is ?)"#
                ),
                expressions,
            )
        }
        _ => return Err(unsupported_backend()),
    };
    Ok(if not { predicate.not() } else { predicate })
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

impl PredicateChain<'_> {
    fn json_contains(
        self,
        context: &QueryContext<'_>,
        selector: &str,
        value: JsonValue,
        or: bool,
        not: bool,
    ) -> Result<Self> {
        let (field, path) = json_field_and_path(context, selector)?;
        let expr = predicate(context.db.backend()?, field, path, value, not)?;
        Ok(self.add_comparison(expr.clone(), expr, Vec::new(), or))
    }
}

macro_rules! json_contains_methods {
    () => {
        /// 添加 JSON 包含条件，接受限定列及 `->` 路径。
        ///
        /// SQLite 仅接受标量查找值。
        pub fn where_json_contains(self, selector: &str, value: JsonValue) -> Result<Self> {
            self.json_contains(selector, value, false, false)
        }

        /// 添加 OR JSON 包含条件，保留 SQL 的 AND 优先级。
        pub fn or_where_json_contains(self, selector: &str, value: JsonValue) -> Result<Self> {
            self.json_contains(selector, value, true, false)
        }

        /// 添加 JSON 不包含条件，只对当前包含谓词取反。
        pub fn where_json_doesnt_contain(self, selector: &str, value: JsonValue) -> Result<Self> {
            self.json_contains(selector, value, false, true)
        }

        /// 添加 OR JSON 不包含条件，只对当前包含谓词取反。
        pub fn or_where_json_doesnt_contain(
            self,
            selector: &str,
            value: JsonValue,
        ) -> Result<Self> {
            self.json_contains(selector, value, true, true)
        }
    };
}

impl QueryBuilder<'_, '_> {
    json_contains_methods!();

    fn json_contains(self, selector: &str, value: JsonValue, or: bool, not: bool) -> Result<Self> {
        self.transform(|mut state| {
            let context = state.context();
            state.predicates = state
                .predicates
                .json_contains(&context, selector, value, or, not)?;
            Ok(state)
        })?;
        Ok(self)
    }
}

impl WhereGroup<'_, '_> {
    json_contains_methods!();

    fn json_contains(
        mut self,
        selector: &str,
        value: JsonValue,
        or: bool,
        not: bool,
    ) -> Result<Self> {
        self.condition = self
            .condition
            .json_contains(&self.context, selector, value, or, not)?;
        Ok(self)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/database/query/json_contains.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/database/query/sqlite_json_contains.rs"]
mod sqlite_tests;

#[cfg(test)]
#[path = "../../../tests/unit/database/query/json_contains_selectors.rs"]
mod selector_tests;

#[cfg(all(test, feature = "db-postgres"))]
#[path = "json_contains_real_tests.rs"]
mod real_tests;
