use std::sync::Arc;

use crate::{
    contracts::{
        cache::Factory, config::Repository as Config, container::Container as _,
        foundation::Application as _,
    },
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::CacheManager;

/// Registers the cache manager in the application container.
pub struct CacheServiceProvider;

impl ServiceProvider for CacheServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let base = app.base_path().to_path_buf();
            container.singletonNamed("cache", move |container| {
                let config = container.get::<dyn Config>("config")?;
                let database = container.get::<DatabaseManager>("db")?;
                Ok(Arc::new(CacheManager::new(config, database, base.clone())))
            })?;
            container.alias(
                "cache",
                crate::container::ServiceKey::typed::<CacheManager>(),
            )?;
            container.singleton::<dyn Factory, _>(|container| {
                let manager = container.get::<CacheManager>("cache")?;
                Ok(manager)
            })?;
            Ok(())
        })
    }
}
