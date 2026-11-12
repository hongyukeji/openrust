use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{
        broadcasting::Factory, config::Repository, container::Container as _,
        foundation::Application as _,
    },
    foundation::Application,
    queue::QueueManager,
    support::{ProviderResult, ServiceProvider},
};

use super::{BroadcastFuture, BroadcastManager};

/// Registers the broadcast manager and its factory binding.
pub struct BroadcastServiceProvider;

impl ServiceProvider for BroadcastServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> BroadcastFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singletonNamed("broadcast", |container| {
                let config = container.get::<dyn Repository>("config")?;
                let log = container.make()?;
                Ok(Arc::new(BroadcastManager::new(config, log)))
            })?;
            container.alias("broadcast", ServiceKey::typed::<BroadcastManager>())?;
            container.singleton::<dyn Factory, _>(|container| {
                Ok(container.make::<BroadcastManager>()?)
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> BroadcastFuture<'a, ProviderResult> {
        Box::pin(async move {
            let queue = app.make::<QueueManager>().await?;
            let factory = app.make::<dyn Factory>().await?;
            queue.register(
                super::JOB,
                Arc::new(super::broadcast_event::DeliverBroadcast(factory)),
            )?;
            Ok(())
        })
    }
}
