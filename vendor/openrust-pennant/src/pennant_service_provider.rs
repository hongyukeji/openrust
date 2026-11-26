use crate::{FeatureManager, feature_manager::FeatureRegistry};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

pub struct PennantServiceProvider;
impl ServiceProvider for PennantServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "pennant")?;
            }
            app.container()
                .singleton::<FeatureRegistry, _>(|_| Ok(Arc::new(FeatureRegistry::default())))?;
            app.container().scoped::<FeatureManager, _>(|container| {
                Ok(Arc::new(FeatureManager::new(
                    container.get::<dyn openrust::contracts::config::Repository>("config")?,
                    container.make::<openrust::database::DatabaseManager>()?,
                    container.make::<openrust::auth::AuthManager>()?,
                    container.make::<openrust::events::Dispatcher>()?,
                    container.make::<FeatureRegistry>()?,
                )))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[("config/pennant.rs", include_bytes!("../config/pennant.rs"))],
                "pennant-config",
            )?;
            self.commands(app, crate::commands::register);
            let config = app.config()?;
            if config
                .get("pennant.stores.database.table")
                .and_then(|value| value.as_str().map(str::to_owned))
                .as_deref()
                == Some("features")
                && config
                    .get("pennant.stores.database.connection")
                    .is_none_or(|value| value.is_null())
            {
                self.load_migrations_from(app, Self::migrations()).await?;
            }
            Ok(())
        })
    }
}
impl PennantServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        static ENTRIES: &[openrust::database::migrations::MigrationEntry] = &[
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust-pennant/database/migrations/2026_11_21_000000_create_features_table.rs",
                &crate::create_features_table::MIGRATION,
            ),
        ];
        ENTRIES
    }
}
