//! 同一账户表的可选因子authority元数据；不复制身份。

use crate::db::{
    DatabaseTableNames, dialect,
    sea_orm::{ConnectionTrait, DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};
use sha2::{Digest, Sha256};

const VERSION: &str = "m20261105_000011_openrust_factor_authority";
const SOURCE: &str = "two_factor_authority_source";
const GENERATION: &str = "two_factor_authority_generation";
const CONSUMED: &str = "two_factor_consumed_codes";
const MANAGEMENT_HASH: &str = "two_factor_management_session_hash";
const MANAGEMENT_GENERATION: &str = "two_factor_management_generation";
const MANAGEMENT_AT: &str = "two_factor_management_authorized_at";

/// 为显式选用数据库authority的同一模型增加持久来源和代际。
#[derive(Clone, Debug)]
pub struct FactorAuthorityMigration {
    table: String,
    version: String,
}
impl FactorAuthorityMigration {
    /// 默认沿用逻辑users表。
    #[must_use]
    pub fn new() -> Self {
        Self {
            table: "users".into(),
            version: VERSION.into(),
        }
    }
    /// 自定义账户表使用稳定独立迁移名，仍须先存在。
    pub fn with_table(table: impl Into<String>) -> crate::Result<Self> {
        let table = table.into();
        DatabaseTableNames::default().resolve(&table)?;
        let version = if table == "users" {
            VERSION.into()
        } else {
            format!("{VERSION}_{:x}", Sha256::digest(table.as_bytes()))
        };
        Ok(Self { table, version })
    }
    fn table(&self, backend: DbBackend) -> Result<Alias, DbErr> {
        let names = DatabaseTableNames::current()
            .ok_or_else(|| DbErr::Migration("因子authority迁移需要标准表名作用域".into()))?;
        names
            .quote(&self.table, backend)
            .map_err(|error| DbErr::Migration(error.to_string()))?;
        names
            .resolve(&self.table)
            .map(Alias::new)
            .map_err(|error| DbErr::Migration(error.to_string()))
    }
}
impl Default for FactorAuthorityMigration {
    fn default() -> Self {
        Self::new()
    }
}
impl MigrationName for FactorAuthorityMigration {
    fn name(&self) -> &str {
        &self.version
    }
}
#[crate::db::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for FactorAuthorityMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = self.table(manager.get_database_backend())?;
        for column in [
            ColumnDef::new(Alias::new(SOURCE)).text().null().take(),
            ColumnDef::new(Alias::new(GENERATION)).text().null().take(),
            ColumnDef::new(Alias::new(CONSUMED)).text().null().take(),
            ColumnDef::new(Alias::new(MANAGEMENT_HASH))
                .text()
                .null()
                .take(),
            ColumnDef::new(Alias::new(MANAGEMENT_GENERATION))
                .text()
                .null()
                .take(),
            dialect::timestamp_utc(Alias::new(MANAGEMENT_AT), manager.get_database_backend())?
                .null()
                .take(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(table.clone())
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = self.table(manager.get_database_backend())?;
        let query = Query::select()
            .expr(Expr::cust("1"))
            .from(table.clone())
            .cond_where(
                Condition::any()
                    .add(Expr::col(Alias::new(SOURCE)).is_not_null())
                    .add(Expr::col(Alias::new(GENERATION)).is_not_null())
                    .add(Expr::col(Alias::new(CONSUMED)).is_not_null())
                    .add(Expr::col(Alias::new(MANAGEMENT_HASH)).is_not_null())
                    .add(Expr::col(Alias::new(MANAGEMENT_GENERATION)).is_not_null())
                    .add(Expr::col(Alias::new(MANAGEMENT_AT)).is_not_null()),
            )
            .limit(1)
            .to_owned();
        if manager
            .get_connection()
            .query_one_raw(manager.get_database_backend().build(&query))
            .await?
            .is_some()
        {
            return Err(DbErr::Migration(
                "因子authority已有有效元数据，不能回退丢失来源或代际".into(),
            ));
        }
        for column in [
            MANAGEMENT_AT,
            MANAGEMENT_GENERATION,
            MANAGEMENT_HASH,
            CONSUMED,
            GENERATION,
            SOURCE,
        ] {
            dialect::drop_column(manager, table.clone(), Alias::new(column)).await?;
        }
        Ok(())
    }
}
