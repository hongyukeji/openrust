use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{Connection, DatabaseManager, Value},
};
use serde_json::{Value as Json, json};

fn connection(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

#[tokio::test]
async fn non_mysql_position_modifiers_do_not_change_generated_sql() {
    for driver in ["pgsql", "sqlite", "sqlsrv"] {
        let db = connection(json!({"driver":driver,"database":":memory:"}));
        let mut variants = Vec::new();
        for positioned in [false, true] {
            let queries = db
                .pretend(move |db| {
                    Box::pin(async move {
                        let mut schema = db.getSchemaBuilder();
                        schema
                            .create("positioned", |table| {
                                table.string("password", 80);
                                let column = table.text("secret");
                                column.nullable();
                                if positioned {
                                    column.after("password");
                                }
                                let column = table.string("token", 80);
                                if positioned {
                                    column.first();
                                }
                            })
                            .await?;
                        schema
                            .table("positioned", |table| {
                                let column = table.text("extra_secret");
                                column.nullable();
                                if positioned {
                                    column.after("password");
                                }
                                let column = table.string("extra_token", 80);
                                if positioned {
                                    column.first();
                                }
                            })
                            .await?;
                        if driver == "pgsql" {
                            schema
                                .table("positioned", |table| {
                                    let column = table.string("password", 120);
                                    column.change();
                                    if positioned {
                                        column.after("token");
                                    }
                                })
                                .await?;
                        }
                        Ok(())
                    })
                })
                .await
                .unwrap();
            variants.push(
                queries
                    .into_iter()
                    .map(|query| query.query)
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(variants[0], variants[1], "{driver}");
    }
}

#[tokio::test]
async fn mysql_keeps_position_clauses() {
    let db = connection(json!({"driver":"mysql"}));
    let queries = db
        .pretend(|db| {
            Box::pin(async move {
                db.getSchemaBuilder()
                    .table("users", |table| {
                        table.text("secret").nullable().after("password");
                        table.string("token", 80).first();
                        table.string("name", 120).change().after("token");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    let sql = queries
        .iter()
        .map(|query| query.query.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(sql.contains(" after `password`"), "{sql}");
    assert!(sql.contains(" first"), "{sql}");
    assert!(sql.contains(" after `token`"), "{sql}");
}

#[tokio::test]
async fn postgres_migrates_positioned_auth_columns_without_reordering_or_data_loss() {
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES").is_none());
        return;
    };
    let db = connection(json!({"url":url}));
    let mut schema = db.getSchemaBuilder();
    schema
        .create("positioned_auth", |table| {
            table.string("password", 80).first();
            table.string("marker", 80).after("password");
        })
        .await
        .unwrap();
    db.insert(
        "insert into positioned_auth (password, marker) values (?, ?)",
        &["hash".into(), "keep".into()],
    )
    .await
    .unwrap();
    schema
        .table("positioned_auth", |table| {
            table.text("two_factor_secret").nullable().after("password");
            table.string("token", 80).nullable().first();
            table.after("two_factor_secret", |table| {
                table.text("two_factor_recovery_codes").nullable();
                table.timestamp("two_factor_confirmed_at", None).nullable();
            });
        })
        .await
        .unwrap();
    schema
        .table("positioned_auth", |table| {
            table.string("password", 120).change().after("token");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.getColumnListing("positioned_auth").await.unwrap(),
        [
            "password",
            "marker",
            "two_factor_secret",
            "token",
            "two_factor_recovery_codes",
            "two_factor_confirmed_at",
        ]
    );
    let row = db
        .selectOne(
            "select password, marker, two_factor_secret from positioned_auth",
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("password"), Some(&Value::Text("hash".into())));
    assert_eq!(row.get("marker"), Some(&Value::Text("keep".into())));
    assert_eq!(row.get("two_factor_secret"), Some(&Value::Null));
    schema
        .table("positioned_auth", |table| {
            table.dropColumn(&[
                "two_factor_secret",
                "token",
                "two_factor_recovery_codes",
                "two_factor_confirmed_at",
            ]);
        })
        .await
        .unwrap();
    assert_eq!(
        schema.getColumnListing("positioned_auth").await.unwrap(),
        ["password", "marker"]
    );
    schema.drop("positioned_auth").await.unwrap();
}
