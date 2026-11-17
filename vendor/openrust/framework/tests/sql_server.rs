use openrust::database::{Bindings, Value, connectors::ConnectionFactory, query::Record};
use serde_json::json;

#[tokio::test]
async fn sql_server_tds_bindings_transactions_and_schema() {
    let url = std::env::var("OPENRUST_TEST_SQL_SERVER_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
    let Some(url) = url else { return };
    let root = tempfile::tempdir().unwrap();
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url}), root.path())
        .unwrap();
    let tls = db
        .scalar(
            "select encrypt_option from sys.dm_exec_connections where session_id=@@spid",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(tls, "TRUE".into());
    let rows = db.selectResultSets(
        "select :name as [value], 'literal ? :n' as [literal]; select cast(:amount as decimal(20,4)) as [amount]",
        Bindings::Named(&[("name","测试 ' 声称?".into()), ("amount","1234567890123.1234".into())]), true,
    ).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0][0].get("value"),
        Some(&Value::Text("测试 ' 声称?".into()))
    );
    assert_eq!(
        rows[0][0].get("literal"),
        Some(&Value::Text("literal ? :n".into()))
    );
    assert_eq!(
        rows[1][0].get("amount"),
        Some(&Value::Text("1234567890123.1234".into()))
    );

    let table = format!("sqlserver_release_{:016x}", fastrand::u64(..));
    let mut schema = db.getSchemaBuilder();
    schema
        .create(&table, |table| {
            table.id();
            table.string("name", 64).unique();
            table.integer("score");
            table.binary("blob").nullable();
        })
        .await
        .unwrap();
    let columns = schema.getColumns(&table).await.unwrap();
    assert!(
        columns
            .iter()
            .any(|column| column.name == "id" && column.auto_increment && column.primary)
    );
    assert!(
        schema
            .getSchemas()
            .await
            .unwrap()
            .iter()
            .any(|schema| schema.name == "dbo" && schema.default)
    );
    assert!(
        schema
            .getTables(Some("dbo"))
            .await
            .unwrap()
            .iter()
            .any(|item| item.name == table)
    );
    let record = |name: &str, score: i64| {
        Record::from([("name".into(), name.into()), ("score".into(), score.into())])
    };
    assert_eq!(
        db.table(&table)
            .insertGetId(record("first", 1))
            .await
            .unwrap(),
        1.into()
    );
    let mut transaction = db.beginTransaction().await.unwrap();
    transaction
        .table(&table)
        .insert(&[record("kept", 2)])
        .await
        .unwrap();
    transaction.beginTransaction().await.unwrap();
    transaction
        .table(&table)
        .insert(&[record("rolled back", 3)])
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(db.table(&table).count("*").await.unwrap(), 2);
    db.table(&table)
        .upsert(&[record("first", 5)], &["name"], Some(&["score"]))
        .await
        .unwrap();
    let rows = db
        .table(&table)
        .orderByDesc("score")
        .offset(1)
        .limit(1)
        .get()
        .await
        .unwrap();
    assert_eq!(rows[0].get("name"), Some(&Value::Text("kept".into())));
    let encoded = Value::Bytes(vec![0, 255, 33]);
    db.table(&table)
        .r#where("name", "=", "first")
        .update(Record::from([("blob".into(), encoded.clone())]))
        .await
        .unwrap();
    assert_eq!(
        db.table(&table)
            .r#where("name", "=", "first")
            .value("blob")
            .await
            .unwrap(),
        encoded
    );
    schema.dropIfExists(&table).await.unwrap();
}

#[tokio::test]
async fn sql_server_online_indexes_and_named_foreign_key_drop() {
    let root = tempfile::tempdir().unwrap();
    let preview = ConnectionFactory
        .make("sqlsrv", json!({"driver":"sqlsrv"}), root.path())
        .unwrap();
    let statements = preview
        .pretend(|db| {
            Box::pin(async move {
                db.getSchemaBuilder()
                    .table("dbo.entries", |table| {
                        table.index(&["score"], Some("search")).online();
                        table.unique(&["ref"], Some("unique_ref")).online();
                        table.dropForeign("entry]parent");
                    })
                    .await
            })
        })
        .await
        .unwrap();
    assert_eq!(
        statements
            .iter()
            .map(|entry| entry.query.as_str())
            .collect::<Vec<_>>(),
        [
            "create index [search] on [dbo].[entries] ([score]) with (online = on)",
            "create unique index [unique_ref] on [dbo].[entries] ([ref]) with (online = on)",
            "alter table [dbo].[entries] drop constraint [entry]]parent]",
        ]
    );

    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = ConnectionFactory
        .make("sqlsrv", json!({"url":url}), root.path())
        .unwrap();
    let suffix = fastrand::u64(..);
    let parent = format!("dbo.parents_{suffix:016x}");
    let child = format!("dbo.entries_{suffix:016x}");
    let foreign = format!("entry_parent_{suffix:016x}");
    let mut schema = db.getSchemaBuilder();
    schema
        .create(&parent, |table| {
            table.id();
        })
        .await
        .unwrap();
    schema
        .create(&child, |table| {
            table.id();
            table.bigInteger("parent_id");
            table.integer("score");
            table.integer("ref");
            table
                .foreign(&["parent_id"], Some(&foreign))
                .references("id")
                .on(&parent);
            table.index(&["score"], Some("search")).online();
        })
        .await
        .unwrap();
    let keys = schema.getForeignKeys(&child).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name.as_deref(), Some(foreign.as_str()));
    assert_eq!(keys[0].columns, ["parent_id"]);
    assert_eq!(keys[0].foreign_schema.as_deref(), Some("dbo"));
    assert_eq!(keys[0].foreign_columns, [Some("id".into())]);
    assert_eq!(keys[0].on_delete, "no action");
    schema
        .table(&child, |table| {
            table.unique(&["ref"], Some("unique_ref")).online();
            table.dropForeign(&foreign);
        })
        .await
        .unwrap();

    let indexes = schema.getIndexes(&child).await.unwrap();
    assert!(indexes.iter().any(|index| index.name == "search"
        && !index.unique
        && index.columns == [Some("score".into())]));
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "unique_ref" && index.unique)
    );
    assert!(schema.getForeignKeys(&child).await.unwrap().is_empty());
    schema.dropIfExists(&parent).await.unwrap();
    schema.dropIfExists(&child).await.unwrap();
}
