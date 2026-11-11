//! Event dispatch, listener registration, and queue adapters.

mod call_queued_listener;
mod dispatcher;
mod event_service_provider;
mod listener_registry;

use std::{any::Any, error::Error, future::Future, pin::Pin, time::Duration};

pub use call_queued_listener::CallQueuedListener;
pub use dispatcher::Dispatcher;
pub use event_service_provider::EventServiceProvider;
pub use listener_registry::ListenerRegistry;

/// An asynchronous event operation.
pub type EventFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of an event operation.
pub type EventResult<T> = Result<T, EventError>;

/// A registration, delivery, or listener failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum EventError {
    InvalidRegistration,
    QueueUnavailable,
    Unavailable(crate::contracts::container::BindingResolutionError),
    Transaction(crate::database::DatabaseError),
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

/// An explicitly named event with a typed payload.
pub struct NamedEvent<T> {
    name: String,
    payload: T,
}

impl<T> NamedEvent<T> {
    pub fn new(name: impl Into<String>, payload: T) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }

    pub fn payload(&self) -> &T {
        &self.payload
    }
}

impl<T: Send + Sync + 'static> crate::contracts::events::Event for NamedEvent<T> {
    fn name(&self) -> &str {
        &self.name
    }
}

/// The transaction scope for an event dispatch.
#[derive(Default)]
pub struct EventContext<'tx> {
    transaction: Option<&'tx mut crate::database::Transaction>,
}

impl<'tx> EventContext<'tx> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn in_transaction(transaction: &'tx mut crate::database::Transaction) -> Self {
        Self {
            transaction: Some(transaction),
        }
    }

    pub(super) fn active_transaction(&mut self) -> Option<&mut crate::database::Transaction> {
        self.transaction()
            .filter(|transaction| transaction.transactionLevel() != 0)
    }

    pub fn transaction(&mut self) -> Option<&mut crate::database::Transaction> {
        self.transaction.as_deref_mut()
    }
}

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRegistration => "invalid event registration",
            Self::InvalidPayload => "event payload has an unexpected type",
            Self::QueueUnavailable => "queued event listeners are unavailable",
            Self::Unavailable(_) => "event dispatcher is unavailable",
            Self::Transaction(_) => "event transaction failed",
            Self::Listener { .. } => "event listener failed",
            Self::Queue(_) => "event queue operation failed",
        })
    }
}

impl Error for EventError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(source) => Some(source),
            Self::Transaction(source) => Some(source),
            Self::Queue(source) => Some(source),
            Self::Listener { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<EventError> for crate::http::HttpError {
    fn from(source: EventError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}
