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
