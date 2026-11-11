use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{
    DatabaseManager,
    migrations::{MigrationRegistry, Migrator},
};

/// Registers the migration registry and migrator with the application.
pub struct MigrationServiceProvider;

impl ServiceProvider for MigrationServiceProvider {
    fn register<'a>(&'a mut self, application: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = application.base_path().to_path_buf();
            let container = application.container();
            container.singleton::<MigrationRegistry, _>(|_| {
                Ok(Arc::new(MigrationRegistry::default()))
            })?;
            container.singletonNamed("migrator", move |container| {
                let manager = container.get::<DatabaseManager>("db")?;
                let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
                let registry =
                    container.get::<MigrationRegistry>(crate::container::ServiceKey::typed::<
                        MigrationRegistry,
                    >())?;
                let table = config
                    .get("database.migrations.table")
                    .or_else(|| {
                        config
                            .get("database.migrations")
                            .filter(serde_json::Value::is_string)
                    })
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "migrations".to_owned());
                Ok(Arc::new(Migrator::new(
                    manager,
                    registry,
                    table,
                    root.clone(),
                )))
            })?;
            container.alias(
                "migrator",
                crate::container::ServiceKey::typed::<Migrator>(),
            )?;
            Ok(())
        })
    }
}
