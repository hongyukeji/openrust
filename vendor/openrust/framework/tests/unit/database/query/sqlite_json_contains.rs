//! 固定 SQLite JSON 包含语法与 PDO 标量绑定回归。

use super::*;
use sea_orm::sea_query::{Asterisk, ExprTrait, Query, SqliteQueryBuilder};
use serde_json::json;

#[test]
fn sqlite_contains_binds_one_pdo_typed_value_between_neighbouring_conditions() -> Result<()> {
    for (needle, expected) in [
        (json!("' OR 1=1 -- 中文"), "' OR 1=1 -- 中文".into()),
        (json!(true), 1_i64.into()),
        (json!(false), 0_i64.into()),
        (json!(null), Value::String(None)),
        (json!(i64::MIN), i64::MIN.into()),
        (json!(i64::MAX), i64::MAX.into()),
        (json!(1.0), "1".into()),
        (json!(-0.0), "-0".into()),
        (json!(1e20), "1.0E+20".into()),
        (json!(1e-7), "1.0E-7".into()),
        (json!(1.234_567_890_123_456), "1.2345678901235".into()),
        (
            json!(9_223_372_036_854_775_808_u64),
            "9.2233720368548E+18".into(),
        ),
        (json!(u64::MAX), "1.844674407371E+19".into()),
        (json!(1e308), "1.0E+308".into()),
    ] {
        let (sql, values) = Query::select()
            .column(Asterisk)
            .from(Alias::new("users"))
            .and_where(Expr::col(Alias::new("tenant_id")).eq(41_i64))
            .and_where(predicate(DbBackend::Sqlite, "options", needle)?)
            .and_where(Expr::col(Alias::new("active")).eq(true))
            .build(SqliteQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT * FROM "users" WHERE "tenant_id" = ?"#,
                // SeaQuery 对并列条件的自定义表达式保留分组。
                r#" AND (exists (select 1 from json_each("options") where "json_each"."value" is ?))"#,
                r#" AND "active" = ?"#,
            )
        );
        assert_eq!(values.0, vec![41_i64.into(), expected, true.into()]);
    }
    Ok(())
}

#[test]
fn sqlite_rejects_structured_needles_outside_the_official_supported_inputs() {
    for needle in [
        json!([]),
        json!(["en"]),
        json!(["en", "de"]),
        json!({"language": "en"}),
    ] {
        assert!(matches!(
            predicate(DbBackend::Sqlite, "options", needle),
            Err(error) if error.code() == "query_sqlite_json_contains_requires_scalar"
        ));
    }
}
