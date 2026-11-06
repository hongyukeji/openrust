//! 显式直接登录增强的统一因子挑战；官方Session挑战继续由原Guard承载。

use crate::database::{
    DatabaseTableNames, dialect,
    sea_orm::{ConnectionTrait, DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

/// 为已验证第一因素的直接登录保存短时、一次性因子挑战。
#[derive(Clone, Copy, Debug, Default)]
pub struct CredentialChallengeMigration;

impl CredentialChallengeMigration {
    /// 创建显式直接登录挑战迁移，不接管旧挑战或旧凭据。
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
impl MigrationName for CredentialChallengeMigration {
    fn name(&self) -> &'static str {
        "m20261105_000013_openrust_credential_challenge"
    }
}
fn table(name: &str, backend: DbBackend) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("凭据挑战迁移需要标准表名作用域".into()))?;
    names
        .quote(name, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(name)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}
#[crate::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CredentialChallengeMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let challenges = table("auth_factor_challenges", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(challenges.clone())
                    .col(
                        ColumnDef::new(Alias::new("token_hash"))
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("user_id"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("source_hash"))
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("context")).text().not_null())
                    .col(ColumnDef::new(Alias::new("purpose")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("password_hash_digest"))
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("attempts"))
                            .big_integer()
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(Alias::new("expires_at"), backend)?)
                    .col(dialect::timestamp_utc(Alias::new("created_at"), backend)?)
                    .col(dialect::timestamp_utc(Alias::new("consumed_at"), backend)?.null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_factor_challenge_user")
                            .from(challenges.clone(), Alias::new("user_id"))
                            .to(table("users", backend)?, Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ix_factor_challenge_user")
                    .table(challenges)
                    .col(Alias::new("user_id"))
                    .to_owned(),
            )
            .await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = table("auth_factor_challenges", manager.get_database_backend())?;
        let query = Query::select()
            .expr(Expr::cust("1"))
            .from(table.clone())
            .limit(1)
            .to_owned();
        if manager
            .get_connection()
            .query_one_raw(manager.get_database_backend().build(&query))
            .await?
            .is_some()
        {
            return Err(DbErr::Migration(
                "存在直接登录挑战记录，不能回退丢失认证消费证据".into(),
            ));
        }
        manager
            .drop_table(Table::drop().table(table).to_owned())
            .await
    }
}
