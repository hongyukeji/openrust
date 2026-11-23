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

    fn getFacadeAccessor() -> ServiceKey {
        "events".into()
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Event {
    pub fn push(event: impl Payload) -> EventResult<()> {
        Self::dispatcher()?.push(Arc::new(event))
    }
    pub async fn flush(name: &str) -> EventResult<()> {
        Self::dispatcher()?
            .flush(name, &mut EventContext::new())
            .await
    }
    pub fn forgetPushed() -> EventResult<()> {
        Self::dispatcher()?.forgetPushed()
    }

    pub fn dispatcher() -> EventResult<Arc<Dispatcher>> {
        Self::getFacadeRoot().map_err(Into::into)
    }

    /// Records the selected events in the current application.
    pub fn fake(events: &[&str]) -> EventResult<Arc<EventFake>> {
        Ok(Self::dispatcher()?.fake(events))
    }

    /// Records all events except those allowed to reach their listeners.
    pub fn fakeExcept(events: &[&str]) -> EventResult<Arc<EventFake>> {
        Ok(Self::dispatcher()?.install_fake(EventFake::new(&[], events)))
    }

    /// Fakes events selected by their exact name or the given predicate.
    pub fn fakeWhen(
        events: &[&str],
        predicate: impl Fn(&dyn Payload) -> bool + Send + Sync + 'static,
    ) -> EventResult<Arc<EventFake>> {
        Ok(Self::dispatcher()?.install_fake(EventFake::matching(events, predicate)))
    }

    /// Fakes selected events while the callback runs in the current task.
    pub async fn fakeFor<T>(
        callback: impl Future<Output = EventResult<T>>,
        events: &[&str],
    ) -> EventResult<T> {
        Self::dispatcher()?
            .with_fake(EventFake::new(events, &[]), callback)
            .await
    }

    /// Allows the named events and fakes all others during the callback.
    pub async fn fakeExceptFor<T>(
        callback: impl Future<Output = EventResult<T>>,
        events: &[&str],
    ) -> EventResult<T> {
        Self::dispatcher()?
            .with_fake(EventFake::new(&[], events), callback)
            .await
    }

    /// Fakes events selected by exact name or predicate while the callback runs.
    pub async fn fakeForWhen<T>(
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
    pub fn listenClass<L: Listener + 'static>(name: &str, delivery: Delivery) -> EventResult<()> {
        Self::dispatcher()?.listenClass::<L>(name, delivery)
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

    /// Delivers selected events after the callback succeeds.
    pub async fn defer<T>(
        names: Option<&[&str]>,
        callback: impl Future<Output = EventResult<T>>,
    ) -> EventResult<T> {
        Self::dispatcher()?.defer(names, callback).await
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
