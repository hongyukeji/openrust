//! 应用数据库升级；框架迁移与应用迁移只登记一次。

#[cfg(feature = "auth-fortify")]
#[path = "migrations/accounts.rs"]
mod accounts;

use openrust::database::sea_orm_migration::prelude::*;

pub(crate) struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        #[allow(unused_mut, reason = "应用迁移按显式feature贡献")]
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        #[cfg(feature = "auth-fortify")]
        migrations.push(Box::new(accounts::AccountIdentifiers));
        #[cfg(feature = "auth-fortify")]
        migrations.push(Box::new(
            openrust::auth::fortify::FactorAuthorityMigration::new(),
        ));
        #[cfg(feature = "auth-fortify")]
        migrations.push(Box::new(
            openrust::auth::fortify::CredentialFactorMigration::new(),
        ));
        #[cfg(feature = "auth-fortify")]
        migrations.push(Box::new(
            openrust::auth::fortify::CredentialChallengeMigration::new(),
        ));
        migrations
    }
}

#[cfg(all(test, feature = "auth-fortify", feature = "db-sqlite"))]
mod tests;
