use openrust::database::{DatabaseError, Value, connectors::ConnectionFactory};
use serde_json::json;

#[tokio::test]
async fn schema_and_query_over_mariadb() {
    let url = std::env::var("OPENRUST_TEST_MARIADB_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_MARIADB").is_none());
    let Some(url) = url else { return };
    let root = tempfile::tempdir().unwrap();
    let db = ConnectionFactory
        .make("mariadb", json!({"url":url}), root.path())
        .unwrap();
    let Value::Text(version) = db.scalar("select version()", &[]).await.unwrap() else {
        panic!("MariaDB server version was not text");
    };
    let table = format!("mariadb_release_{:016x}", fastrand::u64(..));
    let mut schema = db.get_schema_builder();
    schema
        .create(&table, |table| {
            table.id();
            table.uuid("token");
            table
                .json("document")
                .default(Value::Json(json!({"title":"new"})));
            table.string("title", 80).virtual_as_json("document->title");
            table
                .string("old,name", 50)
                .collation("utf8mb4_bin")
                .default("a,b'()")
                .comment("retained, comment");
        })
        .await
        .unwrap();
    let columns = schema.get_columns(&table).await.unwrap();
    let token = columns
        .iter()
        .find(|column| column.name == "token")
        .unwrap();
    if version.starts_with("10.3.") {
        assert_eq!(token.full_type, "char(36)");
    } else if version.starts_with("11.") {
        assert_eq!(token.full_type, "uuid");
    }
    let id = db
        .table(&table)
        .insert_get_id(
            [(
                "token".into(),
                "de305d54-75b4-431b-adb2-eb6b9e546014".into(),
            )]
            .into_iter()
            .collect(),
        )
        .await
        .unwrap();
    assert_eq!(db.table(&table).value("title").await.unwrap(), "new".into());
    schema
        .table(&table, |table| table.rename_column("old,name", "renamed"))
        .await
        .unwrap();
    assert_eq!(
        db.table(&table).value("renamed").await.unwrap(),
        Value::Bytes(b"a,b'()".to_vec())
    );
    let columns = schema.get_columns(&table).await.unwrap();
    let renamed = columns
        .iter()
        .find(|column| column.name == "renamed")
        .unwrap();
    assert_eq!(renamed.collation.as_deref(), Some("utf8mb4_bin"));
    assert_eq!(renamed.comment.as_deref(), Some("retained, comment"));
    assert_eq!(
        db.table(&table)
            .r#where("id", "=", id)
            .update(
                [
                    ("document->title".into(), "changed".into()),
                    ("document->enabled".into(), true.into()),
                    (
                        "document->nested".into(),
                        Value::Json(json!({"present":true}))
                    ),
                ]
                .into_iter()
                .collect()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table(&table).value("title").await.unwrap(),
        "changed".into()
    );
    assert_eq!(
        db.table(&table)
            .where_json_contains("document->nested->present", json!(true))
            .where_json_contains("document->enabled", json!(true))
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        db.table(&table)
            .join_lateral(&mut db.table(&table), "inner_table"),
        Err(DatabaseError::InvalidQuery(_))
    ));
    schema.drop_if_exists(&table).await.unwrap();
}
