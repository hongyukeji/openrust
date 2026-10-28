//! 综合示例使用的应用数据库迁移。

use openrust::database::sea_orm_migration::prelude::*;

pub(crate) struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(openrust::database::DatabaseRateLimitMigration),
            Box::new(openrust::audit::DatabaseAuditMigration),
            Box::new(openrust::audit::DatabaseAuditTenantMigration),
        ];
        #[cfg(feature = "auth")]
        let migrations = {
            let mut migrations = migrations;
            migrations.extend([
                Box::new(openrust::auth::AuthMigration) as Box<dyn MigrationTrait>,
                Box::new(openrust::auth::AuthRefreshTokenMigration),
                Box::new(openrust::auth::AuthVerificationCodeMigration),
                Box::new(openrust::auth::AuthPolicyMigration),
            ]);
            migrations
        };
        #[cfg(feature = "signing")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(openrust::signing::SignedLinkRevocationMigration));
            migrations
        };
        #[cfg(feature = "queue")]
        let migrations = {
            let mut migrations = migrations;
            migrations.push(Box::new(openrust::queue::QueueMigration));
            migrations
        };
        migrations
    }
}
