use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::events::{
        Dispatcher as _, Event as Payload, Listener, ShouldDispatchAfterCommit, Subscriber,
    },
    events::{
        Delivery, DispatchResult, Dispatcher, EventContext, EventError, EventResult,
        ListenerResponse,
    },
};

use super::Facade;

/// Resolves the event dispatcher for the current application.
pub struct Event;

impl Facade for Event {
    type Service = Dispatcher;

    fn getFacadeAccessor() -> ServiceKey {
        "events".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Event facade")]
impl Event {
    pub fn dispatcher() -> EventResult<Arc<Dispatcher>> {
        Self::getFacadeRoot().map_err(EventError::Unavailable)
    }

    pub fn listen(name: &str, listener: Arc<dyn Listener>, delivery: Delivery) -> EventResult<()> {
        Self::dispatcher()?.listen(name, listener, delivery)
    }

    pub fn subscribe(subscriber: &dyn Subscriber) -> EventResult<()> {
        Self::dispatcher()?.subscribe(subscriber)
    }

    pub fn hasListeners(name: &str) -> EventResult<bool> {
        Ok(Self::dispatcher()?.hasListeners(name))
    }

    pub fn forget(name: &str) -> EventResult<()> {
        Self::dispatcher()?.forget(name);
        Ok(())
    }

    pub async fn dispatch(event: impl Payload) -> EventResult<DispatchResult> {
        Self::dispatcher()?
            .dispatch(Arc::new(event), &mut EventContext::new())
            .await
    }

    pub async fn dispatchInTransaction(
        event: impl Payload,
        transaction: &mut crate::database::Transaction,
    ) -> EventResult<DispatchResult> {
        Self::dispatcher()?
            .dispatch(
                Arc::new(event),
                &mut EventContext::in_transaction(transaction),
            )
            .await
    }

    pub async fn dispatchAfterCommit(
        event: impl ShouldDispatchAfterCommit,
        transaction: &mut crate::database::Transaction,
    ) -> EventResult<DispatchResult> {
        Self::dispatcher()?
            .dispatchAfterCommit(event, &mut EventContext::in_transaction(transaction))
            .await
    }

    pub async fn until(event: impl Payload) -> EventResult<Option<ListenerResponse>> {
        Self::dispatcher()?
            .until(Arc::new(event), &mut EventContext::new())
            .await
    }
}
