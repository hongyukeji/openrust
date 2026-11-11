use std::sync::Arc;

use crate::{
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        hashing::Hasher,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::HashManager;

/// Registers the hash manager and hasher contract in the service container.
pub struct HashingServiceProvider;

impl ServiceProvider for HashingServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singletonNamed("hash", |container| {
                let config = container.get::<dyn Repository>("config")?;
                HashManager::new(config).map(Arc::new).map_err(|source| {
                    BindingResolutionError::FactoryFailed {
                        service: "hash",
                        source: Box::new(source),
                    }
                })
            })?;
            container.alias("hash", crate::container::ServiceKey::typed::<HashManager>())?;
            container.singleton::<dyn Hasher, _>(|container| {
                let manager = container.get::<HashManager>("hash")?;
                Ok(manager)
            })?;
            Ok(())
        })
    }
}
