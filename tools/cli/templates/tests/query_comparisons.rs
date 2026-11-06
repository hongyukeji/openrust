//! 验证生成应用的值绑定、NULL 比较、条件分组与事务写入复用同一比较契约。

use openrust::{
    Result,
    db::{
        Db,
        query::WhereOperator,
        sea_orm::{ConnectionTrait, TransactionTrait},
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
async fn generated_comparisons_bind_values_and_preserve_null_groups_and_transactions()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    // {{database_setup}}
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        db.connection()?.execute_unprepared(
            "CREATE TABLE comparison_items (id INTEGER PRIMARY KEY, score INTEGER, label VARCHAR(255))",
        ).await.map_err(openrust::Error::internal)?;
        let literal = "bound ' value ? $1";
        for (id, score) in [(1_i64, Some(10_i64)), (2, Some(20)), (3, None)] {
            db.table("comparison_items")?.insert(vec![
                ("id", id.into()), ("score", score.into()), ("label", literal.into()),
            ]).await?;
        }
        assert_eq!(db.table("comparison_items")?.r#where("label", literal)?.count().await?, 3);
        assert_eq!(db.table("comparison_items")?.r#where("score", Option::<i64>::None)?.count().await?, 1);
        assert_eq!(db.table("comparison_items")?.where_compare("score", WhereOperator::NotEq, Option::<i64>::None)?.count().await?, 2);
        assert_eq!(db.table("comparison_items")?.where_compare("score", WhereOperator::NullSafeEq, 10_i64)?.count().await?, 1);
        let transaction = db.connection()?.begin().await.map_err(openrust::Error::internal)?;
        let query = db.table("comparison_items")?.using_transaction(&transaction)
            .where_nested(|group| group.r#where("id", 1_i64)?.or_where_value("id", 2_i64))?
            .where_compare("score", WhereOperator::Ge, 20_i64)?;
        assert_eq!(query.update(vec![("label", "changed".into())]).await?, 1);
        drop(query);
        transaction.rollback().await.map_err(openrust::Error::internal)?;
        assert_eq!(db.table("comparison_items")?.r#where("label", literal)?.count().await?, 3);
        Ok(())
    }.await;
    database.close().await?;
    outcome?;
    Ok(())
}
