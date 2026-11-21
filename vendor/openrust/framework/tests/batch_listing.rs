use openrust::{bus::DatabaseBatchRepository, config::Repository, database::DatabaseManager};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

async fn exercise(url: Option<String>) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("batches.sqlite");
    std::fs::File::create(&database).unwrap();
    let config = url.map_or_else(
        || json!({"driver":"sqlite","database":database}),
        |url| json!({"url":url}),
    );
    let repository = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":config}}),
    )]));
    let connection = DatabaseManager::new(Arc::new(repository), directory.path())
        .connection(None)
        .unwrap();
    let table = format!("batch_listing_{}", uuid::Uuid::now_v7().simple());
    connection
        .getSchemaBuilder()
        .create(&table, |table| {
            table.string("id", None).primary();
            table.string("name", None);
            table.integer("total_jobs");
            table.integer("pending_jobs");
            table.integer("failed_jobs");
            table.text("failed_job_ids");
            table.text("options").nullable();
            table.integer("cancelled_at").nullable();
            table.integer("created_at");
            table.integer("finished_at").nullable();
        })
        .await
        .unwrap();
    let batches = DatabaseBatchRepository::new(connection.clone(), &table).unwrap();
    let first = batches.store("first", Default::default()).await.unwrap();
    let second = batches.store("second", Default::default()).await.unwrap();
    let third = batches.store("third", Default::default()).await.unwrap();
    let latest = batches.get(2, None).await.unwrap();
    assert_eq!(
        latest
            .iter()
            .map(|batch| batch.id.as_str())
            .collect::<Vec<_>>(),
        vec![third.id.as_str(), second.id.as_str()]
    );
    assert_eq!(batches.get(2, Some(&second.id)).await.unwrap(), vec![first]);
    assert!(batches.get(0, None).await.unwrap().is_empty());
    connection.getSchemaBuilder().drop(&table).await.unwrap();
}
#[tokio::test]
async fn batch_listing_paginates_on_sqlite() {
    exercise(None).await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL 18 fixture"]
async fn batch_listing_paginates_on_postgres() {
    assert_eq!(
        std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
        Ok("1")
    );
    exercise(Some(std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap())).await;
}
#[tokio::test]
#[ignore = "requires isolated MySQL 8 fixture"]
async fn batch_listing_paginates_on_mysql() {
    assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
    exercise(Some(std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap())).await;
}
