use std::path::Path;

use serde_json::json;

use crate::database::{Row, connectors::ConnectionFactory};

use super::{Grammar, drop_statement};

#[tokio::test]
async fn postgres_wipe_quotes_catalog_names_and_honors_table_exclusions() {
    let factory = ConnectionFactory;
    let connection = factory
        .make(
            "pgsql",
            json!({"driver":"pgsql","prefix":"app_"}),
            Path::new("."),
        )
        .unwrap();
    assert_eq!(connection.postgres_dont_drop.as_ref(), ["spatial_ref_sys"]);
    let connection = factory
        .make(
            "pgsql",
            json!({"driver":"pgsql","prefix":"app_","dont_drop":["app_keep","other.notes"]}),
            Path::new("."),
        )
        .unwrap();

    let table = |schema: &str, name: &str| {
        Row::from_columns(vec![
            ("schema".into(), schema.into()),
            ("name".into(), name.into()),
        ])
    };
    let rows = [
        table("public", "app_keep"),
        table("other", "notes"),
        table("public", "notes"),
        table("name.with.dot", "a\"b"),
    ];
    let statement = drop_statement(
        &connection.grammar,
        "table",
        &rows,
        &connection.postgres_dont_drop,
    )
    .unwrap();
    assert_eq!(
        statement.as_deref(),
        Some(r#"drop table if exists "public"."notes", "name.with.dot"."a""b" cascade"#)
    );
    let grammar = Grammar::postgres("");
    assert!(
        drop_statement(
            &grammar,
            "table",
            &rows[..2],
            &connection.postgres_dont_drop
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(
        drop_statement(&grammar, "domain", &rows[2..3], &[])
            .unwrap()
            .as_deref(),
        Some(r#"drop domain if exists "public"."notes" cascade"#)
    );
}
