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
            let owner = Arc::downgrade(&app.container_handle());
            container.singletonNamed("hash", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                HashManager::for_container(config, owner.clone())
                    .map(Arc::new)
                    .map_err(|source| BindingResolutionError::FactoryFailed {
                        service: "hash",
                        source: Box::new(source),
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
