//! 为显式选用统一因子授权的凭据保存受控上下文。

use crate::database::{
    DatabaseTableNames, dialect,
    sea_orm::{ConnectionTrait, DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

/// 为 refresh 与个人访问凭据增加可空因子上下文；不回填旧凭据。
#[derive(Clone, Copy, Debug, Default)]
pub struct CredentialFactorMigration;

impl CredentialFactorMigration {
    /// 创建显式凭据上下文迁移。
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
impl MigrationName for CredentialFactorMigration {
    fn name(&self) -> &'static str {
        "m20261105_000012_openrust_credential_factor"
    }
}
fn table(name: &str, backend: DbBackend) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("凭据因子迁移需要标准表名作用域".into()))?;
    names
        .quote(name, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(name)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}
#[crate::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CredentialFactorMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for name in ["auth_refresh_tokens", "personal_access_tokens"] {
            manager
                .alter_table(
                    Table::alter()
                        .table(table(name, manager.get_database_backend())?)
                        .add_column(ColumnDef::new(Alias::new("factor_context")).text().null())
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 先检查两个表，避免第二表有资产时已丢失第一表的上下文列。
        for name in ["auth_refresh_tokens", "personal_access_tokens"] {
            let query = Query::select()
                .expr(Expr::cust("1"))
                .from(table(name, manager.get_database_backend())?)
                .and_where(Expr::col(Alias::new("factor_context")).is_not_null())
                .limit(1)
                .to_owned();
            if manager
                .get_connection()
                .query_one_raw(manager.get_database_backend().build(&query))
                .await?
                .is_some()
            {
                return Err(DbErr::Migration(
                    "存在凭据因子上下文，不能回退丢失授权资产".into(),
                ));
            }
        }
        for name in ["personal_access_tokens", "auth_refresh_tokens"] {
            dialect::drop_column(
                manager,
                table(name, manager.get_database_backend())?,
                Alias::new("factor_context"),
            )
            .await?;
        }
        Ok(())
    }
}
