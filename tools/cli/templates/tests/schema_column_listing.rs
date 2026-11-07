//! 生成应用实际使用所选Db的公开Schema列清单与同源事务；真实服务显式专跑。
use openrust::{
    Error,
    database::{
        Db,
        sea_orm::ConnectionTrait,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
#[tokio::test]
#[ignore = "{{schema_test_reason}}"]
async fn generated_schema_preserves_catalog_order_and_transaction_source()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    // {{schema_database_setup}}
    // getter返回Err也在worker内传回，数据库owner一直留在外层。
    let connection = database.connection();
    let outcome = tokio::spawn(async move {
        let db = Db::default();
        db.initialize(connection?)?;
        db.connection()?
            .execute_unprepared("CREATE TABLE schema_probe (z INTEGER,a TEXT)")
            .await
            .map_err(Error::internal)?;
        assert_eq!(
            db.schema().get_column_listing("schema_probe").await?,
            ["z", "a"]
        );
        assert!(db.schema().get_column_listing("missing").await?.is_empty());
        let owner = db.clone();
        db.transaction_for_update_checked_context(
            move |context| {
                Box::pin(async move {
                    assert_eq!(
                        owner
                            .schema()
                            .using_transaction(&context)?
                            .get_column_listing("schema_probe")
                            .await?,
                        ["z", "a"]
                    );
                    Ok(())
                })
            },
            |_| Box::pin(async { Ok(()) }),
        )
        .await?;
        Ok::<(), Error>(())
    })
    .await;
    let cleanup = database.close().await;
    if let Err(error) = &cleanup {
        eprintln!(
            "generated_schema_cleanup_failed stage=database_close code={}",
            error.code()
        );
    }
    match outcome {
        Ok(Err(primary)) => Err(primary.into()),
        Ok(Ok(())) => {
            cleanup?;
            Ok(())
        }
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => Err(Error::internal(error).into()),
    }
}
