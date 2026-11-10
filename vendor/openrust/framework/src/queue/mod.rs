//! Queue drivers, workers, and dispatch data.

pub mod connectors;
pub mod console;
mod database_queue;
pub mod failed;
pub mod jobs;
mod queue_base;
mod queue_manager;
mod queue_service_provider;
mod redis_queue;
mod sync_queue;
mod worker;
mod worker_options;

use std::{
    error::Error,
    future::Future,
    pin::Pin,
    time::{Duration, SystemTime},
};

pub use database_queue::DatabaseQueue;
pub use queue_base::Queue;
pub use queue_manager::QueueManager;
pub use queue_service_provider::QueueServiceProvider;
pub use redis_queue::RedisQueue;
pub use sync_queue::SyncQueue;
pub use worker::Worker;
pub use worker_options::WorkerOptions;

/// An asynchronous queue operation.
pub type QueueFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a queue operation.
pub type QueueResult<T> = Result<T, QueueError>;

/// A queue resolution, payload, storage, or job failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum QueueError {
    UnknownConnection {
        name: String,
    },
    InvalidPayload,
    LostReservation,
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
    Execution {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// A relative delay or absolute time before a job becomes available.
pub enum Delay {
    For(Duration),
    Until(SystemTime),
}

/// The registered job name and serialized dispatch data.
pub struct Payload {
    _private: (),
}

/// The service scope and metadata for a single job attempt.
pub struct JobContext {
    _private: (),
}

/// Retry, backoff, and timeout settings from a job payload.
pub struct JobOptions {
    _private: (),
}
