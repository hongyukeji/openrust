//! 固定 JSON 包含的限定列、路径和布尔组合。

use super::tests::{selector_context as context, selector_predicate as predicate};
use super::{DbBackend, Error, Expr, PredicateChain, Result, Value};
use sea_orm::sea_query::{
    Alias, Asterisk, Condition, ExprTrait, MysqlQueryBuilder, PostgresQueryBuilder, Query,
    SqliteQueryBuilder,
};
use serde_json::json;

fn build(backend: DbBackend, condition: Condition) -> (String, Vec<Value>) {
    let query = Query::select()
        .column(Asterisk)
        .from(Alias::new("users"))
        .cond_where(condition)
        .to_owned();
    let (sql, values) = match backend {
        DbBackend::MySql => query.build(MysqlQueryBuilder),
        DbBackend::Postgres => query.build(PostgresQueryBuilder),
        _ => query.build(SqliteQueryBuilder),
    };
    (sql, values.0)
}

#[test]
fn qualified_path_contains_keeps_native_json_and_binds_only_the_needle() -> Result<()> {
    let context = context();
    let selector = "users.options->nested->items[0][1]->lang";
    let json_binding = Value::Json(Some(Box::new(json!("en"))));
    for (backend, expected, binding) in [
        (
            DbBackend::MySql,
            r#"SELECT * FROM `users` WHERE JSON_CONTAINS(`users`.`options`, ?, '$."nested"."items"[0][1]."lang"')"#,
            json_binding.clone(),
        ),
        (
            DbBackend::Postgres,
            r#"SELECT * FROM "users" WHERE ("users"."options"->E'nested'->E'items'->0->1->E'lang')::jsonb @> $1"#,
            json_binding,
        ),
        (
            DbBackend::Sqlite,
            r#"SELECT * FROM "users" WHERE exists (select 1 from json_each("users"."options", '$."nested"."items"[0][1]."lang"') where "json_each"."value" is ?)"#,
            "en".into(),
        ),
    ] {
        let expr = predicate(&context, backend, selector, json!("en"), false)?;
        assert_eq!(
            build(backend, Condition::all().add(expr)),
            (expected.into(), vec![binding])
        );
    }
    Ok(())
}

#[test]
fn nested_or_not_contains_does_not_move_the_owner_or_its_bindings() -> Result<()> {
    let context = context();
    let selector = "users.options->languages";
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let contains = predicate(&context, backend, selector, json!("en"), false)?;
        let not_contains = predicate(&context, backend, selector, json!("fr"), true)?;
        let nested = PredicateChain::all()
            .add_comparison(contains.clone(), contains, Vec::new(), false)
            .add_comparison(not_contains.clone(), not_contains, Vec::new(), true);
        let chain = PredicateChain::all()
            .add(Expr::col(Alias::new("owner_id")).eq(41_i64))
            .add_nested(nested, context.clone())
            .add(Expr::col(Alias::new("active")).eq(true));
        let (sql, values) = build(backend, chain.compile(&context, false)?);
        let invalid_shape = || Error::custom("query_json_contains_sql_shape", sql.clone());
        let (owner, group) = sql.split_once(" AND (").ok_or_else(invalid_shape)?;
        let (group, active) = group.rsplit_once(") AND ").ok_or_else(invalid_shape)?;
        assert!(
            owner.contains("owner_id") && active.contains("active"),
            "{sql}"
        );
        let (contains, not_contains) = group.split_once(" OR ").ok_or_else(invalid_shape)?;
        assert!(
            !contains.contains("NOT (") && not_contains.contains("NOT ("),
            "{sql}"
        );
        let expected = if backend == DbBackend::Sqlite {
            vec![41_i64.into(), "en".into(), "fr".into(), true.into()]
        } else {
            vec![
                41_i64.into(),
                Value::Json(Some(Box::new(json!("en")))),
                Value::Json(Some(Box::new(json!("fr")))),
                true.into(),
            ]
        };
        assert_eq!(values, expected);
        assert_eq!(build(backend, chain.compile(&context, true)?).1, expected);
    }
    Ok(())
}

#[test]
fn selector_rejects_identifier_injection_and_nul_but_preserves_path_quotes() -> Result<()> {
    let context = context();
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        for (selector, code) in [
            (
                "users.options; DROP TABLE users->tag",
                "query_identifier_invalid",
            ),
            ("users.options->a\0b", "query_json_path_invalid"),
        ] {
            assert!(matches!(
                predicate(&context, backend, selector, json!("en"), false),
                Err(error) if error.code() == code
            ));
        }
        let expr = predicate(
            &context,
            backend,
            "users.options->a'b ? $1 as key",
            json!("en"),
            false,
        )?;
        let (sql, values) = build(backend, Condition::all().add(expr));
        assert!(sql.contains("a''b ? $1 as key"));
        assert_eq!(values.len(), 1);
    }
    Ok(())
}

#[test]
fn selector_splits_index_before_final_newline_like_laravel() -> Result<()> {
    let context = context();
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let ordinary = predicate(
            &context,
            backend,
            "users.options->items[0]",
            json!("en"),
            false,
        )?;
        let final_newline = predicate(
            &context,
            backend,
            "users.options->items[0]\n",
            json!("en"),
            false,
        )?;
        assert_eq!(
            build(backend, Condition::all().add(ordinary)),
            build(backend, Condition::all().add(final_newline))
        );
    }
    Ok(())
}
