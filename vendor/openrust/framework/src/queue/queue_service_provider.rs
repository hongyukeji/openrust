use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{config::Repository, container::Container as _},
    foundation::Application,
    support::{ProviderResult, ServiceProvider},
};

use super::{QueueFuture, QueueManager};

/// Registers the queue manager in the application container.
pub struct QueueServiceProvider;

impl ServiceProvider for QueueServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> QueueFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            let owner = Arc::downgrade(&container);
            container.singletonNamed("queue", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(QueueManager::new(config, owner.clone())))
            })?;
            container.alias("queue", ServiceKey::typed::<QueueManager>())?;
            Ok(())
        })
    }
}
