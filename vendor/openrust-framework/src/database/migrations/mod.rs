//! Migration discovery, execution, and history.

mod database_creation;
mod database_migration_repository;
mod migration;
mod migration_creator;
mod migration_repository_interface;
mod migration_result;
mod migrator;
mod schema_state;

pub(crate) use migrator::WipeOptions;
pub(super) use schema_state::SchemaCredentials;

pub use database_migration_repository::DatabaseMigrationRepository;
pub use migration::{Migration, MigrationContext, MigrationEntry, MigrationRegistry};

/// 为保留文件名的包迁移生成编译条目与可发布源码。
#[doc(hidden)]
#[macro_export]
macro_rules! compiled_migration_assets {
    ($package:literal; $($file:literal => $module:ident),+ $(,)?) => {
        const MIGRATION_ENTRIES: &[$crate::database::migrations::MigrationEntry] = &[
            $( $crate::database::migrations::MigrationEntry::new(
                concat!("vendor/openrust-", $package, "/database/migrations/", $file),
                &$module::MIGRATION,
            ), )+
        ];
        const MIGRATION_SOURCES: &[(&str, &[u8])] = &[
            $( (
                concat!("database/migrations/", $file),
                concat!(
                    include_str!(concat!("../database/migrations/", $file)),
                    "\npub(crate) use self::MIGRATION as Definition;\n",
                ).as_bytes(),
            ), )+
        ];
    };
}
pub use migration_creator::MigrationCreator;
pub use migration_repository_interface::{MigrationRecord, MigrationRepositoryInterface};
pub use migration_result::MigrationResult;
pub use migrator::{MigrationOutcome, MigrationStatus, Migrator, RollbackOptions, RunOptions};
