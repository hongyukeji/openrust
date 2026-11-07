//! 检出引用归一、错误默认schema、绑定顺序与旧DDL准入被放宽。
use super::{
    Db, DbBackend, Error, Result,
    grammar::{compile, parse},
};
use crate::database::DatabaseTableNames;
use std::collections::BTreeMap;

#[test]
fn references_are_literal_and_have_at_most_two_parts() -> Result<()> {
    for (input, expected) in [
        ("", (None, "")),
        (" . t ", (Some(" "), " t ")),
        (".t", (Some(""), "t")),
        ("0.t", (Some("0"), "t")),
        ("名.表", (Some("名"), "表")),
    ] {
        assert_eq!(parse(input)?, expected);
    }
    for reference in ["a.b.c", "..", "a..b"] {
        let error = parse(reference).err().ok_or(Error::Unauthorized)?;
        assert_eq!(error.code(), "schema_reference_invalid");
        assert_eq!(error.status().as_u16(), 500);
    }
    Ok(())
}
fn values(statement: &sea_orm::Statement) -> Vec<sea_orm::Value> {
    statement
        .values
        .as_ref()
        .map_or_else(Vec::new, |values| values.0.clone())
}
#[test]
fn catalog_binding_uses_same_mapping_without_weakening_ddl_names() -> Result<()> {
    let names = DatabaseTableNames::new(
        "pre_",
        BTreeMap::from([("users".into(), "accounts".into())]),
    )?;
    let mysql = compile(DbBackend::MySql, &names, "users")?;
    assert_eq!(values(&mysql.columns), vec!["accounts".into()]);
    assert!(mysql.columns.sql.contains("table_schema = schema()"));
    let hostile = "x' OR 1=1 --";
    let mysql = compile(DbBackend::MySql, &names, hostile)?;
    assert_eq!(
        values(&mysql.columns),
        vec![format!("pre_{hostile}").into()]
    );
    assert!(!mysql.columns.sql.contains(hostile));
    assert!(names.resolve(hostile).is_err());
    let pg = compile(DbBackend::Postgres, &names, "quoted'schema.users")?;
    assert_eq!(
        values(&pg.columns),
        vec!["accounts".into(), "quoted'schema".into()]
    );
    assert!(pg.columns.sql.contains("a.atttypid = t.oid"));
    assert!(pg.columns.sql.ends_with("order by a.attnum"));
    assert!(!pg.columns.sql.contains("attgenerated"));
    Ok(())
}
#[test]
fn schema_defaults_and_sqlite_identifier_escape_are_distinct() -> Result<()> {
    let names = DatabaseTableNames::default();
    for schema in ["", "0"] {
        assert_eq!(
            values(&compile(DbBackend::MySql, &names, &format!("{schema}.t"))?.columns),
            vec!["t".into()]
        );
        assert_eq!(
            values(&compile(DbBackend::Postgres, &names, &format!("{schema}.t"))?.columns),
            vec!["t".into()]
        );
        assert_eq!(
            values(&compile(DbBackend::Sqlite, &names, &format!("{schema}.t"))?.columns),
            vec!["t".into(), schema.into()]
        );
    }
    let query = compile(DbBackend::Sqlite, &names, "a\"b.t'")?;
    assert_eq!(values(&query.columns), vec!["t'".into(), "a\"b".into()]);
    let second = query.definition.ok_or(Error::Unauthorized)?;
    assert!(second.sql.contains("\"a\"\"b\".sqlite_master"));
    assert_eq!(values(&second), vec!["table".into(), "t'".into()]);
    for invalid in ["*.t", "a\0b.t", "main.t\0suffix"] {
        assert_eq!(
            compile(DbBackend::Sqlite, &names, invalid)
                .err()
                .map(|e| e.code().to_owned())
                .as_deref(),
            Some("schema_identifier_invalid")
        );
    }
    Ok(())
}
#[tokio::test]
async fn view_is_lazy_and_reference_error_precedes_uninitialized_query() -> Result<()> {
    let schema = Db::default().schema();
    assert_eq!(
        schema
            .get_column_listing("a.b.c")
            .await
            .err()
            .map(|e| e.code().to_owned())
            .as_deref(),
        Some("schema_reference_invalid")
    );
    assert!(schema.get_column_listing("t").await.is_err());
    Ok(())
}
