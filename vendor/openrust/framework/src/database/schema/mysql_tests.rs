//! 日常真实MySQL：列序、映射、primary及同源CTX；未执行。
use super::{Db, Error, Result};
use crate::{
    database::{
        DatabaseTableNames,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use futures_util::FutureExt as _;
use sea_orm::ConnectionTrait;
use std::{collections::BTreeMap, panic::AssertUnwindSafe};
struct Empty;
impl MigratorTrait for Empty {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
fn finish(stage: &str, result: std::thread::Result<Result<()>>, cleanup: Result<()>) -> Result<()> {
    if let Err(error) = &cleanup {
        eprintln!(
            "schema_mysql_cleanup_failed stage={stage} code={}",
            error.code()
        );
    }
    match result {
        Ok(Err(error)) => Err(error),
        Ok(Ok(())) => cleanup,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
#[tokio::test]
#[ignore = "真实MySQL8.4日常专跑：需OPENRUST_TEST_DATABASE_URL与独立库权限"]
async fn mysql_listing_uses_original_order_mapping_and_same_source_transaction() -> Result<()> {
    let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    if !url.starts_with("mysql://") {
        return Err(Error::custom(
            "test_database_backend_mismatch",
            "Schema日常验收需要MySQL",
        ));
    }
    let database = TestDatabase::create::<Empty>(&url).await?;
    let outcome = AssertUnwindSafe(async {
        let mut connection = database.connection()?;
        let metrics = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = std::sync::Arc::clone(&metrics);
        connection.set_metric_callback(move |_| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let (rows, lease) = connection
            .query_all_with_read_lease(sea_orm::Statement::from_string(
                sea_orm::DbBackend::MySql,
                "select CAST(17 AS SIGNED) as probe".to_owned(),
            ))
            .await
            .map_err(Error::internal)?;
        assert!(lease.is_none());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .try_get::<i64>("", "probe")
                .map_err(Error::internal)?,
            17
        );
        assert_eq!(metrics.load(std::sync::atomic::Ordering::SeqCst), 1);

        connection
            .execute_unprepared("CREATE TABLE schema_physical(z BIGINT,a TEXT,`01` TEXT)")
            .await
            .map_err(Error::internal)?;
        let db = Db::with_table_names(DatabaseTableNames::new(
            "ignored_",
            BTreeMap::from([("probe".into(), "schema_physical".into())]),
        )?);
        db.initialize(connection.clone())?;
        assert_eq!(
            db.schema().get_column_listing("probe").await?,
            ["z", "a", "01"]
        );
        assert_eq!(metrics.load(std::sync::atomic::Ordering::SeqCst), 2); // Schema None分支不能重放首query。
        assert_eq!(
            db.schema().get_column_listing("missing").await?,
            [] as [String; 0]
        );
        assert_eq!(
            db.schema().get_column_listing("x' OR 1=1 --").await?,
            [] as [String; 0]
        );
        primary_over_replica(&url, connection.clone()).await?;
        let alias = Db::default();
        alias.initialize(connection)?;
        let owner = db.clone();
        db.transaction_for_update_checked_context(
            move |context| {
                Box::pin(async move {
                    assert_eq!(
                        alias
                            .schema()
                            .using_transaction(&context)
                            .err()
                            .map(|e| e.code().to_owned())
                            .as_deref(),
                        Some("schema_transaction_source_mismatch")
                    );
                    assert_eq!(
                        owner
                            .schema()
                            .using_transaction(&context)?
                            .get_column_listing("probe")
                            .await?,
                        ["z", "a", "01"]
                    );
                    Ok(())
                })
            },
            |_| Box::pin(async { Ok(()) }),
        )
        .await?;
        Ok(())
    })
    .catch_unwind()
    .await;
    finish("primary_close", outcome, database.close().await)
}

async fn primary_over_replica(url: &str, primary: sea_orm::DatabaseConnection) -> Result<()> {
    let reader = TestDatabase::create::<Empty>(url).await?;
    let outcome = AssertUnwindSafe(async {
        let replica = reader.connection()?;
        replica
            .execute_unprepared("CREATE TABLE schema_physical(reader_only BIGINT)")
            .await
            .map_err(Error::internal)?;
        let db = Db::with_table_names(DatabaseTableNames::new(
            "",
            BTreeMap::from([("probe".into(), "schema_physical".into())]),
        )?);
        db.initialize_with_replicas(primary, vec![replica])?;
        assert_eq!(
            db.schema().get_column_listing("probe").await?,
            ["z", "a", "01"]
        );
        assert_eq!(db.next_replica.load(std::sync::atomic::Ordering::SeqCst), 0);
        Ok(())
    })
    .catch_unwind()
    .await;
    finish("replica_close", outcome, reader.close().await)
}
