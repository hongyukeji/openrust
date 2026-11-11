use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::container::Container as _,
    foundation::Application,
    support::{ProviderResult, ServiceProvider},
};

use super::{Dispatcher, EventFuture};

/// Registers the event dispatcher with the application container.
pub struct EventServiceProvider;

impl ServiceProvider for EventServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> EventFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            container.instanceNamed(
                "events",
                Arc::new(Dispatcher::for_application(Arc::downgrade(&container))),
            )?;
            container.alias("events", ServiceKey::typed::<Dispatcher>())?;
            Ok(())
        })
    }
}
