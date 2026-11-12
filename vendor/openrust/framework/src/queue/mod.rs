//! Queue drivers, workers, and dispatch data.

pub mod connectors;
pub mod console;
mod database_queue;
mod facade;
pub mod failed;
pub mod jobs;
pub mod middleware;
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
pub use facade::QueueFacade;
pub use queue_base::Queue;
pub use queue_manager::QueueManager;
pub use queue_service_provider::QueueServiceProvider;
pub use redis_queue::RedisQueue;
pub use sync_queue::SyncQueue;
pub use worker::{WorkResult, Worker};
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
    InvalidBatchConnection,
    Timeout,
    MaxAttempts,
    InvalidConfiguration,
    UnsupportedDriver(String),
    Unavailable(crate::contracts::container::BindingResolutionError),
    UnknownJob(String),
    UnknownBatchCallback(String),
    LostReservation,
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
    Execution {
        source: Box<dyn Error + Send + Sync>,
    },
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownConnection { name } => {
                write!(f, "queue connection {name:?} is not configured")
            }
            Self::UnsupportedDriver(name) => write!(f, "queue driver {name:?} is unavailable"),
            Self::UnknownBatchCallback(name) => {
                write!(f, "batch callback {name:?} is not registered")
            }
            Self::UnknownJob(name) => write!(f, "queued job {name:?} is not registered"),
            Self::InvalidConfiguration => f.write_str("invalid queue configuration"),
            Self::Timeout => f.write_str("queued job timed out"),
            Self::MaxAttempts => f.write_str("queued job exceeded its allowed attempts"),
            Self::InvalidBatchConnection => f.write_str(
                "job batches require Redis or a database queue on the batch repository connection",
            ),
            Self::InvalidPayload => f.write_str("invalid queue payload"),
            Self::LostReservation => f.write_str("queue job reservation has expired or changed"),
            Self::Unavailable(_) => f.write_str("application queue manager is unavailable"),
            Self::Backend { .. } => f.write_str("queue storage operation failed"),
            Self::Execution { .. } => f.write_str("queue job execution failed"),
        }
    }
}

impl Error for QueueError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(source) => Some(source),
            Self::Backend { source } | Self::Execution { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<QueueError> for crate::http::HttpError {
    fn from(source: QueueError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

pub(super) fn backend(source: impl Error + Send + Sync + 'static) -> QueueError {
    QueueError::Backend {
        source: Box::new(source),
    }
}

/// A relative delay or absolute time before a job becomes available.
#[derive(Clone, Copy)]
pub enum Delay {
    For(Duration),
    Until(SystemTime),
}

/// Retry, backoff, and timeout metadata preserved with a queued payload.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct JobOptions {
    pub max_tries: Option<u32>,
    pub timeout: Option<Duration>,
    pub backoff: Vec<Duration>,
    #[serde(default)]
    pub retry_until: Option<SystemTime>,
    #[serde(default)]
    pub fail_on_timeout: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Payload {
    version: u32,
    uuid: String,
    name: String,
    data: serde_json::Value,
    options: JobOptions,
    #[serde(
        default,
        skip_serializing_if = "crate::log::context::DehydratedContext::is_empty"
    )]
    context: crate::log::context::DehydratedContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    batch_id: Option<String>,
}

impl Payload {
    pub fn new(name: impl Into<String>, data: impl serde::Serialize) -> QueueResult<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(QueueError::InvalidPayload);
        }
        Ok(Self {
            version: 1,
            uuid: new_id()?,
            name,
            data: serde_json::to_value(data).map_err(|_| QueueError::InvalidPayload)?,
            options: JobOptions::default(),
            context: Default::default(),
            delivery_id: None,
            batch_id: None,
        })
    }

    pub fn with_options(mut self, options: JobOptions) -> Self {
        self.options = options;
        self
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    #[allow(non_snake_case, reason = "Matches Laravel batch identifier")]
    pub fn batchId(&self) -> Option<&str> {
        self.batch_id.as_deref()
    }

    pub fn uuid(&self) -> &str {
        &self.uuid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn data(&self) -> &serde_json::Value {
        &self.data
    }
    pub fn options(&self) -> &JobOptions {
        &self.options
    }

    pub(crate) fn for_scheduled_dispatch(&self) -> QueueResult<Self> {
        Ok(Self {
            uuid: new_id()?,
            delivery_id: None,
            context: Default::default(),
            ..self.clone()
        })
    }

    pub(super) fn renewed(mut self) -> QueueResult<Self> {
        self.delivery_id = Some(new_id()?);
        Ok(self)
    }

    pub(super) fn encode(&self) -> QueueResult<String> {
        serde_json::to_string(self).map_err(|_| QueueError::InvalidPayload)
    }

    pub(super) fn decode(raw: &str) -> QueueResult<Self> {
        let payload: Self = serde_json::from_str(raw).map_err(|_| QueueError::InvalidPayload)?;
        if payload.version != 1
            || payload.name.trim().is_empty()
            || uuid::Uuid::parse_str(&payload.uuid).is_err()
        {
            return Err(QueueError::InvalidPayload);
        }
        Ok(payload)
    }
}

/// The metadata available while a job handler executes.
#[derive(Default)]
pub struct JobContext<'tx> {
    connection: String,
    queue: String,
    attempts: u32,
    transaction: Option<&'tx mut crate::database::Transaction>,
    release_after: Option<Duration>,
}

impl JobContext<'_> {
    pub fn release(&mut self, delay: Duration) {
        self.release_after = Some(delay);
    }

    pub fn transaction(&mut self) -> Option<&mut crate::database::Transaction> {
        self.transaction.as_deref_mut()
    }

    pub fn connection(&self) -> &str {
        &self.connection
    }
    pub fn queue(&self) -> &str {
        &self.queue
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

#[derive(Default)]
pub struct DispatchOptions {
    pub connection: Option<String>,
    pub queue: Option<String>,
    pub delay: Option<Delay>,
    pub after_commit: Option<bool>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DispatchResult {
    Completed,
    Queued(String),
    Deferred,
}

pub(super) fn queue_name<'a>(selected: Option<&'a str>, default: &'a str) -> QueueResult<&'a str> {
    let queue = selected.unwrap_or(default);
    if queue.is_empty() || queue.len() > 255 || queue.chars().any(char::is_control) {
        Err(QueueError::InvalidConfiguration)
    } else {
        Ok(queue)
    }
}

pub(super) fn new_id() -> QueueResult<String> {
    let mut bytes = [0; 16];
    aws_lc_rs::rand::fill(&mut bytes).map_err(|_| QueueError::InvalidPayload)?;
    Ok(uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string())
}
