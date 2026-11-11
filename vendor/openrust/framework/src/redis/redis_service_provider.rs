use std::sync::Arc;

use crate::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _, redis::Factory,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::RedisManager;

/// Registers the named Redis connection manager.
pub struct RedisServiceProvider;

impl ServiceProvider for RedisServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singletonNamed("redis", |container| {
                Ok(Arc::new(RedisManager::new(
                    container.get::<dyn Repository>("config")?,
                )))
            })?;
            container.alias(
                "redis",
                crate::container::ServiceKey::typed::<RedisManager>(),
            )?;
            container.singleton::<dyn Factory, _>(|container| {
                let manager = container.get::<RedisManager>("redis")?;
                Ok(manager)
            })?;
            Ok(())
        })
    }
}
