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
            container.scoped::<super::dispatcher::PushedEvents, _>(|_| Ok(Arc::default()))?;
            let dispatcher = Arc::new(Dispatcher::for_application(Arc::downgrade(&container)));
            app.register_deferred_load_events(&dispatcher)?;
            container.instance_named("events", dispatcher)?;
            container.alias("events", ServiceKey::typed::<Dispatcher>())?;
            container.bind::<dyn crate::contracts::events::Dispatcher, _>(|container| {
                Ok(
                    container.make::<Dispatcher>()?
                        as Arc<dyn crate::contracts::events::Dispatcher>,
                )
            })?;
            Ok(())
        })
    }
}
