//! Queue drivers, workers, and dispatch data.

mod beanstalkd_queue;
mod chain_edit;
mod chain_routing;
pub mod connectors;
pub mod console;
pub(crate) mod sqs_queue;
pub use chain_edit::ChainJob;
pub(crate) use chain_edit::ChainUpdate;
mod database_queue;
pub mod events;
mod facade;
pub mod failed;
mod failover_queue;
pub(crate) mod isolated_job;
mod job_popper;
mod lifecycle_queue;
pub use job_popper::{JobPopCallback, JobPopper};
pub mod jobs;
pub mod middleware;
mod queue_base;
mod queue_manager;
mod queue_service_provider;
mod redis_queue;
mod sync_queue;
mod unique_job;
mod worker;
mod worker_attempt_marker;
mod worker_control;
pub use unique_job::UniqueJob;
mod worker_hooks;
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
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum QueueError {
    UnknownConnection {
        name: String,
    },
    InvalidPayload,
    MissingModel(String),
    InvalidBatchConnection,
    Timeout,
    MaxAttempts,
    InvalidConfiguration,
    UnsupportedDriver(String),
    Unavailable(std::sync::Arc<crate::contracts::container::BindingResolutionError>),
    UnknownJob(String),
    UnknownBatchCallback(String),
    UnknownChainCallback(String),
    ChainedBatchFailed(String),
    LostReservation,
    Backend {
        source: std::sync::Arc<dyn Error + Send + Sync>,
    },
    Execution {
        source: std::sync::Arc<dyn Error + Send + Sync>,
    },
}

impl QueueError {
    /// 保存原始错误对象，不要求错误实现 Clone 或序列化。
    pub fn execution(source: impl Error + Send + Sync + 'static) -> Self {
        Self::Execution {
            source: std::sync::Arc::new(source),
        }
    }
    pub fn backend(source: impl Error + Send + Sync + 'static) -> Self {
        Self::Backend {
            source: std::sync::Arc::new(source),
        }
    }
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
            Self::InvalidBatchConnection => {
                f.write_str("queue connection does not support job batches")
            }
            Self::InvalidPayload => f.write_str("invalid queue payload"),
            Self::MissingModel(table) => write!(f, "queued model in {table:?} was not found"),
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
            Self::Unavailable(source) => Some(source.as_ref()),
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
    QueueError::backend(source)
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

/// 原生入队入口创建的任务类型。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    #[default]
    Job,
    Broadcast,
    Event,
    Mail,
    Notification,
}

