use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::events::{
        Dispatcher as _, Event as Payload, Listener, ShouldDispatchAfterCommit, Subscriber,
    },
    events::{
        Delivery, DispatchResult, Dispatcher, EventContext, EventFake, EventResult,
        ListenerResponse,
    },
};

use super::Facade;

/// Resolves the event dispatcher for the current application.
pub struct Event;

impl Facade for Event {
    type Service = Dispatcher;

    fn get_facade_accessor() -> ServiceKey {
        "events".into()
    }
}

impl Event {
    pub fn push(event: impl Payload) -> EventResult<()> {
        Self::dispatcher()?.push(Arc::new(event))
    }
    pub async fn flush(name: &str) -> EventResult<()> {
        Self::dispatcher()?
            .flush(name, &mut EventContext::new())
            .await
    }
    pub fn forget_pushed() -> EventResult<()> {
        Self::dispatcher()?.forget_pushed()
    }

    pub fn dispatcher() -> EventResult<Arc<Dispatcher>> {
        Self::get_facade_root().map_err(Into::into)
    }

    /// Records the selected events in the current application.
    pub fn fake(events: &[&str]) -> EventResult<Arc<EventFake>> {
        Ok(Self::dispatcher()?.fake(events))
    }

    /// Records all events except those allowed to reach their listeners.
    pub fn fake_except(events: &[&str]) -> EventResult<Arc<EventFake>> {
        Ok(Self::dispatcher()?.install_fake(EventFake::new(&[], events)))
    }

    /// Fakes events selected by their exact name or the given predicate.
    pub fn fake_when(
        events: &[&str],
        predicate: impl Fn(&dyn Payload) -> bool + Send + Sync + 'static,
    ) -> EventResult<Arc<EventFake>> {
        Ok(Self::dispatcher()?.install_fake(EventFake::matching(events, predicate)))
    }

    /// Fakes selected events while the callback runs in the current task.
    pub async fn fake_for<T>(
        callback: impl Future<Output = EventResult<T>>,
        events: &[&str],
    ) -> EventResult<T> {
        Self::dispatcher()?
            .with_fake(EventFake::new(events, &[]), callback)
            .await
    }

    /// Allows the named events and fakes all others during the callback.
    pub async fn fake_except_for<T>(
        callback: impl Future<Output = EventResult<T>>,
        events: &[&str],
    ) -> EventResult<T> {
        Self::dispatcher()?
            .with_fake(EventFake::new(&[], events), callback)
            .await
    }

    /// Fakes events selected by exact name or predicate while the callback runs.
    pub async fn fake_for_when<T>(
        callback: impl Future<Output = EventResult<T>>,
        events: &[&str],
        predicate: impl Fn(&dyn Payload) -> bool + Send + Sync + 'static,
    ) -> EventResult<T> {
        Self::dispatcher()?
            .with_fake(EventFake::matching(events, predicate), callback)
            .await
    }

    pub fn listen(name: &str, listener: Arc<dyn Listener>, delivery: Delivery) -> EventResult<()> {
        Self::dispatcher()?.listen(name, listener, delivery)
    }

    /// Registers a listener resolved by the current application's container on delivery.
    pub fn listen_class<L: Listener + 'static>(name: &str, delivery: Delivery) -> EventResult<()> {
        Self::dispatcher()?.listen_class::<L>(name, delivery)
    }

    pub fn subscribe(subscriber: &dyn Subscriber) -> EventResult<()> {
        Self::dispatcher()?.subscribe(subscriber)
    }

    pub fn has_listeners(name: &str) -> EventResult<bool> {
        Ok(Self::dispatcher()?.has_listeners(name))
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

    /// Delivers selected events after the callback succeeds.
    pub async fn defer<T>(
        names: Option<&[&str]>,
        callback: impl Future<Output = EventResult<T>>,
    ) -> EventResult<T> {
        Self::dispatcher()?.defer(names, callback).await
    }

    pub async fn dispatch_in_transaction(
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

    pub async fn dispatch_after_commit(
        event: impl ShouldDispatchAfterCommit,
        transaction: &mut crate::database::Transaction,
    ) -> EventResult<DispatchResult> {
        Self::dispatcher()?
            .dispatch_after_commit(event, &mut EventContext::in_transaction(transaction))
            .await
    }

    pub async fn until(event: impl Payload) -> EventResult<Option<ListenerResponse>> {
        Self::dispatcher()?
            .until(Arc::new(event), &mut EventContext::new())
            .await
    }
}
