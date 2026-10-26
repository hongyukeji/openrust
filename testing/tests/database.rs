//! 验证测试数据库实例会在隔离数据库中运行迁移。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use openrust::database::sea_orm_migration::{self, MigratorTrait};
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;
use testing::TestDatabase;

struct TestMigrator;

impl MigratorTrait for TestMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(CreateProbe)]
    }
}

struct CreateProbe;

impl MigrationName for CreateProbe {
    fn name(&self) -> &'static str {
        "m20261025_000002_create_openrust_isolation_probe"
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateProbe {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(IsolationProbe::Table)
                    .col(
                        ColumnDef::new(IsolationProbe::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(IsolationProbe::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum IsolationProbe {
    Table,
    Id,
}

#[tokio::test]
async fn each_test_database_has_its_own_migrated_schema_and_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database isolation: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let first = TestDatabase::create::<TestMigrator>(&url).await?;
    let second = TestDatabase::create::<TestMigrator>(&url).await?;
    assert_ne!(first.url(), second.url());

    let first_connection = first.connection()?;
    let second_connection = second.connection()?;
    first_connection
        .execute_unprepared("INSERT INTO isolation_probe (id) VALUES (1)")
        .await?;
    let count = second_connection
        .query_one_raw(Statement::from_string(
            second_connection.get_database_backend(),
            "SELECT COUNT(*) AS row_count FROM isolation_probe".to_owned(),
        ))
        .await?
        .ok_or("isolated probe query returned no row")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(count, 0);
    drop((first_connection, second_connection));
    first.close().await?;
    second.close().await?;
    Ok(())
}
