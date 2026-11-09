//! 不建立数据库连接，验证应用实际迁移清单包含完整框架基础设施且没有重复。

use openrust::database::sea_orm_migration::{MigrationName, MigratorTrait};
use std::collections::HashSet;

#[test]
fn generated_application_extends_the_framework_catalog_once() {
    let names = openrust_migration::Migrator::migrations()
        .into_iter()
        .map(|migration| migration.name().to_owned())
        .collect::<Vec<_>>();
    let framework = openrust::database::FrameworkMigrator::migrations()
        .into_iter()
        .map(|migration| migration.name().to_owned())
        .collect::<Vec<_>>();
    assert!(names.starts_with(&framework));
    assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());
    assert!(names.contains(&openrust::queue::QueueMigration.name().to_owned()));
    assert!(names.contains(&openrust::session::SessionGcMigration.name().to_owned()));
    // {{auth_migration_assertions}}
    #[cfg(feature = "passport")]
    {
        let passport = openrust_passport::default_passport_migrations()
            .into_iter()
            .map(|migration| migration.name().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(passport.len(), 4);
        assert!(names[framework.len()..].starts_with(&passport));
    }
}
