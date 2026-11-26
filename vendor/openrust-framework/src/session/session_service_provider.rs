use std::sync::Arc;

use crate::{
    cache::CacheManager,
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::SessionManager;

/// Registers the application session manager.
pub struct SessionServiceProvider;

impl ServiceProvider for SessionServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let base = app.base_path().to_path_buf();
            container.singleton_named("session", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                let database = container.get::<DatabaseManager>("db")?;
                let cache = container.get::<CacheManager>("cache")?;
                let paths = container.make::<crate::foundation::Paths>()?;
                SessionManager::new(config.get("session"), database, cache, base.clone())
                    .map(|manager| Arc::new(manager.with_paths(paths)))
                    .map_err(|source| BindingResolutionError::FactoryFailed {
                        service: "session",
                        source: Box::new(source),
                    })
            })?;
            container.alias(
                "session",
                crate::container::ServiceKey::typed::<SessionManager>(),
            )?;
            Ok(())
        })
    }
}
