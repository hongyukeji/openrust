//! 原生 Session 表的显式绝对截止增强；不在默认迁移中隐式安装。

use crate::database::{
    dialect,
    sea_orm::{DbBackend, sea_query::SqliteQueryBuilder},
    sea_orm_migration::prelude::*,
};
use sha2::{Digest, Sha256};

use super::migration::{execute_sqlite_index, table_name};

/// 为已存在的原生 Session 表添加可空 UTC 截止列，不转换或覆盖已有载荷。
pub struct GuardedSessionMigration;

/// 为显式增强 Session 表添加截止索引；重复或异列同名冲突正常失败。
pub struct GuardedSessionGcMigration;

/// 显式逻辑表名的截止列迁移。
pub struct GuardedSessionTableMigration {
    table: String,
    version: String,
}

/// 显式逻辑表名的截止索引迁移。
pub struct GuardedSessionTableGcMigration {
    table: String,
    version: String,
}

#[derive(DeriveIden)]
enum Columns {
    ExpiresAt,
}

impl GuardedSessionMigration {
    /// 复用应用公开表名作用域，为指定逻辑表添加同一增强列。
    pub fn for_table(table: impl Into<String>) -> GuardedSessionTableMigration {
        let table = table.into();
        let digest = format!("{:x}", Sha256::digest(table.as_bytes()));
        GuardedSessionTableMigration {
            version: format!("m20261103_000003_guarded_session_{}", &digest[..24]),
            table,
        }
    }
}

impl GuardedSessionGcMigration {
    /// 为指定逻辑表登记相同的物理表身份摘要索引。
    pub fn for_table(table: impl Into<String>) -> GuardedSessionTableGcMigration {
        let table = table.into();
        let digest = format!("{:x}", Sha256::digest(table.as_bytes()));
        GuardedSessionTableGcMigration {
            version: format!("m20261103_000004_guarded_session_gc_{}", &digest[..24]),
            table,
        }
    }
}

impl MigrationName for GuardedSessionMigration {
    fn name(&self) -> &'static str {
        "m20261103_000003_openrust_guarded_session_expiry"
    }
}
impl MigrationName for GuardedSessionGcMigration {
    fn name(&self) -> &'static str {
        "m20261103_000004_openrust_guarded_session_gc_index"
    }
}
impl MigrationName for GuardedSessionTableMigration {
    fn name(&self) -> &str {
        &self.version
    }
}
impl MigrationName for GuardedSessionTableGcMigration {
    fn name(&self) -> &str {
        &self.version
    }
}

fn nullable_expiry(backend: DbBackend) -> Result<ColumnDef, DbErr> {
    // SeaQuery 现有 nullable 字段由 null() 替换 not_null()，不会累积冲突约束。
    Ok(dialect::timestamp_utc(Columns::ExpiresAt, backend)?
        .null()
        .take())
}

async fn add_expiry(manager: &SchemaManager<'_>, logical: &str) -> Result<(), DbErr> {
    let table = table_name(logical)?;
    for required in ["id", "payload", "last_activity"] {
        if !manager.has_column(&table, required).await? {
            return Err(DbErr::Migration(
                "增强 Session 迁移需要已建的原生 Session 表".into(),
            ));
        }
    }
    manager
        .alter_table(
            Table::alter()
                .table(Alias::new(table))
                .add_column(nullable_expiry(manager.get_database_backend())?)
                .to_owned(),
        )
        .await
}

async fn drop_expiry(manager: &SchemaManager<'_>, logical: &str) -> Result<(), DbErr> {
    dialect::drop_column(
        manager,
        Alias::new(table_name(logical)?),
        Columns::ExpiresAt,
    )
    .await
}

fn expiry_index(logical: &str) -> Result<String, DbErr> {
    let physical = table_name(logical)?;
    let digest = format!("{:x}", Sha256::digest(physical.as_bytes()));
    // 沿用旧增强 GC 合同，物理表名变化须得到不同索引身份；不改成活动索引规则。
    Ok(format!("ix_sessions_expiry_{}", &digest[..40]))
}

async fn add_index(manager: &SchemaManager<'_>, logical: &str) -> Result<(), DbErr> {
    let statement = Index::create()
        .name(expiry_index(logical)?)
        .table(Alias::new(table_name(logical)?))
        .col(Columns::ExpiresAt)
        .to_owned();
    if manager.get_database_backend() == DbBackend::Sqlite {
        return execute_sqlite_index(manager, statement.to_string(SqliteQueryBuilder)).await;
    }
    manager.create_index(statement).await
}

async fn drop_index(manager: &SchemaManager<'_>, logical: &str) -> Result<(), DbErr> {
    let statement = Index::drop()
        .name(expiry_index(logical)?)
        .table(Alias::new(table_name(logical)?))
        .to_owned();
    if manager.get_database_backend() == DbBackend::Sqlite {
        return execute_sqlite_index(manager, statement.to_string(SqliteQueryBuilder)).await;
    }
    manager.drop_index(statement).await
}

#[async_trait::async_trait]
impl MigrationTrait for GuardedSessionMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_expiry(manager, "sessions").await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_expiry(manager, "sessions").await
    }
}
#[async_trait::async_trait]
impl MigrationTrait for GuardedSessionTableMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_expiry(manager, &self.table).await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_expiry(manager, &self.table).await
    }
}
#[async_trait::async_trait]
impl MigrationTrait for GuardedSessionGcMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_index(manager, "sessions").await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_index(manager, "sessions").await
    }
}
#[async_trait::async_trait]
impl MigrationTrait for GuardedSessionTableGcMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_index(manager, &self.table).await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_index(manager, &self.table).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{
        DatabaseTableNames,
        sea_orm::sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder},
    };

    #[test]
    fn nullable_expiry_overrides_the_dialect_not_null_constraint() -> Result<(), DbErr> {
        for backend in [DbBackend::Postgres, DbBackend::MySql, DbBackend::Sqlite] {
            let statement = Table::alter()
                .table(Alias::new("sessions"))
                .add_column(nullable_expiry(backend)?)
                .to_owned();
            let sql = match backend {
                DbBackend::Postgres => statement.to_string(PostgresQueryBuilder),
                DbBackend::MySql => statement.to_string(MysqlQueryBuilder),
                DbBackend::Sqlite => statement.to_string(SqliteQueryBuilder),
                _ => return Err(DbErr::Custom("unexpected test backend".into())),
            };
            assert!(!sql.to_ascii_uppercase().contains("NOT NULL"));
            if backend == DbBackend::MySql {
                assert!(sql.contains("DATETIME(6)"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn expiry_index_preserves_physical_sha_contract_and_migration_names_are_distinct()
    -> crate::Result<()> {
        let names = DatabaseTableNames::new("app_", std::collections::BTreeMap::default())?;
        let actual = names
            .scope(async { expiry_index("sessions") })
            .await
            .map_err(crate::Error::internal)?;
        let hash = format!("{:x}", Sha256::digest(b"app_sessions"));
        assert_eq!(actual, format!("ix_sessions_expiry_{}", &hash[..40]));
        assert_ne!(
            GuardedSessionMigration.name(),
            GuardedSessionGcMigration.name()
        );
        assert_ne!(
            GuardedSessionMigration::for_table("sessions").name(),
            GuardedSessionMigration::for_table("custom").name()
        );
        Ok(())
    }
}
