//! Database schema for shared signed-link revocation.

use crate::db::{
    DatabaseTableNames,
    sea_orm::{DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

#[derive(DeriveIden)]
enum Revocations {
    TokenHash,
    ExpiresAt,
}

fn table(backend: DbBackend) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "OpenRust migration requires MigrationPlugin or TestDatabase table-name scope".into(),
        )
    })?;
    names
        .quote("signed_link_revocations", backend)
        .map_err(|e| DbErr::Migration(e.to_string()))?;
    names
        .resolve("signed_link_revocations")
        .map(Alias::new)
        .map_err(|e| DbErr::Migration(e.to_string()))
}

/// Creates the shared signed-link revocation table.
pub struct SignedLinkRevocationMigration;

impl MigrationName for SignedLinkRevocationMigration {
    fn name(&self) -> &'static str {
        "m20261026_000001_openrust_signed_link_revocations"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for SignedLinkRevocationMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        manager
            .create_table(
                Table::create()
                    .table(table(backend)?)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Revocations::TokenHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Revocations::ExpiresAt)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(table(manager.get_database_backend())?)
                    .to_owned(),
            )
            .await
    }
}
