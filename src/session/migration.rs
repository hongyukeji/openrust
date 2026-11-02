//! Session独立迁移；认证迁移复用此唯一表定义。

use crate::db::sea_orm::sea_query::Alias;
use crate::db::{DatabaseTableNames, dialect, sea_orm_migration::prelude::*};
use sha2::{Digest, Sha256};

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
            crate::db::sea_orm::DbBackend::Postgres | crate::db::sea_orm::DbBackend::Sqlite => {
                data.text();
            }
            _ => {
                return Err(DbErr::Migration(
                    "Session supports PostgreSQL, MySQL and SQLite".into(),
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

/// 已有 Session 表的截止索引升级；自定义 Migrator 在建表迁移后登记一次。
///
/// 不改变既有记录或迁移版本；同名索引冲突正常失败，不静默接受异列定义。
pub struct SessionGcMigration;

fn expiry_index() -> Result<String, DbErr> {
    let names = DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("Session 索引迁移需要表名作用域".into()))?;
    let physical = names
        .resolve("sessions")
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    let digest = format!("{:x}", Sha256::digest(physical.as_bytes()));
    // 摘要同时隔离同schema不同物理表，完整索引名不超过PG/MySQL长度限制。
    Ok(format!("ix_sessions_expiry_{}", &digest[..40]))
}

impl MigrationName for SessionGcMigration {
    fn name(&self) -> &'static str {
        "m20261030_000020_openrust_session_gc_index"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for SessionGcMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name(expiry_index()?)
                    .table(table(manager)?)
                    .col(Columns::ExpiresAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name(expiry_index()?)
                    .table(table(manager)?)
                    .to_owned(),
            )
            .await
    }
}
