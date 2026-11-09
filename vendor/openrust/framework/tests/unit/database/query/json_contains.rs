//! 旧PG模板丢失列与JSON绑定的纯AST反例，以及修复后的声明顺序和标识符边界。

use super::*;
use sea_orm::sea_query::{Asterisk, ExprTrait, MysqlQueryBuilder, PostgresQueryBuilder, Query};
use serde_json::json;

#[test]
fn old_postgres_question_template_leaves_sql_tokens_and_loses_both_expressions() {
    let old = Expr::cust_with_exprs(
        "? @> ?",
        [
            Expr::col(Alias::new("options")),
            Expr::val(Value::Json(Some(Box::new(json!(["en"]))))),
        ],
    );
    let (sql, values) = Query::select()
        .column(Asterisk)
        .from(Alias::new("users"))
        .and_where(old)
        .build(PostgresQueryBuilder);
    assert_eq!(sql, "SELECT * FROM \"users\" WHERE ? @> ?");
    assert_eq!(values.0, Vec::<Value>::new());
}

#[test]
fn postgres_contains_has_one_json_parameter_and_explicit_jsonb_column_cast() -> Result<()> {
    let value = json!(["en", "中文"]);
    let (sql, values) = Query::select()
        .column(Asterisk)
        .from(Alias::new("users"))
        .and_where(predicate(DbBackend::Postgres, "options", value.clone())?)
        .build(PostgresQueryBuilder);
    assert_eq!(
        sql,
        "SELECT * FROM \"users\" WHERE (\"options\")::jsonb @> $1"
    );
    assert_eq!(values.0, vec![Value::Json(Some(Box::new(value)))]);
    Ok(())
}

#[test]
fn json_binding_stays_between_distinct_before_and_after_values_in_both_dialects() -> Result<()> {
    for backend in [DbBackend::Postgres, DbBackend::MySql] {
        let value = json!({"语言":["中文"], "meta":{}});
        let statement = Query::select()
            .column(Asterisk)
            .from(Alias::new("users"))
            .and_where(Expr::col(Alias::new("tenant_id")).eq(41i64))
            .and_where(predicate(backend, "options", value.clone())?)
            .and_where(Expr::col(Alias::new("active")).eq(true))
            .to_owned();
        let (sql, values) = match backend {
            DbBackend::Postgres => statement.build(PostgresQueryBuilder),
            DbBackend::MySql => statement.build(MysqlQueryBuilder),
            _ => return Err(unsupported_backend()),
        };
        assert_eq!(
            values.0,
            vec![
                41i64.into(),
                Value::Json(Some(Box::new(value))),
                true.into()
            ]
        );
        assert!(!sql.contains("? @> ?"));
        if backend == DbBackend::Postgres {
            assert!(sql.contains("@> $2"));
        }
    }
    Ok(())
}

#[test]
fn mysql_statement_matches_original_expression() -> Result<()> {
    let value = json!(["en"]);
    let (sql, values) = Query::select()
        .column(Asterisk)
        .from(Alias::new("users"))
        .and_where(predicate(DbBackend::MySql, "options", value.clone())?)
        .build(MysqlQueryBuilder);
    assert_eq!(
        sql,
        "SELECT * FROM `users` WHERE JSON_CONTAINS(`options`, ?)"
    );
    assert_eq!(values.0, vec![Value::Json(Some(Box::new(value)))]);
    Ok(())
}

#[test]
fn column_injection_paths_and_qualified_names_are_rejected_without_expanding_scope() {
    for column in [
        "",
        "options); DROP TABLE users;--",
        "users.options",
        "options->languages",
        "非法列",
    ] {
        assert!(
            matches!(predicate(DbBackend::Postgres,column,json!({})),Err(error) if error.code()=="query_identifier_invalid")
        );
    }
}
