#![doc = include_str!("../README.md")]

mod database_loader;

pub use database_loader::DatabaseLoader;

use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

/// 向应用提供可显式发布的语言词条迁移。
pub struct LocalizationDatabaseServiceProvider;

impl ServiceProvider for LocalizationDatabaseServiceProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes_migrations(
                app,
                &[(
                    "database/migrations/2026_11_25_000000_create_language_lines_table.rs",
                    include_bytes!("../stubs/create_language_lines_table.stub"),
                )],
                "localization-database-migrations",
            )
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(LocalizationDatabaseServiceProvider)]
}
