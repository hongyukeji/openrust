//! 固定 Laravel 的 JSON 路径存在条件和绑定顺序。

use super::*;
use sea_orm::sea_query::{
    Alias, Asterisk, Condition, MysqlQueryBuilder, PostgresQueryBuilder, Query, SqliteQueryBuilder,
};

fn field() -> Expr {
    Expr::col((Alias::new("users"), Alias::new("options")))
}

fn build(backend: DbBackend, condition: Condition) -> (String, Vec<sea_orm::Value>) {
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
fn qualified_paths_preserve_each_dialect_and_bind_no_path_value() -> Result<()> {
    for (backend, expected) in [
        (
            DbBackend::MySql,
            r#"IFNULL(JSON_CONTAINS_PATH(`users`.`options`, 'one', '$."items"[0][1]'), 0)"#,
        ),
        (
            DbBackend::Postgres,
            r#"CASE WHEN jsonb_typeof(("users"."options"->E'items'->0)::jsonb) = 'array' THEN jsonb_array_length(("users"."options"->E'items'->0)::jsonb) >= 2 ELSE FALSE END"#,
        ),
        (
            DbBackend::Sqlite,
            r#"JSON_TYPE("users"."options", '$."items"[0][1]') IS NOT NULL"#,
        ),
    ] {
        let expression = predicate(backend, field(), Some("items[0][1]"), false)?;
        assert_eq!(
            build(backend, Condition::all().add(expression)),
            (
                format!(
                    "SELECT * FROM {} WHERE {expected}",
                    if backend == DbBackend::MySql {
                        "`users`"
                    } else {
                        "\"users\""
                    }
                ),
                Vec::new()
            )
        );
    }
    Ok(())
}

#[test]
fn postgres_terminal_indices_follow_the_fixed_integer_and_bracket_rules() -> Result<()> {
    for (path, column, threshold) in [
        ("-1", r#""users"."options""#, 1),
        ("items->+1", r#""users"."options"->E'items'"#, 2),
        ("items[-2]", r#""users"."options"->E'items'"#, 2),
        ("items[-0]", r#""users"."options"->E'items'"#, 1),
        ("items[01]\n", r#""users"."options"->E'items'"#, 2),
        ("[0]", r#""users"."options"->E''"#, 1),
        (
            "items[999999999999999999999]",
            r#""users"."options"->E'items'"#,
            2_147_483_648_u64,
        ),
    ] {
        let expression = predicate(DbBackend::Postgres, field(), Some(path), false)?;
        let (sql, values) = build(DbBackend::Postgres, Condition::all().add(expression));
        assert!(
            sql.contains(&format!("jsonb_typeof(({column})::jsonb) = 'array'")),
            "{path}: {sql}"
        );
        assert!(
            sql.contains(&format!(
                "jsonb_array_length(({column})::jsonb) >= {threshold}"
            )),
            "{path}: {sql}"
        );
        assert_eq!(values, Vec::new());
    }
    for (path, key) in [
        ("items->01", "E'01'"),
        ("items[+1]", "E'items[+1]'"),
        ("items->a'b ? $1", "E'a''b ? $1'"),
        ("items->a\\b", r"E'a\\b'"),
    ] {
        let expression = predicate(DbBackend::Postgres, field(), Some(path), false)?;
        let (sql, values) = build(DbBackend::Postgres, Condition::all().add(expression));
        assert!(
            sql.contains(&format!("::jsonb ? {key}, FALSE)")),
            "{path}: {sql}"
        );
        assert_eq!(values, Vec::new());
    }
    Ok(())
}

#[test]
fn not_and_or_scope_keep_surrounding_bindings_and_path_errors_are_explicit() -> Result<()> {
    let context = QueryContext {
        db: super::super::Db::default(),
        table: "users".into(),
        transaction: None,
    };
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let exists = predicate(backend, field(), Some("a'b ? $1"), false)?;
        let missing = predicate(backend, field(), Some("hidden"), true)?;
        let inner = PredicateChain::all()
            .add_comparison(exists.clone(), exists, Vec::new(), false)
            .add_comparison(missing.clone(), missing, Vec::new(), true);
        let outer = PredicateChain::all()
            .add(Expr::col(Alias::new("owner_id")).eq(7_i64))
            .add_nested(inner, context.clone())
            .add(Expr::col(Alias::new("label")).eq("visible"));
        for for_bindings in [false, true] {
            let (sql, values) = build(backend, outer.compile(&context, for_bindings)?);
            let invalid = || Error::custom("query_json_key_sql_shape", sql.clone());
            let (owner, group) = sql.split_once(" AND (").ok_or_else(invalid)?;
            let (group, label) = group.rsplit_once(") AND ").ok_or_else(invalid)?;
            let (positive, negative) = group.split_once(" OR ").ok_or_else(invalid)?;
            assert!(
                owner.contains("owner_id") && label.contains("label"),
                "{sql}"
            );
            assert!(positive.contains("a''b ? $1"), "{sql}");
            assert!(
                negative.trim_start_matches('(').starts_with("NOT "),
                "{sql}"
            );
            assert_eq!(values, vec![7_i64.into(), "visible".into()]);
        }
        assert!(matches!(predicate(backend, field(), Some("x\0y"), false),
            Err(error) if error.code() == "query_json_path_invalid"));
        if backend == DbBackend::Sqlite {
            let expr = predicate(backend, field(), None, false)?;
            assert!(
                build(backend, Condition::all().add(expr))
                    .0
                    .contains("JSON_TYPE(\"users\".\"options\") IS NOT NULL")
            );
        } else {
            assert!(matches!(predicate(backend, field(), None, false),
                Err(error) if error.code() == "query_json_key_path_required"));
        }
    }
    Ok(())
}
