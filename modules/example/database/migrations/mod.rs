//! 仅属于 `example` 模块的业务数据库迁移。

use openrust::database::sea_orm_migration::prelude::*;

pub(crate) struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(CreateExampleHeartbeats)]
    }
}

struct CreateExampleHeartbeats;

impl MigrationName for CreateExampleHeartbeats {
    fn name(&self) -> &'static str {
        "m20261028_000001_example_heartbeats"
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateExampleHeartbeats {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ExampleHeartbeats::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ExampleHeartbeats::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ExampleHeartbeats::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ExampleHeartbeats {
    Table,
    Id,
}
