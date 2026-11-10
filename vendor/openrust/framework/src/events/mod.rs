//! Event dispatch, listener registration, and queue adapters.

mod call_queued_listener;
mod dispatcher;
mod event_service_provider;

use std::{any::Any, error::Error, future::Future, pin::Pin, time::Duration};

pub use call_queued_listener::CallQueuedListener;
pub use dispatcher::Dispatcher;
pub use event_service_provider::EventServiceProvider;

/// An asynchronous event operation.
pub type EventFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of an event operation.
pub type EventResult<T> = Result<T, EventError>;

/// A registration, delivery, or listener failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum EventError {
    InvalidRegistration,
    InvalidPayload,
    Listener {
        source: Box<dyn Error + Send + Sync>,
    },
    Queue(crate::queue::QueueError),
}

/// How a listener is invoked.
pub enum Delivery {
    Sync,
    AfterCommit,
    Queued(QueuedListenerOptions),
}

/// Options for dispatching a registered listener to a queue.
pub struct QueuedListenerOptions {
    pub listener_id: String,
    pub connection: Option<String>,
    pub queue: Option<String>,
    pub delay: Option<Duration>,
    pub after_commit: Option<bool>,
}

/// A response from a listener.
pub enum ListenerResponse {
    Stop,
    Value(Box<dyn Any + Send + Sync>),
}

/// The immediate responses or deferred status of a dispatch.
pub enum DispatchResult {
    Completed(Vec<Option<ListenerResponse>>),
    Deferred,
}

/// The service and transaction scope for an event dispatch.
pub struct EventContext {
    _private: (),
}

/// The ordered mapping between event names, patterns, and listeners.
pub struct ListenerRegistry {
    _private: (),
}
