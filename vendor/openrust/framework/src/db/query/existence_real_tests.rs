//! 真实 MySQL/PostgreSQL 的存在性解码、查询形状及事务可见性。

use super::*;
use crate::{
    db::{
        Db,
        sea_orm::TransactionTrait,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
async fn real_backend_exists_preserves_null_paging_union_and_transaction() -> Result<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过真实存在性查询：未配置 OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let outcome: Result<()> = async {
        let connection = database.connection()?;
        connection
            .execute_unprepared("CREATE TABLE existence_items (id BIGINT, label TEXT NULL)")
            .await
            .map_err(Error::internal)?;
        let db = Db::default();
        db.initialize(connection)?;
        assert!(!db.table("existence_items")?.exists().await?);
        assert!(db.table("existence_items")?.doesnt_exist().await?);
        db.table("existence_items")?
            .insert(vec![
                ("id", 1.into()),
                ("label", Option::<String>::None.into()),
            ])
            .await?;
        let query = db
            .table("existence_items")?
            .select(&["label"])?
            .where_null("label")?;
        assert!(query.exists().await?);
        assert!(!query.doesnt_exist().await?);
        assert_eq!(query.count().await?, 1);
        assert!(!query.clone().limit(0).exists().await?);
        assert!(!query.clone().limit(1).offset(1).exists().await?);
        let absent = db
            .table("existence_items")?
            .select(&["label"])?
            .where_eq("id", 2)?;
        assert!(absent.union(&query, true)?.exists().await?);
        let transaction = db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        db.table("existence_items")?
            .using_transaction(&transaction)
            .insert(vec![("id", 2.into())])
            .await?;
        assert!(
            db.table("existence_items")?
                .using_transaction(&transaction)
                .where_eq("id", 2)?
                .exists()
                .await?
        );
        assert!(
            db.table("existence_items")?
                .where_eq("id", 2)?
                .doesnt_exist()
                .await?
        );
        transaction.rollback().await.map_err(Error::internal)?;
        assert!(
            db.table("existence_items")?
                .where_eq("id", 2)?
                .doesnt_exist()
                .await?
        );
        assert!(db.table("missing_existence_table")?.exists().await.is_err());
        assert!(
            db.table("missing_existence_table")?
                .doesnt_exist()
                .await
                .is_err()
        );
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}
