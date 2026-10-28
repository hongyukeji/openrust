//! 应用数据库迁移。

use openrust::database::sea_orm_migration::prelude::*;

pub struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(openrust::auth::AuthMigration),
            Box::new(openrust::auth::AuthRefreshTokenMigration),
            Box::new(openrust::auth::AuthVerificationCodeMigration),
            Box::new(openrust::auth::AuthPolicyMigration),
            Box::new(openrust::database::DatabaseRateLimitMigration),
            Box::new(openrust::audit::DatabaseAuditMigration),
            Box::new(openrust::audit::DatabaseAuditTenantMigration),
            Box::new(openrust::signing::SignedLinkRevocationMigration),
            Box::new(openrust::queue::QueueMigration),
        ]
    }
}
