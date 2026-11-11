use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    events::{Dispatcher, EventFuture},
    foundation::{Application, events::DiscoverEvents},
    support::{ProviderResult, ServiceProvider},
};

/// Registers the application's compiled listener declarations.
pub struct EventServiceProvider {
    discovery: DiscoverEvents,
}

impl EventServiceProvider {
    pub fn new(discovery: DiscoverEvents) -> Self {
        Self { discovery }
    }
}

impl ServiceProvider for EventServiceProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> EventFuture<'a, ProviderResult> {
        Box::pin(async move {
            let events = app.container().make::<Dispatcher>()?;
            events.register(|registry| self.discovery.register(registry))?;
            Ok(())
        })
    }
}
