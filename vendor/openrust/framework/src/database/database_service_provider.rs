use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{ConnectionResolverInterface, DatabaseManager};

/// Registers database services with the application container.
pub struct DatabaseServiceProvider;

impl ServiceProvider for DatabaseServiceProvider {
    fn register<'a>(&'a mut self, application: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = application.base_path().to_path_buf();
            let owner = Arc::downgrade(&application.container_handle());
            let container = application.container();
            container.singletonNamed("db", move |container| {
                let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
                Ok(Arc::new(
                    DatabaseManager::new(config, root.clone()).with_container(owner.clone()),
                ))
            })?;
            container.alias(
                "db",
                crate::container::ServiceKey::typed::<DatabaseManager>(),
            )?;
            container.singleton::<dyn ConnectionResolverInterface, _>(|container| {
                let manager = container.get::<DatabaseManager>("db")?;
                Ok(manager)
            })?;
            container.singleton::<super::seeder::SeederRegistry, _>(|_| {
                Ok(Arc::new(super::seeder::SeederRegistry::default()))
            })?;
            Ok(())
        })
    }
}
