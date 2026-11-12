//! Queue drivers, workers, and dispatch data.

mod chain_edit;
mod chain_routing;
pub mod connectors;
pub mod console;
pub use chain_edit::ChainJob;
pub(crate) use chain_edit::ChainUpdate;
mod database_queue;
pub mod events;
mod facade;
pub mod failed;
pub(crate) mod isolated_job;
pub mod jobs;
pub mod middleware;
mod queue_base;
mod queue_manager;
mod queue_service_provider;
mod redis_queue;
mod sync_queue;
mod worker;
mod worker_attempt_marker;
mod worker_control;
mod worker_options;
mod worker_signals;
mod worker_stop_reason;

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
pub use worker_stop_reason::WorkerStopReason;

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
    UnknownChainCallback(String),
    ChainedBatchFailed(String),
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
            Self::UnknownChainCallback(name) => {
                write!(f, "chain callback {name:?} is not registered")
            }
            Self::ChainedBatchFailed(cause) => write!(f, "chained batch job failed: {cause}"),
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
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum Delay {
    For(Duration),
    Until(SystemTime),
}

impl From<Duration> for Delay {
    fn from(duration: Duration) -> Self {
        Self::For(duration)
    }
}

impl From<SystemTime> for Delay {
    fn from(time: SystemTime) -> Self {
        Self::Until(time)
    }
}

/// Retry, backoff, and timeout metadata preserved with a queued payload.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct JobOptions {
    pub max_tries: Option<u32>,
    /// Maximum number of unhandled job exceptions within the worker's counting window.
    #[serde(default)]
    pub max_exceptions: Option<u32>,
    /// Includes an unfinished worker attempt in the exception count after redelivery.
    #[serde(default)]
    pub count_crashes_as_exceptions: bool,
    pub timeout: Option<Duration>,
    pub backoff: Vec<Duration>,
    #[serde(default)]
    pub retry_until: Option<SystemTime>,
    #[serde(default)]
    pub fail_on_timeout: bool,
    /// Runs the reserved task in a process that Artisan can terminate at its deadline.
    #[serde(default)]
    pub hard_timeout: bool,
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
    pub(crate) batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chain: Option<JobChain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chained_batch: Option<Box<crate::bus::ChainedBatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dispatch_options: Option<Box<chain_routing::JobDispatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chain_route: Option<Box<chain_routing::ChainRoute>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chain_delivery: Option<Box<chain_routing::ChainDelivery>>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct JobChain {
    remaining: std::collections::VecDeque<Payload>,
    catches: Vec<String>,
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
            chain: None,
            chained_batch: None,
            dispatch_options: None,
            chain_route: None,
            chain_delivery: None,
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

    pub(crate) fn is_chain_member(&self) -> bool {
        self.is_unattached() && self.chained_batch.is_none()
    }

    pub(crate) fn is_unattached(&self) -> bool {
        self.chain.is_none()
            && self.batch_id.is_none()
            && self.chain_delivery.is_none()
            && self.chain_route.is_none()
    }

    pub(crate) fn with_chain(
        mut self,
        remaining: std::collections::VecDeque<Payload>,
        catches: Vec<String>,
    ) -> Self {
        if !remaining.is_empty() || !catches.is_empty() {
            self.chain = Some(JobChain { remaining, catches });
        }
        self
    }

    fn next_chain_job(&self) -> Option<Self> {
        let JobChain {
            mut remaining,
            catches,
        } = self.chain.clone()?;
        let mut next = remaining.pop_front()?;
        next.context.clone_from(&self.context);
        next.chain_route.clone_from(&self.chain_route);
        Some(next.with_chain(remaining, catches))
    }

    fn chain_catches(&self) -> &[String] {
        self.chain.as_ref().map_or(&[], |chain| &chain.catches)
    }

    pub(crate) fn encode(&self) -> QueueResult<String> {
        serde_json::to_string(self).map_err(|_| QueueError::InvalidPayload)
    }

    pub(crate) fn decode(raw: &str) -> QueueResult<Self> {
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
    fail_immediately: bool,
    chain_edits: chain_edit::ChainEdits,
    chain_update: Option<ChainUpdate>,
    running_handler: Option<&'tx worker::RunningHandler>,
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