impl JobType {
    fn is_job(&self) -> bool {
        *self == Self::Job
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Payload {
    version: u32,
    uuid: String,
    name: String,
    #[serde(default, skip_serializing_if = "JobType::is_job")]
    job_type: JobType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subject_type: Option<Box<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event_type: Option<Box<str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    data: serde_json::Value,
    options: JobOptions,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    delete_when_missing_models: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sqs_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sqs_deduplication: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unique_lock: Option<unique_job::UniqueJobLock>,
    #[serde(
        default,
        skip_serializing_if = "crate::log::context::DehydratedContext::is_empty"
    )]
    context: crate::log::context::DehydratedContext,
    #[serde(
        default,
        deserialize_with = "crate::foundation::observability::deserialize_trace",
        skip_serializing_if = "Option::is_none"
    )]
    trace_context: Option<crate::foundation::observability::TraceContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    redis_metadata_inputs:
        Option<std::sync::Arc<std::collections::BTreeMap<String, serde_json::Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    redis_metadata: Option<std::sync::Arc<std::collections::BTreeMap<String, serde_json::Value>>>,
    #[serde(skip)]
    redis_metadata_prepared: bool,
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
            job_type: JobType::Job,
            subject_type: None,
            event_type: None,
            display_name: None,
            data: serde_json::to_value(data).map_err(|_| QueueError::InvalidPayload)?,
            options: JobOptions::default(),
            delete_when_missing_models: false,
            sqs_group: None,
            sqs_deduplication: None,
            unique_lock: None,
            context: Default::default(),
            trace_context: None,
            delivery_id: None,
            retry_of: None,
            redis_metadata_inputs: None,
            redis_metadata: None,
            redis_metadata_prepared: false,
            batch_id: None,
            chain: None,
            chained_batch: None,
            dispatch_options: None,
            chain_route: None,
        })
    }

    /// Acknowledges this job if an explicitly restored model is missing.
    #[must_use]
    pub fn delete_when_missing_models(mut self) -> Self {
        self.delete_when_missing_models = true;
        self
    }

    pub fn with_options(mut self, options: JobOptions) -> Self {
        self.options = options;
        self
    }

    pub fn on_group(mut self, group: impl Into<String>) -> Self {
        self.sqs_group = Some(group.into());
        self
    }

    /// An empty ID selects the FIFO queue's content-based deduplication.
    pub fn with_deduplication_id(mut self, id: impl Into<String>) -> Self {
        self.sqs_deduplication = Some(id.into());
        self
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn batch_id(&self) -> Option<&str> {
        self.batch_id.as_deref()
    }

    pub fn uuid(&self) -> &str {
        &self.uuid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn job_type(&self) -> JobType {
        self.job_type
    }
    /// 返回投递的底层应用类型，例如排队邮件的 Mailable 类型。
    pub fn subject_type(&self) -> Option<&str> {
        self.subject_type.as_deref()
    }

    /// 返回排队监听器接收的事件类型。
    pub fn event_type(&self) -> Option<&str> {
        self.event_type.as_deref()
    }

    pub(crate) fn with_event_type(mut self, event_type: &str) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    pub(crate) fn with_job_subject(mut self, job_type: JobType, subject_type: &str) -> Self {
        self.job_type = job_type;
        self.subject_type = Some(subject_type.into());
        self
    }
    pub(crate) fn with_display_name(mut self, name: String) -> Self {
        self.display_name = Some(name);
        self
    }
    /// 返回任务展示名称。未设置时使用处理器注册名。
    pub fn display_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }
    pub fn data(&self) -> &serde_json::Value {
        &self.data
    }
    pub fn options(&self) -> &JobOptions {
        &self.options
    }

    /// 返回此次重新投递对应的原失败记录标识。
    pub fn retry_of(&self) -> Option<&str> {
        self.retry_of.as_deref()
    }

    /// 判断载荷是否由核心失败队列重新投递。
    pub fn is_retry_delivery(&self) -> bool {
        self.delivery_id.is_some()
    }

    /// 读取 Redis 入队时由应用扩展生成并随任务保存的元数据。
    pub fn redis_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.redis_metadata.as_ref()?.get(key)
    }

    /// 设置扩展在生成 Redis 投递元数据时读取的应用输入。
    pub fn with_redis_metadata_input(
        mut self,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        let inputs = self
            .redis_metadata_inputs
            .get_or_insert_with(Default::default);
        std::sync::Arc::make_mut(inputs).insert(key.into(), value);
        self
    }

    /// 读取应用为 Redis 投递元数据生成器提供的输入。
    pub fn redis_metadata_input(&self, key: &str) -> Option<&serde_json::Value> {
        self.redis_metadata_inputs.as_ref()?.get(key)
    }

    pub(crate) fn for_scheduled_dispatch(&self) -> QueueResult<Self> {
        Ok(Self {
            uuid: new_id()?,
            delivery_id: None,
            retry_of: None,
            redis_metadata: None,
            redis_metadata_prepared: false,
            context: Default::default(),
            ..self.clone()
        })
    }

    pub(super) fn renewed(mut self, retry_of: Option<&str>) -> QueueResult<Self> {
        self.unique_lock = None;
        self.delivery_id = Some(new_id()?);
        self.retry_of = retry_of.map(str::to_owned);
        Ok(self)
    }

    pub(crate) fn is_chain_member(&self) -> bool {
        self.is_unattached() && self.chained_batch.is_none()
    }

    pub(crate) fn is_unattached(&self) -> bool {
        self.chain.is_none() && self.batch_id.is_none() && self.chain_route.is_none()
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
    stop_retries: Option<bool>,
    chain_edits: chain_edit::ChainEdits,
    chain_update: Option<ChainUpdate>,
    running_handler: Option<&'tx worker::RunningHandler>,
}

impl JobContext<'_> {
    pub fn release(&mut self, delay: Duration) {
        self.release_after = Some(delay);
    }

    /// 获取处理器显式请求的重新投递延迟。
    pub fn release_delay(&self) -> Option<Duration> {
        self.release_after
    }

    /// 处理器返回错误时立即进入终态失败流程。
    pub fn fail_without_retry(&mut self) {
        self.fail_immediately = true;
    }

    /// 当前处理器是否要求本次错误直接进入终态失败。
    pub fn fail_without_retry_requested(&self) -> bool {
        self.fail_immediately
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

#[derive(Clone, Default)]
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
    Duplicate,
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

impl From<crate::contracts::container::BindingResolutionError> for QueueError {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::Unavailable(std::sync::Arc::new(source))
    }
}
