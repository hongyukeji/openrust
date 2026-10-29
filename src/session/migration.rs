//! Session独立迁移；认证迁移复用此唯一表定义。

use crate::db::sea_orm::sea_query::Alias;
use crate::db::{DatabaseTableNames, dialect, sea_orm_migration::prelude::*};

/// Session持久表，保留受限JSON原文及微秒级截止。
///
/// 仅无Auth迁移时独立登记；`AuthMigration`已包含同一表，禁止同时登记两者，
/// 避免独立迁移版本回滚时删除另一版本仍使用的表。
pub struct SessionMigration;

#[derive(DeriveIden)]
enum Columns {
    IdHash,
    Data,
    ExpiresAt,
    CreatedAt,
}

fn table(manager: &SchemaManager<'_>) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration("Session migration requires application table-name scope".into())
    })?;
    names
        .quote("sessions", manager.get_database_backend())
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve("sessions")
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}
impl MigrationName for SessionMigration {
    fn name(&self) -> &'static str {
        "m20261029_000010_openrust_sessions"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for SessionMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        // JSONB/JSON会重排键、改写空白或拒绝NUL；受限JSON原文是此唯一store的编码。
        let mut data = ColumnDef::new(Columns::Data);
        match backend {
            crate::db::sea_orm::DbBackend::MySql => {
                data.custom("LONGTEXT");
            }
            crate::db::sea_orm::DbBackend::Postgres => {
                data.text();
            }
            _ => {
                return Err(DbErr::Migration(
                    "Session supports PostgreSQL and MySQL".into(),
                ));
            }
        }
        manager
            .create_table(
                Table::create()
                    .table(table(manager)?)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Columns::IdHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(data.not_null())
                    .col(dialect::timestamp_utc(Columns::ExpiresAt, backend)?)
                    .col(dialect::timestamp_utc(Columns::CreatedAt, backend)?)
                    .to_owned(),
            )
            .await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(table(manager)?).if_exists().to_owned())
            .await
    }
}
