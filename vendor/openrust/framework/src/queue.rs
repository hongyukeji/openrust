//! 基于数据库的持久化任务派发。
//!
//! 应用必须在 migration crate 中注册 [`QueueMigration`]。外围数据库事务提交后，派发的任务即被持久化；
//! handler 和 worker 通过队列 worker API 提供。
//!
//! Worker 收到 Ctrl-C 或 SIGTERM 后停止领取新任务，并通知运行中的 handler 合作取消。
//! handler 在配置期限内完成时正常确认；超时会取消 handler 并按当前租约 token 尝试重试。
//! 若数据库不可用，worker 返回错误并保留租约，任务会在租约过期后恢复。
//!
//! ```no_run
//! use openrust::{database::Db, queue::Queue};
//! # async fn example(db: Db) -> openrust::Result<()> {
//! let queue = Queue::new(db);
//! queue.dispatch("emails", "welcome.v1", &serde_json::json!({"user_id": 42})).await?;
//! # Ok(())
//! # }
//! ```

use crate::core::{
    Cancellation, Error, ExecutionContext, ExecutionMetadata, Result, with_execution_context,
};
use crate::database::{DatabaseTransaction, Db, DbBackend};
use crate::support::BoxFuture;
use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "queue-redis")]
#[path = "queue/redis.rs"]
mod redis;
#[cfg(feature = "queue-redis")]
pub use redis::RedisQueueDriver;

const MAX_QUEUE_NAME: usize = 64;
const MAX_JOB_NAME: usize = 128;
const MAX_PAYLOAD_BYTES: usize = 1_048_576;
const MAX_GROUP_TASKS: usize = 128;
const MAX_GROUP_CONCURRENCY: usize = 64;
const DEFAULT_MAX_ATTEMPTS: u32 = 5;
const MAX_DELAY_SECS: u64 = 2_592_000;
const HEX: &[u8; 16] = b"0123456789abcdef";
const CHAIN_JOB_NAME: &str = "openrust.internal.chain.v1";
const BATCH_JOB_NAME: &str = "openrust.internal.batch.v1";
const EXECUTION_ENVELOPE_KEY: &str = "__openrust_execution";
const EXECUTION_ENVELOPE_VERSION: &str = "v1";
type ProgressReporter = Arc<dyn Fn(Value) -> BoxFuture<'static, Result<()>> + Send + Sync>;

struct AttemptScope {
    scope: Option<Arc<crate::container::Scope>>,
    timeout: Duration,
}
impl AttemptScope {
    fn create(context: &mut JobContext) -> Result<Option<Self>> {
        if context.attempt_scope.is_some() {
            context.scope()?;
            return Ok(None);
        }
        let Some(state) = &context.app_state else {
            return Ok(None);
        };
        let scope = Arc::new(state.container().scope()?);
        context.attempt_scope = Some(Arc::clone(&scope));
        Ok(Some(Self {
            scope: Some(scope),
            timeout: Duration::from_secs(state.config().server.shutdown_timeout_secs),
        }))
    }
    async fn shutdown(self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        self.shutdown_at(deadline).await
    }

    async fn shutdown_at(mut self, deadline: tokio::time::Instant) -> Result<()> {
        if let Some(scope) = &self.scope {
            scope.shutdown(deadline).await?;
        }
        self.scope = None;
        Ok(())
    }
}
impl Drop for AttemptScope {
    fn drop(&mut self) {
        if let Some(scope) = &self.scope {
            scope.close();
        }
    }
}

#[cfg(feature = "metrics")]
struct WorkerJobMetricGuard;

#[cfg(feature = "metrics")]
impl WorkerJobMetricGuard {
    fn new() -> Self {
        metrics::gauge!("openrust_worker_jobs_in_flight").increment(1.0);
        Self
    }
}

#[cfg(feature = "metrics")]
impl Drop for WorkerJobMetricGuard {
    fn drop(&mut self) {
        metrics::gauge!("openrust_worker_jobs_in_flight").decrement(1.0);
    }
}

/// 通过队列贡献目录显式注册的类型化任务处理器。
pub trait Job: DeserializeOwned + Send + Sync + 'static {
    /// 持久化使用的稳定任务名称。重命名 Rust 类型时应保留此名称。
    const NAME: &'static str;

    /// 运行任务。数据库队列提供至少一次执行语义，因此 handler 必须具有幂等性。
    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>>;
}

/// 任务尝试的运行时元数据与协作取消信号。
#[derive(Clone)]
pub struct JobContext {
    id: String,
    attempt: u64,
    max_attempts: u64,
    cancelled: Cancellation,
    app_state: Option<crate::foundation::ApplicationState>,
    attempt_scope: Option<Arc<crate::container::Scope>>,
    progress_reporter: Option<ProgressReporter>,
    metadata: ExecutionMetadata,
    deadline: Option<tokio::time::Instant>,
}

/// 自定义 driver 持有的一次任务尝试及其资源作用域。
///
/// 超时或租约丢失时，先丢弃 handler future，再在确认或重试前等待
/// [`JobAttempt::shutdown`]。Drop 仅触发取消兜底，不等待资源回收。
pub struct JobAttempt {
    context: JobContext,
    scope: AttemptScope,
}

impl JobAttempt {
    /// 获取可传给 [`JobRegistry::execute`] 的本次尝试上下文。
    pub fn context(&self) -> &JobContext {
        &self.context
    }

    /// 等待本次作用域及受管任务关闭，deadline 为单调时钟时间。
    pub async fn shutdown(self, deadline: tokio::time::Instant) -> Result<()> {
        self.scope.shutdown_at(deadline).await
    }
}

impl JobContext {
    /// 为自定义 driver 创建独立的尝试作用域。
    ///
    /// 必须先设置应用状态；每次重试均须创建新的上下文和 guard。
    pub fn begin_attempt(mut self) -> Result<JobAttempt> {
        if self.attempt_scope.is_some() {
            return Err(Error::custom(
                "queue_attempt_already_started",
                "job attempt already started",
            ));
        }
        let scope = AttemptScope::create(&mut self)?.ok_or_else(|| {
            Error::custom(
                "queue_application_state_unavailable",
                "this queue execution has no application service context",
            )
        })?;
        Ok(JobAttempt {
            context: self,
            scope,
        })
    }

    /// 为自定义 driver 创建一次任务尝试的上下文。
    ///
    /// `cancellation` 应由 driver 持有，并在任务需要停止时调用 [`Cancellation::cancel`]。
    /// 默认不支持进度写入；使用 [`JobContext::with_progress_reporter`] 显式提供后端实现。
    pub fn new(
        id: impl Into<String>,
        attempt: u64,
        max_attempts: u64,
        cancellation: Cancellation,
    ) -> Result<Self> {
        let id = id.into();
        if id.is_empty() || attempt == 0 || max_attempts == 0 || attempt > max_attempts {
            return Err(Error::custom(
                "invalid_queue_job_context",
                "job context requires a non-empty id and a valid attempt range",
            ));
        }
        Ok(Self {
            id,
            attempt,
            max_attempts,
            cancelled: cancellation,
            app_state: None,
            attempt_scope: None,
            progress_reporter: None,
            metadata: ExecutionMetadata::default(),
            deadline: None,
        })
    }

    /// 设置自定义 driver 的进度持久化行为。
    #[must_use]
    pub fn with_progress_reporter(
        mut self,
        reporter: impl Fn(Value) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    ) -> Self {
        self.progress_reporter = Some(Arc::new(reporter));
        self
    }

    /// 设置自定义 worker 正在运行的应用服务状态。
    #[must_use]
    pub fn with_app_state(mut self, state: crate::foundation::ApplicationState) -> Self {
        self.app_state = Some(state);
        self
    }

    /// 设置任务执行的非敏感关联元数据。
    #[must_use]
    pub fn with_execution_metadata(mut self, metadata: ExecutionMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// 设置当前尝试的单调时钟 deadline。
    #[must_use]
    pub fn with_deadline(mut self, deadline: tokio::time::Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// 持久化任务 ID。
    pub fn id(&self) -> &str {
        &self.id
    }
    /// 当前尝试次数，从 1 开始。
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }
    /// 此任务配置的最大尝试次数。
    pub const fn max_attempts(&self) -> u64 {
        self.max_attempts
    }
    /// 当前尝试的非敏感关联元数据。
    pub fn metadata(&self) -> &ExecutionMetadata {
        &self.metadata
    }
    /// 返回当前尝试的剩余时间预算。
    pub fn remaining_budget(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(tokio::time::Instant::now()))
    }
    /// 返回 worker 停机流程是否请求了协作取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.is_cancelled()
    }

    /// 从 `queue work` worker 正常运行时取得应用中注册的服务。
    ///
    /// 直接调用 `Queue::run_next` 的 worker driver 没有应用状态，此时返回错误。
    pub async fn service<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.scope()?.resolve::<T>().await
    }

    /// 当前尝试的作用域；可用于登记随本次尝试取消并回收的受管任务。
    pub fn scope(&self) -> Result<Arc<crate::container::Scope>> {
        let state = self.app_state.as_ref().ok_or_else(|| {
            Error::custom(
                "queue_application_state_unavailable",
                "this queue execution has no application service context",
            )
        })?;
        let scope = self.attempt_scope.as_ref().ok_or_else(|| {
            Error::custom(
                "queue_attempt_scope_unavailable",
                "this queue context is not executing a job attempt",
            )
        })?;
        if !scope.belongs_to(state.container()) {
            return Err(Error::custom(
                "queue_attempt_container_mismatch",
                "job attempt belongs to another application container",
            ));
        }
        Ok(Arc::clone(scope))
    }

    /// 等待 worker 停机流程请求协作取消。
    pub async fn cancelled(&self) {
        self.cancelled.cancelled().await;
    }

    /// 仅当本次尝试仍持有任务租约时，保存有界 JSON 进度。
    pub async fn set_progress(&self, value: &Value) -> Result<()> {
        if self.is_cancelled() {
            return Err(Error::custom(
                "queue_job_cancelled",
                "job is no longer active",
            ));
        }
        let encoded = serde_json::to_string(value).map_err(Error::internal)?;
        if encoded.len() > 4096 {
            return Err(Error::custom(
                "queue_progress_too_large",
                "job progress exceeds 4 KiB",
            ));
        }
        let reporter = self.progress_reporter.as_ref().ok_or_else(|| {
            Error::custom(
                "queue_progress_unsupported",
                "this queue driver does not provide progress persistence",
            )
        })?;
        reporter(value.clone()).await
    }
}

async fn persist_database_progress(
    db: Db,
    id: String,
    owner: String,
    token: String,
    value: Value,
) -> Result<()> {
    let encoded = serde_json::to_string(&value).map_err(Error::internal)?;
    if encoded.len() > 4096 {
        return Err(Error::custom(
            "queue_progress_too_large",
            "job progress exceeds 4 KiB",
        ));
    }
    let connection = db.connection()?;
    let backend = db.backend()?;
    let table = db.quoted_table_name("jobs")?;
    let (sql, values) = match backend {
        DbBackend::Postgres => (
            format!(
                "UPDATE {table} SET progress = $1::jsonb WHERE id = $2 AND reserved_by = $3 AND ownership_token = $4"
            ),
            vec![
                encoded.into(),
                id.clone().into(),
                owner.clone().into(),
                token.clone().into(),
            ],
        ),
        DbBackend::MySql => (
            format!(
                "UPDATE {table} SET progress = CAST(? AS JSON) WHERE id = ? AND reserved_by = ? AND ownership_token = ?"
            ),
            vec![
                encoded.into(),
                id.clone().into(),
                owner.clone().into(),
                token.clone().into(),
            ],
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let result = connection
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    if result.rows_affected() == 0 {
        return Err(Error::custom(
            "queue_lease_lost",
            "job lease ownership changed",
        ));
    }
    Ok(())
}

type JobFn = fn(Value, JobContext) -> BoxFuture<'static, Result<()>>;

/// 可供队列 driver 复用的 typed Job 注册表。
///
/// 克隆共享同一目录；后续 Provider boot 登记的任务也对既有 driver 可见。
/// handler 查询在调用前结束，不持有目录锁执行用户代码。
#[derive(Default, Clone)]
pub struct JobRegistry(Arc<std::sync::RwLock<HashMap<&'static str, JobFn>>>);

impl JobRegistry {
    fn from_handlers(handlers: HashMap<&'static str, JobFn>) -> Self {
        Self(Arc::new(std::sync::RwLock::new(handlers)))
    }

    fn handler(&self, name: &str) -> Option<JobFn> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .copied()
    }

    /// 检查当前共享目录是否已经登记指定任务。
    pub fn contains(&self, name: &str) -> bool {
        self.handler(name).is_some()
    }

    /// 注册一个 typed Job；重复名称或框架保留名称会返回错误。
    pub fn register_job<J: Job>(&mut self) -> Result<&mut Self> {
        validate_name("job name", J::NAME, MAX_JOB_NAME)?;
        if is_reserved_job_name(J::NAME) {
            return Err(Error::custom(
                "reserved_queue_job_name",
                "job name is reserved for framework task groups",
            ));
        }
        let mut handlers = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if handlers.contains_key(J::NAME) {
            return Err(Error::custom(
                "duplicate_queue_job",
                format!("job handler `{}` is registered more than once", J::NAME),
            ));
        }
        handlers.insert(J::NAME, run_job::<J>);
        drop(handlers);
        Ok(self)
    }

    /// 在当前异步调用中反序列化并执行已注册的 Job。
    ///
    /// 此入口不派发或保存任务，也不提供事务派发、自动重试或任务组语义；调用方负责提供
    /// 一次尝试的 [`JobContext`]。取消仅在 handler 合作检查或等待取消时生效，进度仅在
    /// context 配置 reporter 后可用。需要持久化和恢复时应使用 [`Queue`] 与队列 driver。
    pub async fn execute(&self, job_name: &str, payload: Value, context: JobContext) -> Result<()> {
        validate_name("job name", job_name, MAX_JOB_NAME)?;
        validate_payload(&payload)?;
        let handler = self.handler(job_name).ok_or_else(|| {
            Error::custom(
                "unregistered_queue_job",
                format!("no handler is registered for job `{job_name}`"),
            )
        })?;
        let mut context = context;
        let attempt = AttemptScope::create(&mut context)?;
        let result = execute_handler(handler, payload, context).await;
        finish_attempt(attempt, result).await
    }
}

async fn finish_attempt(attempt: Option<AttemptScope>, result: Result<()>) -> Result<()> {
    if let Some(attempt) = attempt
        && let Err(cleanup) = attempt.shutdown().await
    {
        if result.is_ok() {
            return Err(cleanup);
        }
        tracing::warn!(
            code = safe_error_code(cleanup.code()),
            "queue attempt cleanup failed"
        );
    }
    result
}

fn is_reserved_job_name(name: &str) -> bool {
    name == CHAIN_JOB_NAME
        || name == BATCH_JOB_NAME
        || name == crate::notifications::EmailDeliveryJob::NAME
        || name == crate::notifications::SmsDeliveryJob::NAME
}

fn current_execution_metadata() -> Result<ExecutionMetadata> {
    let metadata = ExecutionContext::current()
        .map(|context| context.metadata().clone())
        .unwrap_or_default();
    let Some(tenant) = crate::tenant::TenantContext::current() else {
        return Ok(metadata);
    };
    if metadata
        .tenant_id()
        .is_some_and(|existing| existing != &tenant)
    {
        return Err(Error::custom_with_status(
            axum::http::StatusCode::FORBIDDEN,
            "tenant_scope_switch_denied",
            "queue dispatch tenant does not match execution metadata",
        ));
    }
    Ok(metadata.with_tenant_id(tenant))
}

pub(super) fn persist_execution_metadata(
    payload: Value,
    metadata: &ExecutionMetadata,
) -> Result<Value> {
    if metadata.is_empty() {
        return Ok(payload);
    }
    if payload.get(EXECUTION_ENVELOPE_KEY).is_some() {
        return Err(Error::custom(
            "queue_payload_reserved_field",
            "job payload uses a reserved execution metadata field",
        ));
    }
    let mut envelope = serde_json::Map::new();
    envelope.insert(
        EXECUTION_ENVELOPE_KEY.to_owned(),
        serde_json::json!(EXECUTION_ENVELOPE_VERSION),
    );
    envelope.insert(
        "metadata".to_owned(),
        serde_json::to_value(metadata).map_err(Error::internal)?,
    );
    envelope.insert("payload".to_owned(), payload);
    let wrapped = Value::Object(envelope);
    validate_payload(&wrapped)?;
    Ok(wrapped)
}

pub(super) fn restore_execution_metadata(payload: Value) -> Result<(Value, ExecutionMetadata)> {
    let is_envelope = payload.get(EXECUTION_ENVELOPE_KEY).and_then(Value::as_str)
        == Some(EXECUTION_ENVELOPE_VERSION);
    let Some(object) = payload.as_object() else {
        return Ok((payload, ExecutionMetadata::default()));
    };
    let has_envelope_shape =
        object.len() == 3 && object.contains_key("metadata") && object.contains_key("payload");
    let carries_tenant = payload
        .get("metadata")
        .and_then(Value::as_object)
        .is_some_and(|metadata| metadata.contains_key("tenant_id"));
    if carries_tenant && (!is_envelope || !has_envelope_shape) {
        return Err(Error::custom(
            "queue_execution_metadata_invalid",
            "persisted tenant metadata envelope has an invalid shape",
        ));
    }
    if !is_envelope || !has_envelope_shape {
        return Ok((payload, ExecutionMetadata::default()));
    }
    let Some(metadata) = payload
        .get("metadata")
        .cloned()
        .and_then(|value| serde_json::from_value::<ExecutionMetadata>(value).ok())
    else {
        return Err(Error::custom(
            "queue_execution_metadata_invalid",
            "persisted execution metadata envelope is invalid",
        ));
    };
    let Some(body) = payload.get("payload").cloned() else {
        return Err(Error::custom(
            "queue_execution_metadata_invalid",
            "persisted execution metadata envelope is missing its payload",
        ));
    };
    Ok((body, metadata))
}

#[derive(DeriveIden)]
#[allow(dead_code)]
enum Jobs {
    Table,
    Id,
    Queue,
    JobName,
    Payload,
    Attempts,
    MaxAttempts,
    AvailableAt,
    ReservedBy,
    OwnershipToken,
    ReservedUntil,
    Progress,
    UniqueKey,
    CreatedAt,
}

#[derive(DeriveIden)]
#[allow(dead_code)]
enum FailedJobs {
    Table,
    Id,
    Queue,
    JobName,
    Payload,
    Attempts,
    Error,
    FailedAt,
    RetriedJobId,
}

#[derive(DeriveIden)]
#[allow(dead_code)]
enum ScheduleRuns {
    Table,
    ScheduleKey,
    SlotKey,
    ScheduledAt,
    TriggeredJobId,
    CreatedAt,
}

/// 创建持久化任务表。必须在应用 migrator 中显式注册此 migration。
pub struct QueueMigration;

impl MigrationName for QueueMigration {
    fn name(&self) -> &'static str {
        "m20261026_000004_openrust_queue_jobs"
    }
}

#[async_trait::async_trait]
#[allow(clippy::too_many_lines)] // Queue tables and indexes are created as one explicit migration.
impl MigrationTrait for QueueMigration {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let table = queue_table("jobs", backend)?;
        let mut timestamp = crate::database::dialect::timestamp_utc("available_at", backend)?;
        let mut created = crate::database::dialect::timestamp_utc("created_at", backend)?;
        let mut reserved_until =
            crate::database::dialect::timestamp_utc("reserved_until", backend)?;
        let mut failed_at = crate::database::dialect::timestamp_utc("failed_at", backend)?;
        let mut ready_index = Index::create();
        ready_index
            .name("idx_openrust_jobs_ready")
            .table(table.clone())
            .col(Jobs::Queue)
            .col(Jobs::AvailableAt)
            .col(Jobs::Id);
        let mut unique_index = Index::create();
        unique_index
            .name("uq_openrust_jobs_queue_unique_key")
            .table(table.clone())
            .col(Jobs::Queue)
            .col(Jobs::UniqueKey)
            .unique();
        manager
            .create_table(
                Table::create()
                    .table(table.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Jobs::Id)
                            .string_len(32)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Jobs::Queue).string_len(64).not_null())
                    .col(ColumnDef::new(Jobs::JobName).string_len(128).not_null())
                    .col(crate::database::dialect::json("payload", backend)?.not_null())
                    .col(
                        ColumnDef::new(Jobs::Attempts)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Jobs::MaxAttempts)
                            .big_integer()
                            .not_null()
                            .default(5),
                    )
                    .col(timestamp.not_null())
                    .col(ColumnDef::new(Jobs::ReservedBy).string_len(64).null())
                    .col(ColumnDef::new(Jobs::OwnershipToken).string_len(32).null())
                    .col(reserved_until.null())
                    .col(crate::database::dialect::json("progress", backend)?.null())
                    .col(ColumnDef::new(Jobs::UniqueKey).string_len(128).null())
                    .col(created.not_null())
                    .to_owned(),
            )
            .await?;
        manager.create_index(ready_index.clone()).await?;
        manager.create_index(unique_index).await?;
        let failed_table = queue_table("failed_jobs", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(failed_table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FailedJobs::Id)
                            .string_len(32)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(FailedJobs::Queue).string_len(64).not_null())
                    .col(
                        ColumnDef::new(FailedJobs::JobName)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(crate::database::dialect::json("payload", backend)?.not_null())
                    .col(
                        ColumnDef::new(FailedJobs::Attempts)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FailedJobs::Error)
                            .string_len(1024)
                            .not_null(),
                    )
                    .col(failed_at.not_null())
                    .col(
                        ColumnDef::new(FailedJobs::RetriedJobId)
                            .string_len(32)
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;
        let mut schedule_created = crate::database::dialect::timestamp_utc("created_at", backend)?;
        let mut scheduled_at = crate::database::dialect::timestamp_utc("scheduled_at", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(queue_table("schedule_runs", backend)?)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ScheduleRuns::ScheduleKey)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ScheduleRuns::SlotKey)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(scheduled_at.not_null())
                    .col(
                        ColumnDef::new(ScheduleRuns::TriggeredJobId)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(schedule_created.not_null())
                    .primary_key(
                        Index::create()
                            .col(ScheduleRuns::ScheduleKey)
                            .col(ScheduleRuns::SlotKey),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let table = queue_table("jobs", manager.get_database_backend())?;
        let failed_table = queue_table("failed_jobs", manager.get_database_backend())?;
        let schedule_table = queue_table("schedule_runs", manager.get_database_backend())?;
        manager
            .drop_table(Table::drop().table(schedule_table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(failed_table).if_exists().to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .table(table.clone())
                    .name("idx_openrust_jobs_ready")
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .table(table.clone())
                    .name("uq_openrust_jobs_queue_unique_key")
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(table).if_exists().to_owned())
            .await
    }
}

fn queue_table(logical: &str, backend: DbBackend) -> std::result::Result<Alias, DbErr> {
    let names = crate::database::DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "queue migration requires MigrationServiceProvider or TestDatabase scope".into(),
        )
    })?;
    names
        .quote(logical, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(logical)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

/// 队列存储提供方的后端契约。
pub trait QueueDriver: Send + Sync + 'static {
    /// 使用给定的重试策略持久化序列化后的任务参数。
    fn dispatch<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>>;
    /// 在调用方的 SeaORM transaction 中持久化任务。
    fn dispatch_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>>;
    /// 按需持久化与当前调用关联的非敏感元数据。
    fn dispatch_with_metadata<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        if metadata.is_empty() {
            self.dispatch(queue, job_name, payload, options)
        } else {
            Box::pin(async {
                Err(Error::custom(
                    "queue_driver_metadata_unsupported",
                    "this queue driver does not persist execution metadata",
                ))
            })
        }
    }
    /// 在调用方事务中按需持久化执行元数据。
    fn dispatch_in_transaction_with_metadata<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        if metadata.is_empty() {
            self.dispatch_in_transaction(transaction, queue, job_name, payload, options)
        } else {
            Box::pin(async {
                Err(Error::custom(
                    "queue_driver_metadata_unsupported",
                    "this queue driver does not persist execution metadata",
                ))
            })
        }
    }
    /// 后端支持任务组执行时持久化任务组。
    fn dispatch_group<'a>(
        &'a self,
        _queue: &'a str,
        _group: TaskGroup,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async {
            Err(Error::custom(
                "queue_driver_group_unsupported",
                "this queue driver does not support task groups",
            ))
        })
    }

    /// 后端支持时，在调用方事务中持久化任务组。
    fn dispatch_group_in_transaction<'a>(
        &'a self,
        _transaction: &'a DatabaseTransaction,
        _queue: &'a str,
        _group: TaskGroup,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async {
            Err(Error::custom(
                "queue_driver_group_unsupported",
                "this queue driver does not support transactional task groups",
            ))
        })
    }

    /// 派发带有当前调用关联信息的任务组。
    fn dispatch_group_with_metadata<'a>(
        &'a self,
        queue: &'a str,
        group: TaskGroup,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        if metadata.is_empty() {
            self.dispatch_group(queue, group, options)
        } else {
            Box::pin(async {
                Err(Error::custom(
                    "queue_driver_metadata_unsupported",
                    "this queue driver does not persist execution metadata",
                ))
            })
        }
    }
    /// 在调用方事务中派发带有关联信息的任务组。
    fn dispatch_group_in_transaction_with_metadata<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        group: TaskGroup,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        if metadata.is_empty() {
            self.dispatch_group_in_transaction(transaction, queue, group, options)
        } else {
            Box::pin(async {
                Err(Error::custom(
                    "queue_driver_metadata_unsupported",
                    "this queue driver does not persist execution metadata",
                ))
            })
        }
    }
    /// 至多运行指定队列中的一个就绪任务。
    fn run_next<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<bool>>;
    /// 在应用 worker 中运行一个任务；旧 driver 默认忽略应用状态以保持实现兼容。
    fn run_next_with_state<'a>(
        &'a self,
        queue: &'a str,
        _state: &'a crate::foundation::ApplicationState,
    ) -> BoxFuture<'a, Result<bool>> {
        self.run_next(queue)
    }
    /// 将失败任务重新入队，并返回新建任务的稳定 ID。
    fn retry_failed<'a>(&'a self, failed_id: &'a str) -> BoxFuture<'a, Result<String>>;
    /// 在 worker 进程启动前清除停机状态。
    fn reset_shutdown(&self);
    /// 请求活动 handler 协作取消。
    fn request_shutdown(&self);
    /// 返回是否已请求停机。
    fn is_shutdown_requested(&self) -> bool;
}

/// 使用应用共享主库连接的数据库队列 driver。
#[derive(Clone)]
pub struct DatabaseQueue {
    db: Db,
    cancelled: tokio::sync::watch::Sender<bool>,
    registry: Arc<JobRegistry>,
    config: crate::config::QueueConfig,
}

/// 将任务持久化与执行委托给已注册 driver 的 Queue facade。
#[derive(Clone)]
pub struct Queue {
    driver: Option<Arc<dyn QueueDriver>>,
    manager: Option<Arc<QueueManager>>,
}

/// 单个任务的重试、延迟与活动任务唯一性设置。
#[derive(Debug, Clone)]
pub struct DispatchOptions {
    /// handler 的最大尝试次数，包含首次尝试。
    pub max_attempts: u32,
    /// 首次尝试前的延迟。
    pub delay: Duration,
    /// 可选的队列范围内唯一键，用于阻止重叠的重复派发。
    pub unique_key: Option<String>,
}

/// 持久化批次或链中包含的类型化名称任务。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueueTask {
    job_name: String,
    payload: Value,
}

impl QueueTask {
    /// 为显式注册的 [`Job`] 创建任务载荷。
    pub fn new<T: Serialize>(job_name: &str, payload: &T) -> Result<Self> {
        validate_name("job name", job_name, MAX_JOB_NAME)?;
        if job_name == CHAIN_JOB_NAME || job_name == BATCH_JOB_NAME {
            return Err(Error::custom(
                "reserved_queue_job_name",
                "job name is reserved for framework task groups",
            ));
        }
        let payload = serde_json::to_value(payload).map_err(Error::internal)?;
        validate_payload(&payload)?;
        Ok(Self {
            job_name: job_name.to_owned(),
            payload,
        })
    }
}

/// 控制任务组如何处理子任务错误。
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum GroupFailurePolicy {
    /// 链遇到首个错误时停止；批次遇错时停止启动后续工作并取消活动 future。
    Stop,
    /// 继续运行剩余链/批次项目；若任一子任务失败，则最终将父任务组标记为失败。
    Continue,
}

/// 通过 [`QueueDriver`] 提交的持久化队列任务组。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TaskGroup {
    /// 按列出顺序串行运行子任务。
    Chain {
        /// 子任务及其执行顺序。
        tasks: Vec<QueueTask>,
        /// 错误处理方式。
        failure_policy: GroupFailurePolicy,
    },
    /// 以每组有界并发运行子任务。
    Batch {
        /// 子任务。
        tasks: Vec<QueueTask>,
        /// 同时运行的子任务 handler future 数上限。
        concurrency: usize,
        /// 错误处理方式。
        failure_policy: GroupFailurePolicy,
    },
}

impl TaskGroup {
    fn validate(&self) -> Result<()> {
        let (tasks, concurrency) = match self {
            Self::Chain { tasks, .. } => (tasks, 1),
            Self::Batch {
                tasks, concurrency, ..
            } => (tasks, *concurrency),
        };
        if tasks.is_empty() || tasks.len() > MAX_GROUP_TASKS {
            return Err(Error::custom(
                "invalid_queue_task_group_size",
                format!("task group size must be 1..={MAX_GROUP_TASKS}"),
            ));
        }
        if !(1..=MAX_GROUP_CONCURRENCY).contains(&concurrency) {
            return Err(Error::custom(
                "invalid_queue_batch_concurrency",
                format!("batch concurrency must be 1..={MAX_GROUP_CONCURRENCY}"),
            ));
        }
        for task in tasks {
            validate_name("job name", &task.job_name, MAX_JOB_NAME)?;
            validate_payload(&task.payload)?;
        }
        let encoded = serde_json::to_vec(self).map_err(Error::internal)?;
        if encoded.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::custom(
                "queue_payload_too_large",
                "task group payload exceeds 1 MiB",
            ));
        }
        Ok(())
    }

    fn persisted_job_name(&self) -> &'static str {
        match self {
            Self::Chain { .. } => CHAIN_JOB_NAME,
            Self::Batch { .. } => BATCH_JOB_NAME,
        }
    }
}

impl Default for DispatchOptions {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            delay: Duration::ZERO,
            unique_key: None,
        }
    }
}

type QueueCreator = Arc<
    dyn Fn(
            crate::container::Resolver,
            String,
            crate::config::QueueConnectionConfig,
            crate::config::QueueConfig,
            JobRegistry,
        ) -> BoxFuture<'static, Result<Arc<dyn QueueDriver>>>
        + Send
        + Sync,
>;

type QueueValidator =
    Arc<dyn Fn(&str, &crate::config::QueueConnectionConfig) -> Result<()> + Send + Sync>;
#[derive(Clone)]
struct QueueCreatorEntry {
    creator: QueueCreator,
    validator: Option<QueueValidator>,
}

#[derive(Default)]
struct QueueContributionsInner {
    plugin: QueueSchedules,
    creators: HashMap<String, QueueCreatorEntry>,
}

/// 队列 Provider 共用的任务、调度及 driver creator 贡献目录。
pub struct QueueContributions {
    inner: std::sync::Mutex<QueueContributionsInner>,
    jobs: JobRegistry,
}

impl Default for QueueContributions {
    fn default() -> Self {
        let handlers: HashMap<&'static str, JobFn> = HashMap::from([
            (
                crate::notifications::EmailDeliveryJob::NAME,
                run_job::<crate::notifications::EmailDeliveryJob> as JobFn,
            ),
            (
                crate::notifications::SmsDeliveryJob::NAME,
                run_job::<crate::notifications::SmsDeliveryJob> as JobFn,
            ),
        ]);
        #[cfg(feature = "data-exchange-tasks")]
        let handlers = {
            let mut handlers = handlers;
            handlers.insert(
                crate::data::exchange::tasks::ImportReportCleanupJob::NAME,
                run_job::<crate::data::exchange::tasks::ImportReportCleanupJob>,
            );
            handlers
        };
        Self {
            inner: std::sync::Mutex::new(QueueContributionsInner::default()),
            jobs: JobRegistry::from_handlers(handlers),
        }
    }
}

impl QueueContributions {
    pub(crate) fn schedules(&self) -> Vec<crate::schedule::ScheduleDefinition> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .plugin
            .schedules
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn job_registry(&self) -> JobRegistry {
        self.jobs.clone()
    }

    /// 注册类型化任务；重复名称报错。
    pub fn register_job<J: Job>(&self) -> Result<()> {
        self.jobs.clone().register_job::<J>()?;
        Ok(())
    }
    /// 注册固定间隔任务。
    pub fn schedule_interval<T: Serialize>(
        &self,
        key: &str,
        queue: &str,
        job: &str,
        payload: &T,
        interval: Duration,
    ) -> Result<()> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .plugin
            .schedule_interval(key, queue, job, payload, interval)?;
        Ok(())
    }
    /// 注册每日 UTC 任务。
    pub fn schedule_daily_at<T: Serialize>(
        &self,
        key: &str,
        queue: &str,
        job: &str,
        payload: &T,
        hour: u8,
        minute: u8,
    ) -> Result<()> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .plugin
            .schedule_daily_at(key, queue, job, payload, hour, minute)?;
        Ok(())
    }
    /// 注册带时区和补跑策略的 cron 任务。
    pub fn schedule_cron<T: Serialize>(
        &self,
        key: &str,
        queue: &str,
        job: &str,
        payload: &T,
        expression: &str,
        options: crate::schedule::CronOptions,
    ) -> Result<()> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .plugin
            .schedule_cron(key, queue, job, payload, expression, options)?;
        Ok(())
    }
    /// 注册或替换 driver creator；已创建的连接保持不变。
    ///
    /// 此方法不校验 driver 专属选项；需要最终配置校验时使用 `extend_validated`。
    pub fn extend<F>(&self, name: &str, creator: F) -> Result<()>
    where
        F: Fn(
                crate::container::Resolver,
                String,
                crate::config::QueueConnectionConfig,
                crate::config::QueueConfig,
                JobRegistry,
            ) -> BoxFuture<'static, Result<Arc<dyn QueueDriver>>>
            + Send
            + Sync
            + 'static,
    {
        self.install_creator(name, Arc::new(creator), None)
    }

    /// 注册 creator 与不执行 I/O 的配置校验器；同名替换两者。
    pub fn extend_validated<F, V>(&self, name: &str, validator: V, creator: F) -> Result<()>
    where
        F: Fn(
                crate::container::Resolver,
                String,
                crate::config::QueueConnectionConfig,
                crate::config::QueueConfig,
                JobRegistry,
            ) -> BoxFuture<'static, Result<Arc<dyn QueueDriver>>>
            + Send
            + Sync
            + 'static,
        V: Fn(&str, &crate::config::QueueConnectionConfig) -> Result<()> + Send + Sync + 'static,
    {
        self.install_creator(name, Arc::new(creator), Some(Arc::new(validator)))
    }

    fn install_creator(
        &self,
        name: &str,
        creator: QueueCreator,
        validator: Option<QueueValidator>,
    ) -> Result<()> {
        validate_name("driver", name, MAX_QUEUE_NAME)?;
        let previous = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .creators
            .insert(name.to_owned(), QueueCreatorEntry { creator, validator });
        drop(previous);
        Ok(())
    }
}

fn queue_stopped() -> Error {
    Error::custom("queue_shutdown_requested", "queue manager is stopped")
}

type QueueConnections =
    std::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<Arc<dyn QueueDriver>>>>>;

struct QueueManager {
    resolver: crate::container::Resolver,
    contributions: Arc<QueueContributions>,
    connections: QueueConnections,
    shutdown: std::sync::atomic::AtomicBool,
    generation: std::sync::atomic::AtomicU64,
}

/// 在统一容器中装配任务目录、队列连接与 worker 命令。
#[derive(Default)]
pub struct QueueServiceProvider {
    contributions: Arc<QueueContributions>,
}

impl QueueServiceProvider {
    /// 创建共享贡献目录。
    pub fn new() -> Self {
        Self::default()
    }
    /// 获取可在 Provider register 阶段共享的贡献句柄。
    pub fn contributions(&self) -> Arc<QueueContributions> {
        Arc::clone(&self.contributions)
    }
}

impl crate::support::ServiceProvider for QueueServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            app.config_section(
                "queue",
                crate::config::QueueConfig::default(),
                S::object([
                    ("default", S::String),
                    ("connections", S::map(S::any().sensitive())),
                    ("poll_interval_millis", S::Integer),
                    ("lease_secs", S::Integer),
                    ("job_timeout_secs", S::Integer),
                    ("worker_concurrency", S::Integer),
                    ("shutdown_timeout_secs", S::Integer),
                ]),
                validate_config,
            )?;
            let contributions = Arc::clone(&self.contributions);
            app.instance(Arc::clone(&contributions)).await?;
            app.singleton::<crate::config::QueueConfig, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(
                        resolver.section::<crate::config::QueueConfig>("queue")?,
                    ))
                })
            })
            .await?;
            app.singleton::<Queue, _>(move |resolver| {
                Box::pin(async move {
                    let contributions = resolver.resolve::<QueueContributions>().await?;
                    Ok(Arc::new(Queue {
                        driver: None,
                        manager: Some(Arc::new(QueueManager {
                            resolver,
                            contributions,
                            connections: std::sync::Mutex::new(HashMap::new()),
                            shutdown: std::sync::atomic::AtomicBool::new(false),
                            generation: std::sync::atomic::AtomicU64::new(0),
                        })),
                    }))
                })
            })
            .await?;
            app.singleton::<crate::schedule::Scheduler, _>(move |resolver| {
                Box::pin(async move {
                    let contributions = resolver.resolve::<QueueContributions>().await?;
                    let db = resolver.resolve::<Db>().await?;
                    let queue = resolver.resolve::<Queue>().await?;
                    let config = resolver.section::<crate::config::QueueConfig>("queue")?;
                    Ok(Arc::new(
                        crate::schedule::Scheduler::new(
                            (*db).clone(),
                            (*queue).clone(),
                            contributions.schedules(),
                            config,
                            resolver.scope_factory(),
                        )
                        .with_live_contributions(contributions),
                    ))
                })
            })
            .await?;
            app.require::<Queue>();
            Ok(())
        })
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let queue = app.resolve::<Queue>().await?;
            let final_queue = Arc::clone(&queue);
            app.validate_at_finalize(move || {
                Box::pin(async move { final_queue.validate_configured() })
            });
            app.command(QueueWorkCommand);
            app.command(QueueRetryCommand);
            app.command(crate::schedule::ScheduleRunCommand);
            app.command(crate::schedule::ScheduleListCommand);
            Ok(())
        })
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::support::ServiceProvider;
    use figment::{Figment, providers::Serialized};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn job_context_resolves_factory_service() -> Result<()> {
        struct FactoryValue(u8);
        struct FactoryProvider;
        impl ServiceProvider for FactoryProvider {
            fn register<'a>(
                &'a self,
                app: &'a mut crate::foundation::RegisterContext<'_>,
            ) -> crate::support::BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.singleton::<FactoryValue, _>(|_| {
                        Box::pin(async { Ok(Arc::new(FactoryValue(7))) })
                    })
                    .await?;
                    Ok(())
                })
            }
        }
        #[derive(serde::Deserialize)]
        struct FactoryJob {}
        impl Job for FactoryJob {
            const NAME: &'static str = "test.factory-scope";
            fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
                Box::pin(async move {
                    assert_eq!(context.service::<FactoryValue>().await?.0, 7);
                    Ok(())
                })
            }
        }
        let mut builder = ProviderBuilder::new();
        builder.provider(FactoryProvider);
        let app = crate::foundation::Application::from_provider_runtime(
            builder.start(Figment::new()).await?,
        )?;
        let context =
            JobContext::new("job", 1, 1, Cancellation::new())?.with_app_state(app.state().clone());
        let mut registry = JobRegistry::default();
        registry.register_job::<FactoryJob>()?;
        registry
            .execute(FactoryJob::NAME, serde_json::json!({}), context)
            .await?;
        let missing = JobContext::new("job", 1, 1, Cancellation::new())?;
        assert_eq!(
            missing
                .service::<FactoryValue>()
                .await
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("queue_application_state_unavailable")
        );
        Ok(())
    }

    #[tokio::test]
    async fn attempt_preserves_handler_error_when_cleanup_fails() -> Result<()> {
        struct Scoped;
        struct Provider;
        impl ServiceProvider for Provider {
            fn register<'a>(
                &'a self,
                app: &'a mut crate::foundation::RegisterContext<'_>,
            ) -> crate::support::BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.scoped::<Scoped, _>(|resolver| {
                        Box::pin(async move {
                            resolver.resources()?.on_shutdown(|| {
                                Box::pin(async {
                                    Err(Error::custom(
                                        "cleanup_failure",
                                        "sensitive cleanup detail",
                                    ))
                                })
                            })?;
                            Ok(Arc::new(Scoped))
                        })
                    })
                    .await?;
                    Ok(())
                })
            }
        }
        #[derive(serde::Deserialize)]
        struct FailingJob {}
        static HANDLED: AtomicUsize = AtomicUsize::new(0);
        impl Job for FailingJob {
            const NAME: &'static str = "test.handler-error";
            fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
                assert!(ExecutionContext::current().is_some());
                HANDLED.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    context.service::<Scoped>().await?;
                    Err(Error::custom("handler_failure", "handler failed"))
                })
            }
        }
        let mut builder = ProviderBuilder::new();
        builder.provider(Provider);
        let app = crate::foundation::Application::from_provider_runtime(
            builder.start(Figment::new()).await?,
        )?;
        let mut registry = JobRegistry::default();
        registry.register_job::<FailingJob>()?;
        let context =
            JobContext::new("job", 1, 1, Cancellation::new())?.with_app_state(app.state().clone());
        let error = registry
            .execute(FailingJob::NAME, serde_json::json!({}), context)
            .await
            .err();
        assert_eq!(error.as_ref().map(Error::code), Some("handler_failure"));
        assert_eq!(HANDLED.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn attempt_rejects_application_rebinding_before_handler() -> Result<()> {
        static HANDLED: AtomicUsize = AtomicUsize::new(0);
        #[derive(serde::Deserialize)]
        struct ReboundJob {}
        impl Job for ReboundJob {
            const NAME: &'static str = "test.rebound-attempt";
            fn handle(&self, _: JobContext) -> BoxFuture<'_, Result<()>> {
                HANDLED.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let app_a = crate::foundation::Application::from_provider_runtime(
            ProviderBuilder::new().start(Figment::new()).await?,
        )?;
        let app_b = crate::foundation::Application::from_provider_runtime(
            ProviderBuilder::new().start(Figment::new()).await?,
        )?;
        let attempt = JobContext::new("job", 1, 1, Cancellation::new())?
            .with_app_state(app_a.state().clone())
            .begin_attempt()?;
        let mut registry = JobRegistry::default();
        registry.register_job::<ReboundJob>()?;
        let rebound = attempt
            .context()
            .clone()
            .with_app_state(app_b.state().clone());
        assert_eq!(
            rebound
                .scope()
                .err()
                .map(|e| e.code().to_owned())
                .as_deref(),
            Some("queue_attempt_container_mismatch")
        );
        assert_eq!(
            rebound
                .service::<u8>()
                .await
                .err()
                .map(|e| e.code().to_owned())
                .as_deref(),
            Some("queue_attempt_container_mismatch")
        );
        let error = registry
            .execute(ReboundJob::NAME, serde_json::json!({}), rebound)
            .await
            .err();
        assert_eq!(
            error.as_ref().map(Error::code),
            Some("queue_attempt_container_mismatch")
        );
        assert_eq!(HANDLED.load(Ordering::SeqCst), 0);
        registry
            .execute(
                ReboundJob::NAME,
                serde_json::json!({}),
                attempt
                    .context()
                    .clone()
                    .with_app_state(app_a.state().clone()),
            )
            .await?;
        assert_eq!(HANDLED.load(Ordering::SeqCst), 1);
        attempt
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        Ok(())
    }

    struct Driver;
    impl QueueDriver for Driver {
        fn dispatch<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: Value,
            _: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Ok("id".into()) })
        }
        fn dispatch_in_transaction<'a>(
            &'a self,
            _: &'a DatabaseTransaction,
            _: &'a str,
            _: &'a str,
            _: Value,
            _: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Ok("id".into()) })
        }
        fn run_next<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn retry_failed<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Ok("id".into()) })
        }
        fn reset_shutdown(&self) {}
        fn request_shutdown(&self) {}
        fn is_shutdown_requested(&self) -> bool {
            false
        }
    }
    fn creator(
        count: Arc<AtomicUsize>,
    ) -> impl Fn(
        crate::container::Resolver,
        String,
        crate::config::QueueConnectionConfig,
        crate::config::QueueConfig,
        JobRegistry,
    ) -> BoxFuture<'static, Result<Arc<dyn QueueDriver>>>
    + Send
    + Sync {
        move |_, _, _, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Driver) as Arc<dyn QueueDriver>)
            })
        }
    }
    fn sources() -> Figment {
        Figment::from(Serialized::defaults(
            serde_json::json!({"queue":{"default":"first","connections":{"first":{"driver":"custom"},"second":{"driver":"custom"}}}}),
        ))
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct ExampleJob;
    impl Job for ExampleJob {
        const NAME: &'static str = "test.queue.provider";
        fn handle(&self, _: JobContext) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }
    #[tokio::test]
    async fn registered_job_is_visible_to_creator() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let contributions = provider.contributions();
        contributions.register_job::<ExampleJob>()?;
        contributions.extend("custom", |_, _, _, _, registry| {
            Box::pin(async move {
                if !registry.contains(ExampleJob::NAME) {
                    return Err(Error::custom("test_missing_job", "missing"));
                }
                Ok(Arc::new(Driver) as Arc<dyn QueueDriver>)
            })
        })?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(sources()).await?;
        runtime
            .container()
            .resolve::<Queue>()
            .await?
            .connection("first")
            .await?;
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct Early;
    impl ServiceProvider for Early {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let queue = app.resolve::<Queue>().await?;
                let _ = queue.connection("first").await?;
                Ok(())
            })
        }
    }
    struct EarlyScheduler;
    impl ServiceProvider for EarlyScheduler {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let scheduler = app.resolve::<crate::schedule::Scheduler>().await?;
                assert!(scheduler.entries().is_empty());
                Ok(())
            })
        }
    }
    struct LateSchedule;
    impl ServiceProvider for LateSchedule {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.resolve::<QueueContributions>()
                    .await?
                    .schedule_interval(
                        "module-late",
                        "default",
                        ExampleJob::NAME,
                        &serde_json::json!({}),
                        Duration::from_secs(60),
                    )?;
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn early_scheduler_observes_late_module_schedule() -> Result<()> {
        let provider = QueueServiceProvider::new();
        provider.contributions().register_job::<ExampleJob>()?;
        provider
            .contributions()
            .extend("custom", creator(Arc::new(AtomicUsize::new(0))))?;
        let mut builder = ProviderBuilder::new();
        builder
            .provider(crate::database::DatabaseServiceProvider)
            .provider(EarlyScheduler)
            .provider(provider)
            .provider(LateSchedule);
        let runtime = builder.start(sources()).await?;
        let scheduler = runtime
            .container()
            .resolve::<crate::schedule::Scheduler>()
            .await?;
        assert_eq!(scheduler.entries().len(), 1);
        assert_eq!(scheduler.entries()[0].key(), "module-late");
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    struct LateDefaultConnection;
    impl ServiceProvider for LateDefaultConnection {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("queue", serde_json::json!({"default":"second"})) })
        }
    }
    #[tokio::test]
    async fn late_module_default_overlay_keeps_existing_connection_and_updates_default()
    -> Result<()> {
        let provider = QueueServiceProvider::new();
        let count = Arc::new(AtomicUsize::new(0));
        provider
            .contributions()
            .extend("custom", creator(Arc::clone(&count)))?;
        let mut builder = ProviderBuilder::new();
        builder
            .provider(crate::database::DatabaseServiceProvider)
            .provider(provider)
            .provider(Early)
            .provider(LateDefaultConnection);
        let runtime = builder.start(sources()).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        let first = queue.connection("first").await?;
        let second = queue.connection("second").await?;
        let default = queue.resolve_driver().await?;
        let retained_first = queue.connection("first").await?;
        assert!(Arc::ptr_eq(
            first
                .driver
                .as_ref()
                .ok_or_else(|| Error::custom("test", "first driver missing"))?,
            retained_first
                .driver
                .as_ref()
                .ok_or_else(|| Error::custom("test", "first driver missing"))?
        ));
        assert!(Arc::ptr_eq(
            second
                .driver
                .as_ref()
                .ok_or_else(|| Error::custom("test", "second driver missing"))?,
            &default
        ));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(
            runtime
                .commands()
                .any(|command| command.name() == "schedule list")
        );
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    #[tokio::test]
    async fn early_resolve_late_extend_and_named_singleflight() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let contributions = provider.contributions();
        let first_count = Arc::new(AtomicUsize::new(0));
        let next_count = Arc::new(AtomicUsize::new(0));
        contributions.extend("custom", creator(Arc::clone(&first_count)))?;
        let mut builder = ProviderBuilder::new();
        builder.provider(Early).provider(provider);
        let runtime = builder.start(sources()).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        let first = queue
            .connection("first")
            .await?
            .driver
            .ok_or_else(|| Error::custom("test", "missing"))?;
        contributions.extend("custom", creator(Arc::clone(&next_count)))?;
        let retained = queue
            .connection("first")
            .await?
            .driver
            .ok_or_else(|| Error::custom("test", "missing"))?;
        assert!(Arc::ptr_eq(&first, &retained));
        let (left, right) = tokio::join!(queue.connection("second"), queue.connection("second"));
        assert!(Arc::ptr_eq(
            &left?
                .driver
                .ok_or_else(|| Error::custom("test", "missing"))?,
            &right?
                .driver
                .ok_or_else(|| Error::custom("test", "missing"))?
        ));
        assert_eq!(first_count.load(Ordering::SeqCst), 1);
        assert_eq!(next_count.load(Ordering::SeqCst), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    struct BadOptions;
    impl ServiceProvider for BadOptions {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay("queue", serde_json::json!({"connections":{"second":{"options":{"valid":false,"secret":"secret-sentinel"}}}}))
            })
        }
    }
    fn require_valid(_: &str, config: &crate::config::QueueConnectionConfig) -> Result<()> {
        if config.options.get("valid").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(Error::custom("private", "secret-sentinel"))
        }
    }
    #[tokio::test]
    async fn validated_creator_rejects_late_invalid_options_and_redacts() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let count = Arc::new(AtomicUsize::new(0));
        provider.contributions().extend_validated(
            "custom",
            require_valid,
            creator(Arc::clone(&count)),
        )?;
        let source = Figment::from(Serialized::defaults(
            serde_json::json!({"queue":{"default":"first","connections":{"first":{"driver":"custom","options":{"valid":true}},"second":{"driver":"custom","options":{"valid":true}}}}}),
        ));
        let mut builder = ProviderBuilder::new();
        builder.provider(provider).provider(BadOptions);
        let runtime = builder.start(source).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        assert_eq!(count.load(Ordering::SeqCst), 0);
        queue.connection("first").await?;
        let result = queue.connection("second").await;
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    #[tokio::test]
    async fn replacing_creator_discards_previous_validator() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let contributions = provider.contributions();
        contributions.extend_validated(
            "custom",
            |_, _| Err(Error::custom("old", "old")),
            creator(Arc::new(AtomicUsize::new(0))),
        )?;
        contributions.extend("custom", creator(Arc::new(AtomicUsize::new(0))))?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(sources()).await?;
        runtime
            .container()
            .resolve::<Queue>()
            .await?
            .connection("first")
            .await?;
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct BadOverlay;
    impl ServiceProvider for BadOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay(
                    "queue",
                    serde_json::json!({"connections":{"second":{"driver":"missing"}}}),
                )
            })
        }
    }
    #[tokio::test]
    async fn late_invalid_driver_fails_only_when_selected() -> Result<()> {
        let provider = QueueServiceProvider::new();
        provider
            .contributions()
            .extend("custom", creator(Arc::new(AtomicUsize::new(0))))?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider).provider(BadOverlay);
        let runtime = builder.start(sources()).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        queue.connection("first").await?;
        assert!(queue.connection("second").await.is_err());
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    struct OverrideContributions(Arc<QueueContributions>);
    impl ServiceProvider for OverrideContributions {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::clone(&self.0)).await?;
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn factory_uses_final_contributions_binding() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let original_count = Arc::new(AtomicUsize::new(0));
        provider
            .contributions()
            .extend("custom", creator(Arc::clone(&original_count)))?;
        let replacement = Arc::new(QueueContributions::default());
        let replacement_count = Arc::new(AtomicUsize::new(0));
        replacement.extend("custom", creator(Arc::clone(&replacement_count)))?;
        let mut builder = ProviderBuilder::new();
        builder
            .provider(provider)
            .provider(OverrideContributions(Arc::clone(&replacement)));
        let runtime = builder.start(sources()).await?;
        runtime
            .container()
            .resolve::<Queue>()
            .await?
            .connection("first")
            .await?;
        assert!(Arc::ptr_eq(
            &runtime.container().resolve::<QueueContributions>().await?,
            &replacement
        ));
        assert_eq!(original_count.load(Ordering::SeqCst), 0);
        assert_eq!(replacement_count.load(Ordering::SeqCst), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    struct LateJob;
    impl ServiceProvider for LateJob {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.resolve::<QueueContributions>()
                    .await?
                    .register_job::<ExampleJob>()
            })
        }
    }
    #[tokio::test]
    async fn boot_job_registration_is_visible_to_existing_driver() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let observed = Arc::new(std::sync::Mutex::new(None::<JobRegistry>));
        let captured = Arc::clone(&observed);
        provider
            .contributions()
            .extend("custom", move |_, _, _, _, registry| {
                *captured
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(registry);
                Box::pin(async { Ok(Arc::new(Driver) as Arc<dyn QueueDriver>) })
            })?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider).provider(Early).provider(LateJob);
        let runtime = builder.start(sources()).await?;
        let registry = observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Error::custom("test", "registry missing"))?;
        assert!(registry.contains(ExampleJob::NAME));
        assert!(registry.contains(crate::notifications::EmailDeliveryJob::NAME));
        assert!(registry.contains(crate::notifications::SmsDeliveryJob::NAME));
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    #[derive(Default)]
    struct StopDriver(std::sync::atomic::AtomicBool);
    impl QueueDriver for StopDriver {
        fn dispatch<'a>(
            &'a self,
            queue: &'a str,
            job: &'a str,
            value: Value,
            options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Driver.dispatch(queue, job, value, options)
        }
        fn dispatch_in_transaction<'a>(
            &'a self,
            transaction: &'a DatabaseTransaction,
            queue: &'a str,
            job: &'a str,
            value: Value,
            options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Driver.dispatch_in_transaction(transaction, queue, job, value, options)
        }
        fn run_next<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn retry_failed<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Ok("id".into()) })
        }
        fn reset_shutdown(&self) {
            self.0.store(false, Ordering::SeqCst);
        }
        fn request_shutdown(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
        fn is_shutdown_requested(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    #[tokio::test]
    async fn shutdown_seals_unresolved_names_and_stops_all_cached_drivers() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let count = Arc::new(AtomicUsize::new(0));
        let captured = Arc::clone(&count);
        provider
            .contributions()
            .extend("custom", move |_, _, _, _, _| {
                captured.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(Arc::new(StopDriver::default()) as Arc<dyn QueueDriver>) })
            })?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(sources()).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        let first = queue.connection("first").await?;
        queue.request_shutdown();
        assert!(queue.is_shutdown_requested());
        assert!(first.is_shutdown_requested());
        assert_eq!(
            queue
                .connection("second")
                .await
                .err()
                .map(|e| e.code().to_owned())
                .as_deref(),
            Some("queue_shutdown_requested")
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!queue.run_next("default").await?);
        queue.reset_shutdown();
        let second = queue.connection("second").await?;
        let rebuilt = queue.connection("first").await?;
        queue.request_shutdown();
        assert!(second.is_shutdown_requested());
        assert!(rebuilt.is_shutdown_requested());
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    #[tokio::test]
    async fn inflight_old_generation_cannot_publish_after_stop_and_reset() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let slow = Arc::new(StopDriver::default());
        let count = Arc::new(AtomicUsize::new(0));
        let (s, r, d, c) = (
            Arc::clone(&started),
            Arc::clone(&release),
            Arc::clone(&slow),
            Arc::clone(&count),
        );
        provider
            .contributions()
            .extend("custom", move |_, name, _, _, _| {
                let (s, r, d, c) = (
                    Arc::clone(&s),
                    Arc::clone(&r),
                    Arc::clone(&d),
                    Arc::clone(&c),
                );
                Box::pin(async move {
                    if name == "second" && c.fetch_add(1, Ordering::SeqCst) == 0 {
                        s.notify_one();
                        r.notified().await;
                        Ok(d as Arc<dyn QueueDriver>)
                    } else {
                        Ok(Arc::new(StopDriver::default()) as Arc<dyn QueueDriver>)
                    }
                })
            })?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(sources()).await?;
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let queue = runtime.container().resolve::<Queue>().await?;
        let task_queue = Arc::clone(&queue);
        let task = tokio::spawn(async move { task_queue.connection("second").await });
        started.notified().await;
        queue.request_shutdown();
        queue.reset_shutdown();
        let current = queue.connection("second").await?;
        release.notify_one();
        assert!(
            task.await
                .map_err(|_| Error::custom("test", "task failed"))?
                .is_err()
        );
        assert!(slow.is_shutdown_requested());
        assert!(!current.is_shutdown_requested());
        let again = queue.connection("second").await?;
        assert!(Arc::ptr_eq(
            current
                .driver
                .as_ref()
                .ok_or_else(|| Error::custom("test", "missing"))?,
            again
                .driver
                .as_ref()
                .ok_or_else(|| Error::custom("test", "missing"))?
        ));
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    #[cfg(feature = "queue-redis")]
    struct RedisOverlay;
    #[cfg(feature = "queue-redis")]
    impl ServiceProvider for RedisOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay(
                    "queue",
                    serde_json::json!({"connections":{"redis":{"redis_url":null}}}),
                )
            })
        }
    }
    #[cfg(feature = "queue-redis")]
    #[tokio::test]
    async fn selected_redis_is_validated_after_overlay_without_io() -> Result<()> {
        let source = Figment::from(Serialized::defaults(
            serde_json::json!({"queue":{"default":"redis","connections":{"redis":{"driver":"redis","redis_url":"redis://127.0.0.1:1","redis_namespace":"test"}}}}),
        ));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(QueueServiceProvider::new())
            .provider(RedisOverlay);
        let runtime = builder.start(source).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        assert!(queue.connection("redis").await.is_err());
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
    #[cfg(feature = "queue-redis")]
    #[tokio::test]
    async fn named_redis_url_and_namespace_are_validated_without_io() -> Result<()> {
        for (url, namespace) in [
            ("secret-sentinel", "valid"),
            ("redis://127.0.0.1:1", "bad:secret-sentinel"),
        ] {
            let provider = QueueServiceProvider::new();
            provider
                .contributions()
                .extend("custom", creator(Arc::new(AtomicUsize::new(0))))?;
            let source = Figment::from(Serialized::defaults(
                serde_json::json!({"queue":{"default":"first","connections":{"first":{"driver":"custom"},"second":{"driver":"redis","redis_url":url,"redis_namespace":namespace}}}}),
            ));
            let mut builder = ProviderBuilder::new();
            builder.provider(provider);
            let runtime = builder.start(source).await?;
            let queue = runtime.container().resolve::<Queue>().await?;
            queue.connection("first").await?;
            let error = queue
                .connection("second")
                .await
                .err()
                .ok_or_else(|| Error::custom("test", "invalid configuration accepted"))?;
            assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn boot_without_database_pool_is_lazy_but_selected_database_fails() -> Result<()> {
        for include_database_provider in [false, true] {
            let mut builder = ProviderBuilder::new();
            if include_database_provider {
                builder.provider(crate::database::DatabaseServiceProvider);
            }
            builder.provider(QueueServiceProvider::new());
            let runtime = builder.start(Figment::new()).await?;
            let queue = runtime.container().resolve::<Queue>().await?;
            assert!(queue.connection("database").await.is_err());
            assert!(
                queue
                    .dispatch("default", ExampleJob::NAME, &serde_json::json!({}))
                    .await
                    .is_err()
            );
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn legacy_top_level_queue_fields_fail_without_disclosing_values() {
        for field in [
            "driver",
            "default_connection",
            "redis_url",
            "redis_namespace",
        ] {
            let mut queue = serde_json::Map::new();
            queue.insert(
                field.to_owned(),
                Value::String("secret-sentinel".to_owned()),
            );
            let mut builder = ProviderBuilder::new();
            builder.provider(QueueServiceProvider::new());
            let result = builder
                .start(Figment::from(Serialized::defaults(
                    serde_json::json!({"queue":queue}),
                )))
                .await;
            assert!(result.is_err());
            if let Err(error) = result {
                assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
            }
        }
    }

    #[cfg(feature = "queue-redis")]
    #[tokio::test]
    async fn redis_connection_can_coexist_with_database_schedule_declarations() -> Result<()> {
        let provider = QueueServiceProvider::new();
        let contributions = provider.contributions();
        contributions.register_job::<ExampleJob>()?;
        contributions.schedule_interval(
            "heartbeat",
            "default",
            ExampleJob::NAME,
            &serde_json::json!({}),
            Duration::from_secs(60),
        )?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let source = Figment::from(Serialized::defaults(serde_json::json!({
            "queue":{"default":"redis","connections":{"redis":{"driver":"redis","redis_url":"redis://127.0.0.1:1","redis_namespace":"schedule-coexist"}}}
        })));
        let runtime = builder.start(source).await?;
        let queue = runtime.container().resolve::<Queue>().await?;
        queue.connection("redis").await?;
        assert_eq!(contributions.schedules().len(), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }

    #[test]
    fn worker_options_distinguish_connections_from_queues() -> Result<()> {
        let mut config = crate::config::QueueConfig::default();
        config.connections.insert(
            "redis".into(),
            serde_json::json!({"driver":"redis", "queue":"priority"}),
        );
        for (arguments, expected_connection, expected_queue) in [
            (vec![], "database", "default"),
            (vec!["redis"], "redis", "priority"),
            (vec!["--queue", "emails"], "database", "emails"),
            (vec!["redis", "--queue=emails"], "redis", "emails"),
            (vec!["--queue=emails", "redis"], "redis", "emails"),
        ] {
            let arguments: Vec<_> = arguments.into_iter().map(str::to_owned).collect();
            assert_eq!(
                queue_work_options(&config, &arguments)?,
                (expected_connection.to_owned(), expected_queue.to_owned())
            );
        }
        for arguments in [
            vec!["database", "redis"],
            vec!["--unknown"],
            vec!["--queue"],
            vec!["--queue", "--unknown"],
            vec!["--queue="],
            vec!["--queue=one", "--queue=two"],
            vec!["missing"],
        ] {
            let arguments: Vec<_> = arguments.into_iter().map(str::to_owned).collect();
            assert!(queue_work_options(&config, &arguments).is_err());
        }
        for queue in [serde_json::json!(""), Value::Null, serde_json::json!(17)] {
            config.connections.insert(
                "redis".into(),
                serde_json::json!({"driver":"redis","queue":queue}),
            );
            assert!(queue_work_options(&config, &["redis".into()]).is_err());
        }
        Ok(())
    }

    struct WorkerProbe(Arc<std::sync::Mutex<Vec<String>>>);
    impl QueueDriver for WorkerProbe {
        fn dispatch<'a>(
            &'a self,
            queue: &'a str,
            job: &'a str,
            payload: Value,
            options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Driver.dispatch(queue, job, payload, options)
        }
        fn dispatch_in_transaction<'a>(
            &'a self,
            transaction: &'a DatabaseTransaction,
            queue: &'a str,
            job: &'a str,
            payload: Value,
            options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Driver.dispatch_in_transaction(transaction, queue, job, payload, options)
        }
        fn run_next<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .map_err(|_| Error::custom("test", "worker probe lock poisoned"))?
                    .push(queue.to_owned());
                Err(Error::custom(
                    "worker_probe_stop",
                    "stop after observing selected queue",
                ))
            })
        }
        fn retry_failed<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<String>> {
            Driver.retry_failed(id)
        }
        fn reset_shutdown(&self) {}
        fn request_shutdown(&self) {}
        fn is_shutdown_requested(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn worker_command_runs_selected_connection_and_queue_without_default_fallback()
    -> Result<()> {
        let provider = QueueServiceProvider::new();
        let created = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::clone(&created);
        let seen = Arc::clone(&observed);
        provider
            .contributions()
            .extend("probe", move |_, name, config, _, _| {
                assert_eq!(name, "second");
                assert_eq!(config.queue, "urgent");
                count.fetch_add(1, Ordering::SeqCst);
                let seen = Arc::clone(&seen);
                Box::pin(async move { Ok(Arc::new(WorkerProbe(seen)) as Arc<dyn QueueDriver>) })
            })?;
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let source = Figment::from(Serialized::defaults(serde_json::json!({
            "queue":{"default":"first","connections":{"first":{"driver":"probe"},"second":{"driver":"probe","queue":"urgent"}}}
        })));
        let app =
            crate::foundation::Application::from_provider_runtime(builder.start(source).await?)?;
        assert_eq!(created.load(Ordering::SeqCst), 0);
        for arguments in [vec!["second"], vec!["second", "--queue", "explicit"]] {
            let result = app
                .run_command(
                    "queue work",
                    arguments.into_iter().map(str::to_owned).collect(),
                )
                .await;
            assert!(matches!(result, Err(error) if error.code() == "worker_probe_stop"));
        }
        assert_eq!(created.load(Ordering::SeqCst), 2);
        assert_eq!(
            *observed
                .lock()
                .map_err(|_| Error::custom("test", "worker probe lock poisoned"))?,
            ["urgent", "explicit"]
        );
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
    }
}

/// 注册队列服务与显式允许的任务 handler。
#[derive(Debug, Default, Clone)]
struct QueueSchedules {
    schedules: Vec<crate::schedule::ScheduleDefinition>,
}

impl QueueSchedules {
    /// 注册一个任务，使其在每个有界 UTC interval 时段派发。
    pub fn schedule_interval<T: Serialize>(
        &mut self,
        key: &str,
        queue: &str,
        job_name: &str,
        payload: &T,
        interval: Duration,
    ) -> Result<&mut Self> {
        self.register_schedule(
            key,
            queue,
            job_name,
            payload,
            crate::schedule::ScheduleFrequency::Interval(interval),
        )
    }

    /// 注册一个任务，使其在每日指定 UTC 小时和分钟派发，并跳过错过的日期时段。
    pub fn schedule_daily_at<T: Serialize>(
        &mut self,
        key: &str,
        queue: &str,
        job_name: &str,
        payload: &T,
        hour_utc: u8,
        minute_utc: u8,
    ) -> Result<&mut Self> {
        self.register_schedule(
            key,
            queue,
            job_name,
            payload,
            crate::schedule::ScheduleFrequency::DailyAtUtc {
                hour: hour_utc,
                minute: minute_utc,
            },
        )
    }

    /// 注册一个五字段 cron 任务，按指定 IANA 时区计算触发时段。
    pub fn schedule_cron<T: Serialize>(
        &mut self,
        key: &str,
        queue: &str,
        job_name: &str,
        payload: &T,
        expression: &str,
        options: crate::schedule::CronOptions,
    ) -> Result<&mut Self> {
        let crate::schedule::CronOptions {
            time_zone,
            missed_runs,
        } = options;
        let frequency =
            crate::schedule::ScheduleFrequency::cron(expression, &time_zone, missed_runs)?;
        self.register_schedule(key, queue, job_name, payload, frequency)
    }

    fn register_schedule<T: Serialize>(
        &mut self,
        key: &str,
        queue: &str,
        job_name: &str,
        payload: &T,
        frequency: crate::schedule::ScheduleFrequency,
    ) -> Result<&mut Self> {
        validate_name("schedule key", key, 128)?;
        validate_name("queue", queue, MAX_QUEUE_NAME)?;
        validate_name("job name", job_name, MAX_JOB_NAME)?;
        frequency.validate()?;
        if self.schedules.iter().any(|schedule| schedule.key() == key) {
            return Err(Error::custom(
                "duplicate_schedule_key",
                format!("schedule `{key}` is registered more than once"),
            ));
        }
        let payload = serde_json::to_value(payload).map_err(Error::internal)?;
        validate_payload(&payload)?;
        self.schedules
            .push(crate::schedule::ScheduleDefinition::new(
                key.to_owned(),
                queue.to_owned(),
                job_name.to_owned(),
                payload,
                frequency,
            ));
        Ok(self)
    }
}

fn run_job<J: Job>(payload: Value, context: JobContext) -> BoxFuture<'static, Result<()>> {
    Box::pin(async move {
        let job = serde_json::from_value::<J>(payload).map_err(Error::internal)?;
        job.handle(context).await
    })
}

async fn execute_handler(handler: JobFn, payload: Value, context: JobContext) -> Result<()> {
    let handler_context = context.clone();
    let handler_future = Box::pin(async move { handler(payload, handler_context).await });
    run_scoped(handler_future, &context).await
}

async fn run_scoped<'a>(
    handler_future: BoxFuture<'a, Result<()>>,
    context: &'a JobContext,
) -> Result<()> {
    if let Some(scope) = &context.attempt_scope {
        return scope.run(run_scoped_inner(handler_future, context)).await;
    }
    run_scoped_inner(handler_future, context).await
}

async fn run_scoped_inner<'a>(
    handler_future: BoxFuture<'a, Result<()>>,
    context: &'a JobContext,
) -> Result<()> {
    let execution = ExecutionContext::new(
        context.metadata.clone(),
        context.deadline,
        context.cancelled.clone(),
    );
    let i18n = match context.app_state.as_ref() {
        Some(state)
            if state
                .container()
                .contains::<crate::translation::Translator>() =>
        {
            Some(state.resolve::<crate::translation::Translator>().await?)
        }
        _ => None,
    };
    let task_future: BoxFuture<'a, Result<()>> = if let Some(i18n) = i18n {
        let locale = context
            .metadata
            .locale()
            .cloned()
            .unwrap_or_else(|| i18n.select(None));
        Box::pin(async move { i18n.with_locale(locale, handler_future).await })
    } else {
        handler_future
    };
    let tenant = context.metadata.tenant_id().cloned();
    let future = execution.run(task_future);
    if let Some(tenant) = tenant {
        crate::tenant::TenantContext::scope(tenant, future).await??
    } else {
        future.await?
    }
}

impl DatabaseQueue {
    /// 使用提供的数据库服务创建派发器。
    pub fn new(db: Db) -> Self {
        Self {
            db,
            cancelled: tokio::sync::watch::channel(false).0,
            registry: Arc::new(JobRegistry::default()),
            config: crate::config::QueueConfig::default(),
        }
    }

    fn configured(db: Db, registry: JobRegistry, config: crate::config::QueueConfig) -> Self {
        Self {
            db,
            cancelled: tokio::sync::watch::channel(false).0,
            registry: Arc::new(registry),
            config,
        }
    }

    async fn dispatch_value(
        &self,
        queue: &str,
        job_name: &str,
        payload: Value,
        options: DispatchOptions,
    ) -> Result<String> {
        validate_options(&options)?;
        let connection = self.db.connection()?;
        let transaction = connection.begin().await.map_err(Error::internal)?;
        let id =
            insert_or_get_job(&self.db, &transaction, queue, job_name, &payload, &options).await?;
        transaction.commit().await.map_err(Error::internal)?;
        Ok(id)
    }

    async fn dispatch_value_with_metadata(
        &self,
        queue: &str,
        job_name: &str,
        payload: Value,
        options: DispatchOptions,
        metadata: &ExecutionMetadata,
    ) -> Result<String> {
        let payload = persist_execution_metadata(payload, metadata)?;
        self.dispatch_value(queue, job_name, payload, options).await
    }

    #[allow(clippy::too_many_lines)] // Claim, cancellation, lease renewal and completion form one token-bound attempt.
    async fn work_once(
        &self,
        queue: &str,
        app_state: Option<crate::foundation::ApplicationState>,
    ) -> Result<bool> {
        validate_name("queue", queue, MAX_QUEUE_NAME)?;
        if self.is_shutdown_requested() {
            return Ok(false);
        }
        let Some(claim) = claim_job(&self.db, queue, self.config.lease_secs).await? else {
            return Ok(false);
        };
        if self.is_shutdown_requested() {
            fail_job(&self.db, &claim, "queue_job_cancelled", 1).await?;
            return Ok(true);
        }
        let (job_payload, metadata) = restore_execution_metadata(claim.payload.clone())?;
        let is_group = claim.job_name == CHAIN_JOB_NAME || claim.job_name == BATCH_JOB_NAME;
        let handler = self.registry.handler(claim.job_name.as_str());
        if !is_group && handler.is_none() {
            fail_job(&self.db, &claim, "unregistered_job_type", 1).await?;
            return Err(Error::custom(
                "unregistered_queue_job",
                format!("no handler is registered for job `{}`", claim.job_name),
            ));
        }
        let job_cancellation = Cancellation::new();
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.config.job_timeout_secs);
        let progress_db = self.db.clone();
        let progress_id = claim.id.clone();
        let progress_owner = claim.owner.clone();
        let progress_token = claim.token.clone();
        let mut context = JobContext {
            id: claim.id.clone(),
            attempt: claim.attempts,
            max_attempts: claim.max_attempts,
            cancelled: job_cancellation.clone(),
            app_state: app_state.clone(),
            attempt_scope: None,
            progress_reporter: Some(Arc::new(move |value| {
                Box::pin(persist_database_progress(
                    progress_db.clone(),
                    progress_id.clone(),
                    progress_owner.clone(),
                    progress_token.clone(),
                    value,
                ))
            })),
            metadata: metadata.clone(),
            deadline: Some(deadline),
        };
        let attempt = AttemptScope::create(&mut context)?;
        let scope = context.attempt_scope.clone();
        let mut cancellation = self.cancelled.subscribe();
        let registry = Arc::clone(&self.registry);
        let handler_future: BoxFuture<'static, Result<()>> = Box::pin(async move {
            if is_group {
                let group =
                    serde_json::from_value::<TaskGroup>(job_payload).map_err(Error::internal)?;
                run_task_group(registry, group, context).await
            } else if let Some(handler) = handler {
                handler(job_payload, context).await
            } else {
                Err(Error::custom(
                    "unregistered_queue_job",
                    "claimed task has no registered handler",
                ))
            }
        });
        let execution =
            ExecutionContext::new(metadata.clone(), Some(deadline), job_cancellation.clone());
        let locale_metadata = metadata.clone();
        let locale_scope = scope.clone();
        let task_future: BoxFuture<'static, Result<()>> = Box::pin(async move {
            let i18n = match app_state.as_ref() {
                Some(state)
                    if state
                        .container()
                        .contains::<crate::translation::Translator>() =>
                {
                    Some(match &locale_scope {
                        Some(scope) => scope.resolve::<crate::translation::Translator>().await?,
                        None => state.resolve::<crate::translation::Translator>().await?,
                    })
                }
                _ => None,
            };
            if let Some(i18n) = i18n {
                let locale = locale_metadata
                    .locale()
                    .cloned()
                    .unwrap_or_else(|| i18n.select(None));
                i18n.with_locale(locale, handler_future).await
            } else {
                handler_future.await
            }
        });
        let tenant_future: BoxFuture<'static, Result<()>> =
            if let Some(tenant) = metadata.tenant_id().cloned() {
                Box::pin(async move {
                    crate::tenant::TenantContext::scope(tenant, task_future).await??;
                    Ok(())
                })
            } else {
                task_future
            };
        let span = tracing::info_span!("queue.job.attempt", attempt = claim.attempts,);
        #[cfg(feature = "otel")]
        crate::core::otel::set_remote_parent(&span, metadata.traceparent());
        let tenant_future = tracing::Instrument::instrument(tenant_future, span);
        let handler_future: BoxFuture<'static, Result<()>> = match scope {
            Some(scope) => Box::pin(async move {
                scope
                    .run(with_execution_context(execution, tenant_future))
                    .await
            }),
            None => Box::pin(with_execution_context(execution, tenant_future)),
        };
        let mut handler_future = handler_future;
        #[cfg(feature = "metrics")]
        let _job_metric_guard = WorkerJobMetricGuard::new();
        #[cfg(feature = "metrics")]
        let job_started = std::time::Instant::now();
        let mut shutdown_deadline = if *cancellation.borrow() {
            job_cancellation.cancel();
            Some(
                tokio::time::Instant::now()
                    + Duration::from_secs(self.config.shutdown_timeout_secs),
            )
        } else {
            None
        };
        let heartbeat_period =
            Duration::from_secs(std::cmp::Ord::max(self.config.lease_secs / 3, 1));
        let mut heartbeat = tokio::time::interval_at(
            tokio::time::Instant::now() + heartbeat_period,
            heartbeat_period,
        );
        let timeout = tokio::time::sleep(Duration::from_secs(self.config.job_timeout_secs));
        tokio::pin!(timeout);
        let result = loop {
            tokio::select! {
                result = &mut handler_future => break Some(result),
                _ = heartbeat.tick() => {
                    match renew_job(&self.db, &claim, self.config.lease_secs).await {
                        Ok(true) => {},
                        Ok(false) => { job_cancellation.cancel(); break Some(Err(Error::custom("queue_lease_lost", "job lease ownership changed"))); },
                        Err(error) => { job_cancellation.cancel(); break Some(Err(error)); },
                    }
                }
                changed = cancellation.changed() => {
                    if changed.is_ok() && *cancellation.borrow() && shutdown_deadline.is_none() {
                        job_cancellation.cancel();
                        shutdown_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(self.config.shutdown_timeout_secs));
                    }
                }
                () = async {
                    if let Some(deadline) = shutdown_deadline {
                        tokio::time::sleep_until(deadline).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    break Some(Err(Error::custom("queue_shutdown_timeout", "job did not stop before the worker shutdown deadline")));
                }
                () = &mut timeout => {
                    job_cancellation.cancel();
                    break None;
                }
            }
        };
        drop(handler_future);
        let result = if let Some(result) = result {
            Some(finish_attempt(attempt, result).await)
        } else {
            let timeout = Error::custom("job_timeout", "job attempt timed out");
            let _ = finish_attempt(attempt, Err(timeout)).await;
            None
        };
        #[cfg(feature = "metrics")]
        {
            let outcome = match &result {
                Some(Ok(())) => "succeeded",
                Some(Err(_)) => "failed",
                None => "timed_out",
            };
            metrics::counter!("openrust_worker_jobs_completed_total", "outcome" => outcome)
                .increment(1);
            metrics::histogram!("openrust_worker_job_duration_seconds", "outcome" => outcome)
                .record(job_started.elapsed().as_secs_f64());
        }
        match result {
            Some(Ok(())) => acknowledge_job(&self.db, &claim).await?,
            Some(Err(error)) => {
                let shift = <u32 as TryFrom<u64>>::try_from(claim.attempts.saturating_sub(1))
                    .unwrap_or(u32::MAX);
                let delay = std::cmp::Ord::min(1_u64.checked_shl(shift).unwrap_or(300), 300);
                fail_job(&self.db, &claim, safe_error_code(error.code()), delay).await?;
                if error.code() == "queue_shutdown_timeout" {
                    return Err(error);
                }
            }
            None => fail_job(&self.db, &claim, "job_timeout", 1).await?,
        }
        Ok(true)
    }

    /// 在调用方的业务事务中插入任务，使回滚同时撤销两项变更。
    async fn dispatch_value_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        job_name: &str,
        payload: Value,
        options: DispatchOptions,
    ) -> Result<String> {
        validate_options(&options)?;
        insert_or_get_job(&self.db, transaction, queue, job_name, &payload, &options).await
    }

    async fn dispatch_value_in_transaction_with_metadata(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        job_name: &str,
        payload: Value,
        options: DispatchOptions,
        metadata: &ExecutionMetadata,
    ) -> Result<String> {
        let payload = persist_execution_metadata(payload, metadata)?;
        self.dispatch_value_in_transaction(transaction, queue, job_name, payload, options)
            .await
    }

    /// 使用新 ID 将失败任务重新入队，同时保留原失败记录。
    async fn retry_failed_inner(&self, failed_id: &str) -> Result<String> {
        if failed_id.len() != 32 || !failed_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::custom(
                "invalid_failed_job_id",
                "failed job ID must be 32 hexadecimal characters",
            ));
        }
        let connection = self.db.connection()?;
        let backend = self.db.backend()?;
        let failed = self.db.quoted_table_name("failed_jobs")?;
        let transaction = connection.begin().await.map_err(Error::internal)?;
        let select = match backend {
            DbBackend::Postgres => format!(
                "SELECT queue, job_name, payload, retried_job_id FROM {failed} WHERE id = $1 FOR UPDATE"
            ),
            DbBackend::MySql => format!(
                "SELECT queue, job_name, payload, retried_job_id FROM {failed} WHERE id = ? FOR UPDATE"
            ),
            _ => {
                return Err(Error::custom(
                    "unsupported_queue_backend",
                    "database queue requires PostgreSQL or MySQL",
                ));
            }
        };
        let row = transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                select,
                vec![failed_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("failed_job_not_found", "failed job does not exist"))?;
        let retried_job_id: Option<String> =
            row.try_get("", "retried_job_id").map_err(Error::internal)?;
        if let Some(id) = retried_job_id {
            transaction.commit().await.map_err(Error::internal)?;
            return Ok(id);
        }
        let queue: String = row.try_get("", "queue").map_err(Error::internal)?;
        let job_name: String = row.try_get("", "job_name").map_err(Error::internal)?;
        let payload: Value = row.try_get("", "payload").map_err(Error::internal)?;
        let id = new_id()?;
        insert_job(
            &self.db,
            &transaction,
            &id,
            &queue,
            &job_name,
            &payload,
            &DispatchOptions::default(),
        )
        .await?;
        let update = match backend {
            DbBackend::Postgres => format!("UPDATE {failed} SET retried_job_id = $1 WHERE id = $2"),
            DbBackend::MySql => format!("UPDATE {failed} SET retried_job_id = ? WHERE id = ?"),
            _ => {
                return Err(Error::custom(
                    "unsupported_queue_backend",
                    "database queue requires PostgreSQL or MySQL",
                ));
            }
        };
        transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                update,
                vec![id.clone().into(), failed_id.into()],
            ))
            .await
            .map_err(Error::internal)?;
        transaction.commit().await.map_err(Error::internal)?;
        Ok(id)
    }
}

impl Queue {
    fn connection_config(
        config: &crate::config::QueueConfig,
        name: &str,
    ) -> Result<crate::config::QueueConnectionConfig> {
        let raw = config.connections.get(name).ok_or_else(|| {
            Error::custom(
                "queue_connection_missing",
                "queue connection is not configured",
            )
        })?;
        let connection: crate::config::QueueConnectionConfig = serde_json::from_value(raw.clone())
            .map_err(|_| {
                Error::custom(
                    "queue_invalid_connection_config",
                    "queue connection configuration is invalid",
                )
            })?;
        validate_name("queue", &connection.queue, MAX_QUEUE_NAME)?;
        Ok(connection)
    }

    /// 校验全局 worker 参数、默认连接名称和调度引用，不解析未选连接或运行 validator。
    pub fn validate_configured(&self) -> Result<()> {
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| Error::custom("queue_manager_missing", "queue manager unavailable"))?;
        let config = manager
            .resolver
            .section::<crate::config::QueueConfig>("queue")?;
        validate_config(&config)?;
        validate_name("queue connection", &config.default, MAX_QUEUE_NAME)?;
        if !config.connections.contains_key(&config.default) {
            return Err(Error::custom(
                "queue_connection_missing",
                "default queue connection is not configured",
            ));
        }
        let schedules = {
            let inner = manager
                .contributions
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.plugin.schedules.clone()
        };
        for schedule in schedules {
            if !manager.contributions.jobs.contains(schedule.job_name()) {
                return Err(Error::custom(
                    "unregistered_schedule_job",
                    "schedule references an unregistered job",
                ));
            }
        }
        Ok(())
    }

    /// 按配置名称异步构造并缓存连接；同名并发请求共享一次构造。
    ///
    /// manager 停机后拒绝解析所有名称，包括尚未构造的连接。worker 显式重启时开启
    /// 新代缓存；旧代在途构造即使随后完成，也只会被停止而不会发布到新代。
    #[allow(clippy::too_many_lines, reason = "连接创建与停机竞态需在同一流程处理")]
    pub async fn connection(&self, name: &str) -> Result<Queue> {
        use std::sync::atomic::Ordering;
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| Error::custom("queue_manager_missing", "queue manager unavailable"))?;
        if manager.shutdown.load(Ordering::SeqCst) {
            return Err(queue_stopped());
        }
        let generation = manager.generation.load(Ordering::SeqCst);
        validate_name("queue connection", name, MAX_QUEUE_NAME)?;
        let cell = manager
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default()
            .clone();
        let driver = cell
            .get_or_try_init(|| async {
                if manager.shutdown.load(Ordering::SeqCst)
                    || manager.generation.load(Ordering::SeqCst) != generation
                {
                    return Err(queue_stopped());
                }
                let config = manager
                    .resolver
                    .section::<crate::config::QueueConfig>("queue")?;
                validate_config(&config)?;
                let connection = Self::connection_config(&config, name)?;
                let driver_name = connection.driver.clone();
                let (creator, registry) = {
                    let inner = manager
                        .contributions
                        .inner
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    (
                        inner.creators.get(&driver_name).cloned(),
                        manager.contributions.jobs.clone(),
                    )
                };
                let driver = if let Some(entry) = creator {
                    if let Some(validator) = &entry.validator {
                        validator(name, &connection).map_err(|_| {
                            Error::custom(
                                "queue_invalid_driver_config",
                                "queue driver configuration is invalid",
                            )
                        })?;
                    }
                    (entry.creator)(
                        manager.resolver.clone(),
                        name.to_owned(),
                        connection.clone(),
                        config,
                        registry,
                    )
                    .await?
                } else {
                    if !serde_json::Value::is_null(&connection.options)
                        && !connection
                            .options
                            .as_object()
                            .is_some_and(serde_json::Map::is_empty)
                    {
                        return Err(Error::custom(
                            "queue_invalid_options",
                            "built-in queue driver does not accept options",
                        ));
                    }
                    match driver_name.as_str() {
                        "database" => {
                            let db = manager.resolver.resolve::<Db>().await?;
                            if !matches!(db.backend()?, DbBackend::Postgres | DbBackend::MySql) {
                                return Err(Error::custom(
                                    "unsupported_queue_backend",
                                    "database queue requires PostgreSQL or MySQL",
                                ));
                            }
                            Ok(
                                Arc::new(DatabaseQueue::configured((*db).clone(), registry, config))
                                    as Arc<dyn QueueDriver>,
                            )
                        }
                        "redis" => {
                            #[cfg(feature = "queue-redis")]
                            {
                                let url = connection.redis_url.as_deref().ok_or_else(|| {
                                    Error::custom(
                                        "queue_redis_url_required",
                                        "queue Redis URL is required",
                                    )
                                })?;
                                let namespace =
                                    connection.redis_namespace.as_deref().ok_or_else(|| {
                                        Error::custom(
                                            "queue_redis_namespace_required",
                                            "queue Redis namespace is required",
                                        )
                                    })?;
                                Ok(Arc::new(RedisQueueDriver::new(
                                    url, namespace, registry, config,
                                )?) as Arc<dyn QueueDriver>)
                            }
                            #[cfg(not(feature = "queue-redis"))]
                            {
                                Err(Error::custom(
                                    "queue_redis_feature_disabled",
                                    "enable queue-redis",
                                ))
                            }
                        }
                        _ => Err(Error::custom(
                            "queue_driver_missing",
                            "queue driver is not registered",
                        )),
                    }?
                };
                if manager.shutdown.load(Ordering::SeqCst)
                    || manager.generation.load(Ordering::SeqCst) != generation
                {
                    driver.request_shutdown();
                    return Err(queue_stopped());
                }
                Ok(driver)
            })
            .await?;
        if manager.shutdown.load(Ordering::SeqCst)
            || manager.generation.load(Ordering::SeqCst) != generation
        {
            driver.request_shutdown();
            return Err(queue_stopped());
        }
        Ok(Queue {
            driver: Some(Arc::clone(driver)),
            manager: None,
        })
    }

    async fn resolve_driver(&self) -> Result<Arc<dyn QueueDriver>> {
        if let Some(driver) = &self.driver {
            return Ok(Arc::clone(driver));
        }
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| Error::custom("queue_manager_missing", "queue manager unavailable"))?;
        let config = manager
            .resolver
            .section::<crate::config::QueueConfig>("queue")?;
        self.connection(&config.default)
            .await?
            .driver
            .ok_or_else(|| Error::custom("queue_driver_missing", "queue driver unavailable"))
    }

    fn ready_driver(&self) -> Option<Arc<dyn QueueDriver>> {
        if let Some(driver) = &self.driver {
            return Some(Arc::clone(driver));
        }
        let manager = self.manager.as_ref()?;
        let config = manager
            .resolver
            .section::<crate::config::QueueConfig>("queue")
            .ok()?;
        manager
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&config.default)
            .and_then(|cell| cell.get())
            .cloned()
    }

    /// 使用应用共享数据库服务创建数据库派发器。
    pub fn new(db: Db) -> Self {
        let config = crate::config::QueueConfig::default();
        Self {
            driver: Some(Arc::new(DatabaseQueue::configured(
                db,
                JobRegistry::default(),
                config,
            ))),
            manager: None,
        }
    }

    /// 使用应用提供的队列 driver 创建派发器。
    pub fn with_driver(
        driver: impl QueueDriver,
        config: &crate::config::QueueConfig,
    ) -> Result<Self> {
        validate_config(config)?;
        Ok(Self {
            driver: Some(Arc::new(driver)),
            manager: None,
        })
    }

    /// 插入一个在事务提交后立即可用的任务。
    pub async fn dispatch<T: Serialize>(
        &self,
        queue: &str,
        job_name: &str,
        payload: &T,
    ) -> Result<String> {
        self.dispatch_with_options(queue, job_name, payload, DispatchOptions::default())
            .await
    }

    /// 插入一个具有显式有界初始延迟和重试上限的任务。
    pub async fn dispatch_with_options<T: Serialize>(
        &self,
        queue: &str,
        job_name: &str,
        payload: &T,
        options: DispatchOptions,
    ) -> Result<String> {
        validate_options(&options)?;
        self.resolve_driver()
            .await?
            .dispatch_with_metadata(
                queue,
                job_name,
                serde_json::to_value(payload).map_err(Error::internal)?,
                options,
                current_execution_metadata()?,
            )
            .await
    }

    /// 为队列范围内的唯一键派发一个任务；重复调用时返回活动任务 ID。
    ///
    /// 任务处于等待或运行状态时，此键会阻止重叠的重复任务。任务成功或移入 `failed_jobs` 后会释放此键。
    pub async fn dispatch_unique<T: Serialize>(
        &self,
        queue: &str,
        job_name: &str,
        unique_key: &str,
        payload: &T,
    ) -> Result<String> {
        let options = DispatchOptions {
            unique_key: Some(unique_key.to_owned()),
            ..DispatchOptions::default()
        };
        self.dispatch_with_options(queue, job_name, payload, options)
            .await
    }

    /// 在调用方的业务事务中插入任务，使回滚同时撤销两项变更。
    pub async fn dispatch_in_transaction<T: Serialize>(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        job_name: &str,
        payload: &T,
    ) -> Result<String> {
        self.resolve_driver()
            .await?
            .dispatch_in_transaction_with_metadata(
                transaction,
                queue,
                job_name,
                serde_json::to_value(payload).map_err(Error::internal)?,
                DispatchOptions::default(),
                current_execution_metadata()?,
            )
            .await
    }

    /// 在调用方事务中唯一派发任务。
    pub async fn dispatch_unique_in_transaction<T: Serialize>(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        job_name: &str,
        unique_key: &str,
        payload: &T,
    ) -> Result<String> {
        let options = DispatchOptions {
            unique_key: Some(unique_key.to_owned()),
            ..DispatchOptions::default()
        };
        validate_options(&options)?;
        self.resolve_driver()
            .await?
            .dispatch_in_transaction_with_metadata(
                transaction,
                queue,
                job_name,
                serde_json::to_value(payload).map_err(Error::internal)?,
                options,
                current_execution_metadata()?,
            )
            .await
    }

    /// 将有界有序任务链作为一个持久化父任务派发。
    pub async fn dispatch_chain(
        &self,
        queue: &str,
        tasks: Vec<QueueTask>,
        failure_policy: GroupFailurePolicy,
    ) -> Result<String> {
        self.dispatch_task_group(
            queue,
            TaskGroup::Chain {
                tasks,
                failure_policy,
            },
        )
        .await
    }

    /// 将有界并发任务批次作为一个持久化父任务派发。
    pub async fn dispatch_batch(
        &self,
        queue: &str,
        tasks: Vec<QueueTask>,
        concurrency: usize,
        failure_policy: GroupFailurePolicy,
    ) -> Result<String> {
        self.dispatch_task_group(
            queue,
            TaskGroup::Batch {
                tasks,
                concurrency,
                failure_policy,
            },
        )
        .await
    }

    /// 在调用方数据库事务中派发有序任务链。
    pub async fn dispatch_chain_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        tasks: Vec<QueueTask>,
        failure_policy: GroupFailurePolicy,
    ) -> Result<String> {
        self.dispatch_task_group_in_transaction(
            transaction,
            queue,
            TaskGroup::Chain {
                tasks,
                failure_policy,
            },
        )
        .await
    }

    /// 在调用方数据库事务中派发有界并发任务批次。
    pub async fn dispatch_batch_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        tasks: Vec<QueueTask>,
        concurrency: usize,
        failure_policy: GroupFailurePolicy,
    ) -> Result<String> {
        self.dispatch_task_group_in_transaction(
            transaction,
            queue,
            TaskGroup::Batch {
                tasks,
                concurrency,
                failure_policy,
            },
        )
        .await
    }

    async fn dispatch_task_group(&self, queue: &str, group: TaskGroup) -> Result<String> {
        group.validate()?;
        self.resolve_driver()
            .await?
            .dispatch_group_with_metadata(
                queue,
                group,
                DispatchOptions::default(),
                current_execution_metadata()?,
            )
            .await
    }

    async fn dispatch_task_group_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        group: TaskGroup,
    ) -> Result<String> {
        group.validate()?;
        self.resolve_driver()
            .await?
            .dispatch_group_in_transaction_with_metadata(
                transaction,
                queue,
                group,
                DispatchOptions::default(),
                current_execution_metadata()?,
            )
            .await
    }

    /// 领取并运行指定队列中的一个就绪任务，并返回是否成功领取。
    pub async fn run_next(&self, queue: &str) -> Result<bool> {
        match self.resolve_driver().await {
            Ok(driver) => driver.run_next(queue).await,
            Err(error) if error.code() == "queue_shutdown_requested" => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// 通过应用 worker context 领取并运行一个任务。
    pub async fn run_next_with_state(
        &self,
        queue: &str,
        state: &crate::foundation::ApplicationState,
    ) -> Result<bool> {
        match self.resolve_driver().await {
            Ok(driver) => driver.run_next_with_state(queue, state).await,
            Err(error) if error.code() == "queue_shutdown_requested" => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// 将失败任务重新入队，并返回新任务的稳定 ID。
    pub async fn retry_failed(&self, failed_id: &str) -> Result<String> {
        self.resolve_driver().await?.retry_failed(failed_id).await
    }

    fn reset_shutdown(&self) {
        if let Some(manager) = &self.manager {
            manager
                .shutdown
                .store(false, std::sync::atomic::Ordering::SeqCst);
        } else if let Some(driver) = &self.driver {
            driver.reset_shutdown();
        }
    }
    fn request_shutdown(&self) {
        if let Some(manager) = &self.manager {
            // 先封整个 manager，再脱离旧代缓存。旧代在途构造不得发布至新代。
            manager
                .shutdown
                .store(true, std::sync::atomic::Ordering::SeqCst);
            manager
                .generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let cells = std::mem::take(
                &mut *manager
                    .connections
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            for cell in cells.values() {
                if let Some(driver) = cell.get() {
                    driver.request_shutdown();
                }
            }
        } else if let Some(driver) = &self.driver {
            driver.request_shutdown();
        }
    }
    fn is_shutdown_requested(&self) -> bool {
        self.manager
            .as_ref()
            .is_some_and(|manager| manager.shutdown.load(std::sync::atomic::Ordering::SeqCst))
            || self
                .ready_driver()
                .is_some_and(|driver| driver.is_shutdown_requested())
    }
}

impl QueueDriver for DatabaseQueue {
    fn dispatch<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { self.dispatch_value(queue, job_name, payload, options).await })
    }

    fn dispatch_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.dispatch_value_in_transaction(transaction, queue, job_name, payload, options)
                .await
        })
    }

    fn dispatch_with_metadata<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.dispatch_value_with_metadata(queue, job_name, payload, options, &metadata)
                .await
        })
    }

    fn dispatch_in_transaction_with_metadata<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.dispatch_value_in_transaction_with_metadata(
                transaction,
                queue,
                job_name,
                payload,
                options,
                &metadata,
            )
            .await
        })
    }

    fn dispatch_group<'a>(
        &'a self,
        queue: &'a str,
        group: TaskGroup,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            group.validate()?;
            let job_name = group.persisted_job_name();
            self.dispatch_value(
                queue,
                job_name,
                serde_json::to_value(group).map_err(Error::internal)?,
                options,
            )
            .await
        })
    }

    fn dispatch_group_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        group: TaskGroup,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            group.validate()?;
            let job_name = group.persisted_job_name();
            self.dispatch_value_in_transaction(
                transaction,
                queue,
                job_name,
                serde_json::to_value(group).map_err(Error::internal)?,
                options,
            )
            .await
        })
    }

    fn dispatch_group_with_metadata<'a>(
        &'a self,
        queue: &'a str,
        group: TaskGroup,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            group.validate()?;
            let job_name = group.persisted_job_name();
            self.dispatch_value_with_metadata(
                queue,
                job_name,
                serde_json::to_value(group).map_err(Error::internal)?,
                options,
                &metadata,
            )
            .await
        })
    }

    fn dispatch_group_in_transaction_with_metadata<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        group: TaskGroup,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            group.validate()?;
            let job_name = group.persisted_job_name();
            self.dispatch_value_in_transaction_with_metadata(
                transaction,
                queue,
                job_name,
                serde_json::to_value(group).map_err(Error::internal)?,
                options,
                &metadata,
            )
            .await
        })
    }

    fn run_next<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { self.work_once(queue, None).await })
    }

    fn run_next_with_state<'a>(
        &'a self,
        queue: &'a str,
        state: &'a crate::foundation::ApplicationState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { self.work_once(queue, Some(state.clone())).await })
    }

    fn retry_failed<'a>(&'a self, failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { self.retry_failed_inner(failed_id).await })
    }

    fn reset_shutdown(&self) {
        self.cancelled.send_replace(false);
    }
    fn request_shutdown(&self) {
        self.cancelled.send_replace(true);
    }
    fn is_shutdown_requested(&self) -> bool {
        *self.cancelled.borrow()
    }
}

async fn run_task_group(
    registry: Arc<JobRegistry>,
    group: TaskGroup,
    context: JobContext,
) -> Result<()> {
    group.validate()?;
    match group {
        TaskGroup::Chain {
            tasks,
            failure_policy,
        } => {
            let total = tasks.len();
            let mut completed = 0;
            let mut failed = 0;
            let mut first_error = None;
            for task in tasks {
                let result = run_group_task(Arc::clone(&registry), task, context.clone()).await;
                completed += 1;
                if result.is_err() {
                    failed += 1;
                }
                context
                    .set_progress(&serde_json::json!({"completed": completed, "failed": failed, "total": total}))
                    .await?;
                if let Err(error) = result {
                    if matches!(failure_policy, GroupFailurePolicy::Stop) {
                        return Err(error);
                    }
                    first_error.get_or_insert(error);
                }
            }
            first_error.map_or(Ok(()), Err)
        }
        TaskGroup::Batch {
            tasks,
            concurrency,
            failure_policy,
        } => {
            let total = tasks.len();
            let completed = Arc::new(tokio::sync::Mutex::new((0_usize, 0_usize)));
            let mut pending = tasks.into_iter();
            let mut workers = tokio::task::JoinSet::new();
            for _ in 0..concurrency {
                spawn_group_task(
                    &mut workers,
                    pending.next(),
                    Arc::clone(&registry),
                    context.clone(),
                    Arc::clone(&completed),
                    total,
                );
            }
            let mut first_error = None;
            while let Some(result) = workers.join_next().await {
                let result = result.map_err(Error::internal)?;
                if let Err(error) = result {
                    if matches!(failure_policy, GroupFailurePolicy::Stop) {
                        workers.abort_all();
                        while workers.join_next().await.is_some() {}
                        return Err(error);
                    }
                    first_error.get_or_insert(error);
                }
                if first_error.is_none() || matches!(failure_policy, GroupFailurePolicy::Continue) {
                    spawn_group_task(
                        &mut workers,
                        pending.next(),
                        Arc::clone(&registry),
                        context.clone(),
                        Arc::clone(&completed),
                        total,
                    );
                }
            }
            first_error.map_or(Ok(()), Err)
        }
    }
}

fn spawn_group_task(
    workers: &mut tokio::task::JoinSet<Result<()>>,
    task: Option<QueueTask>,
    registry: Arc<JobRegistry>,
    context: JobContext,
    completed: Arc<tokio::sync::Mutex<(usize, usize)>>,
    total: usize,
) {
    if let Some(task) = task {
        workers.spawn(async move {
            let result = run_group_task(registry, task, context.clone()).await;
            let mut counts = completed.lock().await;
            counts.0 += 1;
            if result.is_err() {
                counts.1 += 1;
            }
            context
                .set_progress(
                    &serde_json::json!({"completed": counts.0, "failed": counts.1, "total": total}),
                )
                .await?;
            result
        });
    }
}

async fn run_group_task(
    registry: Arc<JobRegistry>,
    task: QueueTask,
    context: JobContext,
) -> Result<()> {
    let Some(handler) = registry.handler(task.job_name.as_str()) else {
        return Err(Error::custom(
            "unregistered_queue_job",
            format!("no handler is registered for job `{}`", task.job_name),
        ));
    };
    execute_handler(handler, task.payload, context).await
}

fn queue_work_options(
    config: &crate::config::QueueConfig,
    args: &[String],
) -> Result<(String, String)> {
    let mut connection = None;
    let mut queue = None;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--queue" {
            if queue.is_some() {
                return Err(invalid_queue_work_command());
            }
            let value = arguments.next().ok_or_else(invalid_queue_work_command)?;
            if value.starts_with('-') {
                return Err(invalid_queue_work_command());
            }
            queue = Some(value.clone());
        } else if let Some(value) = argument.strip_prefix("--queue=") {
            if queue.is_some() {
                return Err(invalid_queue_work_command());
            }
            queue = Some(value.to_owned());
        } else if argument.starts_with('-') || connection.is_some() {
            return Err(invalid_queue_work_command());
        } else {
            connection = Some(argument.clone());
        }
    }
    let connection = connection.unwrap_or_else(|| config.default.clone());
    validate_name("queue connection", &connection, MAX_QUEUE_NAME)?;
    let selected = Queue::connection_config(config, &connection)?;
    let queue = queue.unwrap_or(selected.queue);
    validate_name("queue", &queue, MAX_QUEUE_NAME)?;
    Ok((connection, queue))
}

fn invalid_queue_work_command() -> Error {
    Error::custom(
        "invalid_queue_command",
        "usage: queue work [connection] [--queue <queue>]",
    )
}

struct QueueWorkCommand;

impl crate::console::Command for QueueWorkCommand {
    fn name(&self) -> &'static str {
        "queue work"
    }

    fn about(&self) -> &'static str {
        "运行所选连接的队列 worker"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("[connection] [--queue <queue>]")
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(
        &'a self,
        app: &'a crate::foundation::Application,
        args: Vec<String>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<crate::config::QueueConfig>("queue")?;
            let (connection_name, queue_name) = queue_work_options(&config, &args)?;
            let manager = app.state().resolve::<Queue>().await?;
            manager.reset_shutdown();
            let queue = manager.connection(&connection_name).await?;
            let app_state = app.state().clone();
            queue.reset_shutdown();
            let mut workers = tokio::task::JoinSet::new();
            for _ in 0..config.worker_concurrency {
                let queue = queue.clone();
                let app_state = app_state.clone();
                let queue_name = queue_name.clone();
                let poll_interval = config.poll_interval_millis;
                workers.spawn(async move {
                    worker_loop(queue, app_state, queue_name, poll_interval).await
                });
            }
            let cause = tokio::select! {
                    signal = shutdown_signal() => signal.err(),
                    result = workers.join_next() => {
                        Some(match result {
                            Some(Ok(Ok(()))) => Error::custom("queue_worker_stopped", "queue worker exited unexpectedly"),
                            Some(Ok(Err(error))) => error,
                            Some(Err(error)) => Error::internal(error),
                            None => Error::custom("queue_worker_stopped", "all queue workers exited unexpectedly"),
                        })
                    }
            };
            manager.request_shutdown();
            let drain = join_workers(&mut workers);
            let grace = Duration::from_secs(config.shutdown_timeout_secs.saturating_add(2));
            let drain_result = if let Ok(result) = tokio::time::timeout(grace, drain).await {
                result
            } else {
                workers.abort_all();
                while workers.join_next().await.is_some() {}
                Err(Error::custom(
                    "queue_shutdown_timeout",
                    "queue workers did not join before the supervisor deadline",
                ))
            };
            if let Some(error) = cause {
                return Err(error);
            }
            drain_result
        })
    }
}

async fn worker_loop(
    queue: Queue,
    app_state: crate::foundation::ApplicationState,
    queue_name: String,
    poll_interval_millis: u64,
) -> Result<()> {
    loop {
        if queue.is_shutdown_requested() {
            return Ok(());
        }
        if !queue.run_next_with_state(&queue_name, &app_state).await? {
            tokio::time::sleep(Duration::from_millis(poll_interval_millis)).await;
        }
    }
}

async fn join_workers(workers: &mut tokio::task::JoinSet<Result<()>>) -> Result<()> {
    let mut first_error = None;
    while let Some(result) = workers.join_next().await {
        let error = match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(error) => Some(Error::internal(error)),
        };
        if first_error.is_none() {
            first_error = error;
        }
    }
    first_error.map_or(Ok(()), Err)
}

struct QueueRetryCommand;

impl crate::console::Command for QueueRetryCommand {
    fn name(&self) -> &'static str {
        "queue retry"
    }
    fn about(&self) -> &'static str {
        "Retry a failed database job"
    }
    fn usage(&self) -> Option<&'static str> {
        Some("<failed-job-id>")
    }

    fn accepts_args(&self) -> bool {
        true
    }
    fn run<'a>(
        &'a self,
        app: &'a crate::foundation::Application,
        args: Vec<String>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let [failed_id] = args.as_slice() else {
                return Err(Error::custom(
                    "invalid_queue_command",
                    "usage: queue retry <failed-job-id>",
                ));
            };
            let queue = app.state().resolve::<Queue>().await?;
            let id = queue.retry_failed(failed_id).await?;
            print_requeued(&id);
            Ok(())
        })
    }
}

fn validate_config(config: &crate::config::QueueConfig) -> Result<()> {
    if !(1..=3600).contains(&config.lease_secs)
        || !(1..=86_400).contains(&config.job_timeout_secs)
        || !(10..=60_000).contains(&config.poll_interval_millis)
        || !(1..=3600).contains(&config.shutdown_timeout_secs)
        || !(1..=64).contains(&config.worker_concurrency)
    {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "queue lease/shutdown timeout must be 1..=3600 seconds, job timeout 1..=86400 seconds, poll interval 10..=60000 milliseconds, and worker_concurrency 1..=64".into(),
        )));
    }
    Ok(())
}

struct ClaimedJob {
    id: String,
    job_name: String,
    payload: Value,
    attempts: u64,
    max_attempts: u64,
    owner: String,
    token: String,
}

async fn claim_job(db: &Db, queue: &str, lease_secs: u64) -> Result<Option<ClaimedJob>> {
    let connection = db.connection()?;
    let backend = db.backend()?;
    let table = db.quoted_table_name("jobs")?;
    let transaction = connection.begin().await.map_err(Error::internal)?;
    let select = match backend {
        DbBackend::Postgres => format!(
            "SELECT id, job_name, payload, attempts, max_attempts FROM {table} WHERE queue = $1 AND available_at <= CURRENT_TIMESTAMP AND (reserved_until IS NULL OR reserved_until <= CURRENT_TIMESTAMP) ORDER BY available_at, id LIMIT 1 FOR UPDATE SKIP LOCKED"
        ),
        DbBackend::MySql => format!(
            "SELECT id, job_name, payload, attempts, max_attempts FROM {table} WHERE queue = ? AND available_at <= UTC_TIMESTAMP(6) AND (reserved_until IS NULL OR reserved_until <= UTC_TIMESTAMP(6)) ORDER BY available_at, id LIMIT 1 FOR UPDATE SKIP LOCKED"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let Some(row) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            select,
            vec![queue.into()],
        ))
        .await
        .map_err(Error::internal)?
    else {
        transaction.rollback().await.map_err(Error::internal)?;
        return Ok(None);
    };
    let id: String = row.try_get("", "id").map_err(Error::internal)?;
    let job_name: String = row.try_get("", "job_name").map_err(Error::internal)?;
    let payload: Value = row.try_get("", "payload").map_err(Error::internal)?;
    let attempts: i64 = row.try_get("", "attempts").map_err(Error::internal)?;
    let max_attempts: i64 = row.try_get("", "max_attempts").map_err(Error::internal)?;
    let owner = new_id()?;
    let token = new_id()?;
    let update = match backend {
        DbBackend::Postgres => format!(
            "UPDATE {table} SET attempts = attempts + 1, reserved_by = $1, ownership_token = $2, reserved_until = CURRENT_TIMESTAMP + INTERVAL '{lease_secs} seconds' WHERE id = $3"
        ),
        DbBackend::MySql => format!(
            "UPDATE {table} SET attempts = attempts + 1, reserved_by = ?, ownership_token = ?, reserved_until = DATE_ADD(UTC_TIMESTAMP(6), INTERVAL {lease_secs} SECOND) WHERE id = ?"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let claimed = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            update,
            vec![
                owner.clone().into(),
                token.clone().into(),
                id.clone().into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    if claimed.rows_affected() != 1 {
        transaction.rollback().await.map_err(Error::internal)?;
        return Err(Error::custom(
            "queue_claim_lost",
            "job claim changed before lease assignment",
        ));
    }
    transaction.commit().await.map_err(Error::internal)?;
    Ok(Some(ClaimedJob {
        id,
        job_name,
        payload,
        attempts: <u64 as TryFrom<i64>>::try_from(std::cmp::Ord::max(attempts, 0)).unwrap_or(0) + 1,
        max_attempts: <u64 as TryFrom<i64>>::try_from(std::cmp::Ord::max(max_attempts, 1))
            .unwrap_or(1),
        owner,
        token,
    }))
}

async fn acknowledge_job(db: &Db, job: &ClaimedJob) -> Result<()> {
    let connection = db.connection()?;
    let backend = db.backend()?;
    let table = db.quoted_table_name("jobs")?;
    let (sql, values) = match backend {
        DbBackend::Postgres => (
            format!(
                "DELETE FROM {table} WHERE id = $1 AND reserved_by = $2 AND ownership_token = $3"
            ),
            vec![
                job.id.clone().into(),
                job.owner.clone().into(),
                job.token.clone().into(),
            ],
        ),
        DbBackend::MySql => (
            format!("DELETE FROM {table} WHERE id = ? AND reserved_by = ? AND ownership_token = ?"),
            vec![
                job.id.clone().into(),
                job.owner.clone().into(),
                job.token.clone().into(),
            ],
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let deleted = connection
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    if deleted.rows_affected() != 1 {
        return Err(Error::custom(
            "queue_lease_lost",
            "job lease ownership changed before acknowledgement",
        ));
    }
    Ok(())
}

async fn renew_job(db: &Db, job: &ClaimedJob, lease_secs: u64) -> Result<bool> {
    let connection = db.connection()?;
    let backend = db.backend()?;
    let table = db.quoted_table_name("jobs")?;
    let (sql, values) = match backend {
        DbBackend::Postgres => (
            format!(
                "UPDATE {table} SET reserved_until = CURRENT_TIMESTAMP + INTERVAL '{lease_secs} seconds' WHERE id = $1 AND reserved_by = $2 AND ownership_token = $3"
            ),
            vec![
                job.id.clone().into(),
                job.owner.clone().into(),
                job.token.clone().into(),
            ],
        ),
        DbBackend::MySql => (
            format!(
                "UPDATE {table} SET reserved_until = DATE_ADD(UTC_TIMESTAMP(6), INTERVAL {lease_secs} SECOND) WHERE id = ? AND reserved_by = ? AND ownership_token = ?"
            ),
            vec![
                job.id.clone().into(),
                job.owner.clone().into(),
                job.token.clone().into(),
            ],
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let result = connection
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    Ok(result.rows_affected() == 1)
}

#[allow(clippy::too_many_lines)] // The two-step failed-job transaction must stay visibly atomic.
async fn fail_job(db: &Db, job: &ClaimedJob, error_code: &str, delay: u64) -> Result<()> {
    let connection = db.connection()?;
    let backend = db.backend()?;
    let table = db.quoted_table_name("jobs")?;
    if job.attempts >= job.max_attempts {
        let failed = db.quoted_table_name("failed_jobs")?;
        let transaction = connection.begin().await.map_err(Error::internal)?;
        let insert = match backend {
            DbBackend::Postgres => format!(
                "INSERT INTO {failed} (id, queue, job_name, payload, attempts, error, failed_at) SELECT id, queue, job_name, payload, attempts, $1, CURRENT_TIMESTAMP FROM {table} WHERE id = $2 AND ownership_token = $3"
            ),
            DbBackend::MySql => format!(
                "INSERT INTO {failed} (id, queue, job_name, payload, attempts, error, failed_at) SELECT id, queue, job_name, payload, attempts, ?, UTC_TIMESTAMP(6) FROM {table} WHERE id = ? AND ownership_token = ?"
            ),
            _ => {
                return Err(Error::custom(
                    "unsupported_queue_backend",
                    "database queue requires PostgreSQL or MySQL",
                ));
            }
        };
        let inserted = transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                insert,
                vec![
                    error_code.into(),
                    job.id.clone().into(),
                    job.token.clone().into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        if inserted.rows_affected() != 1 {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(Error::custom(
                "queue_lease_lost",
                "job lease ownership changed before failure recording",
            ));
        }
        let delete = match backend {
            DbBackend::Postgres => {
                format!("DELETE FROM {table} WHERE id = $1 AND ownership_token = $2")
            }
            DbBackend::MySql => format!("DELETE FROM {table} WHERE id = ? AND ownership_token = ?"),
            _ => {
                return Err(Error::custom(
                    "unsupported_queue_backend",
                    "database queue requires PostgreSQL or MySQL",
                ));
            }
        };
        let deleted = transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                delete,
                vec![job.id.clone().into(), job.token.clone().into()],
            ))
            .await
            .map_err(Error::internal)?;
        if deleted.rows_affected() != 1 {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(Error::custom(
                "queue_lease_lost",
                "job lease ownership changed before failure cleanup",
            ));
        }
        transaction.commit().await.map_err(Error::internal)?;
        return Ok(());
    }
    let update = match backend {
        DbBackend::Postgres => format!(
            "UPDATE {table} SET available_at = CURRENT_TIMESTAMP + ($1 * INTERVAL '1 second'), reserved_by = NULL, ownership_token = NULL, reserved_until = NULL WHERE id = $2 AND ownership_token = $3"
        ),
        DbBackend::MySql => format!(
            "UPDATE {table} SET available_at = DATE_ADD(UTC_TIMESTAMP(6), INTERVAL ? SECOND), reserved_by = NULL, ownership_token = NULL, reserved_until = NULL WHERE id = ? AND ownership_token = ?"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let updated = connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            update,
            vec![
                std::cmp::Ord::min(delay, 300).into(),
                job.id.clone().into(),
                job.token.clone().into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    if updated.rows_affected() != 1 {
        return Err(Error::custom(
            "queue_lease_lost",
            "job lease ownership changed before retry scheduling",
        ));
    }
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).map_err(Error::internal)?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.map_err(Error::internal),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> Result<()> {
    tokio::signal::ctrl_c().await.map_err(Error::internal)
}

async fn insert_job<T: Serialize>(
    db: &Db,
    transaction: &DatabaseTransaction,
    id: &str,
    queue: &str,
    job_name: &str,
    payload: &T,
    options: &DispatchOptions,
) -> Result<bool> {
    validate_name("queue", queue, MAX_QUEUE_NAME)?;
    validate_name("job name", job_name, MAX_JOB_NAME)?;
    let payload = serde_json::to_string(payload).map_err(Error::internal)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::custom(
            "queue_payload_too_large",
            "job payload exceeds 1 MiB",
        ));
    }
    validate_options(options)?;
    if let Some(unique_key) = options.unique_key.as_deref() {
        validate_name("unique key", unique_key, 128)?;
    }
    let delay_secs = options
        .delay
        .as_secs()
        .saturating_add(u64::from(options.delay.subsec_nanos() > 0));
    let table = db.quoted_table_name("jobs")?;
    let backend = db.backend()?;
    let (sql, values) = match backend {
        DbBackend::Postgres => (
            format!(
                "INSERT INTO {table} (id, queue, job_name, payload, max_attempts, available_at, created_at, unique_key) VALUES ($1, $2, $3, $4::jsonb, $5, CURRENT_TIMESTAMP + ($6 * INTERVAL '1 second'), CURRENT_TIMESTAMP, $7) ON CONFLICT (queue, unique_key) DO NOTHING"
            ),
            vec![
                id.into(),
                queue.into(),
                job_name.into(),
                payload.into(),
                i64::from(options.max_attempts).into(),
                delay_secs.into(),
                options.unique_key.clone().into(),
            ],
        ),
        DbBackend::MySql => (
            if options.unique_key.is_some() {
                format!(
                    "INSERT INTO {table} (id, queue, job_name, payload, max_attempts, available_at, created_at, unique_key) VALUES (?, ?, ?, CAST(? AS JSON), ?, DATE_ADD(UTC_TIMESTAMP(6), INTERVAL ? SECOND), UTC_TIMESTAMP(6), ?) ON DUPLICATE KEY UPDATE id = id"
                )
            } else {
                format!(
                    "INSERT INTO {table} (id, queue, job_name, payload, max_attempts, available_at, created_at, unique_key) VALUES (?, ?, ?, CAST(? AS JSON), ?, DATE_ADD(UTC_TIMESTAMP(6), INTERVAL ? SECOND), UTC_TIMESTAMP(6), ?)"
                )
            },
            vec![
                id.into(),
                queue.into(),
                job_name.into(),
                payload.into(),
                i64::from(options.max_attempts).into(),
                delay_secs.into(),
                options.unique_key.clone().into(),
            ],
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    let result = transaction
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    Ok(result.rows_affected() == 1)
}

async fn insert_or_get_job(
    db: &Db,
    transaction: &DatabaseTransaction,
    queue: &str,
    job_name: &str,
    payload: &Value,
    options: &DispatchOptions,
) -> Result<String> {
    for _ in 0..3 {
        let id = new_id()?;
        let inserted = insert_job(db, transaction, &id, queue, job_name, payload, options).await?;
        if options.unique_key.is_none() && inserted {
            return Ok(id);
        }
        let unique_key = options.unique_key.as_deref().ok_or_else(|| {
            Error::custom(
                "queue_unique_key_missing",
                "job insert conflicted without a unique key",
            )
        })?;
        if let Some(existing_id) = find_unique_job(db, transaction, queue, unique_key).await? {
            return Ok(existing_id);
        }
    }
    Err(Error::custom(
        "queue_unique_job_race",
        "unique job repeatedly completed while another dispatch was resolving its key",
    ))
}

async fn find_unique_job(
    db: &Db,
    transaction: &DatabaseTransaction,
    queue: &str,
    unique_key: &str,
) -> Result<Option<String>> {
    let table = db.quoted_table_name("jobs")?;
    let backend = db.backend()?;
    let (sql, values) = match backend {
        DbBackend::Postgres => (
            format!("SELECT id FROM {table} WHERE queue = $1 AND unique_key = $2 FOR UPDATE"),
            vec![queue.into(), unique_key.into()],
        ),
        DbBackend::MySql => (
            format!("SELECT id FROM {table} WHERE queue = ? AND unique_key = ? FOR UPDATE"),
            vec![queue.into(), unique_key.into()],
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    transaction
        .query_one_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?
        .map(|row| row.try_get("", "id").map_err(Error::internal))
        .transpose()
}

pub(super) fn validate_options(options: &DispatchOptions) -> Result<()> {
    if options.max_attempts == 0
        || options.max_attempts > 100
        || options.delay > Duration::from_secs(MAX_DELAY_SECS)
    {
        return Err(Error::custom(
            "invalid_queue_dispatch_options",
            "max_attempts must be 1..=100 and delay must not exceed 30 days",
        ));
    }
    if let Some(unique_key) = options.unique_key.as_deref() {
        validate_name("unique key", unique_key, 128)?;
    }
    Ok(())
}

pub(super) fn validate_payload(payload: &Value) -> Result<()> {
    let encoded = serde_json::to_vec(payload).map_err(Error::internal)?;
    if encoded.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::custom(
            "queue_payload_too_large",
            "job payload exceeds 1 MiB",
        ));
    }
    Ok(())
}

pub(super) fn validate_name(kind: &str, value: &str, limit: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > limit
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(Error::custom(
            "invalid_queue_name",
            format!("{kind} must be 1..={limit} ASCII letters, digits, '.', '_', ':', or '-'"),
        ));
    }
    Ok(())
}

fn safe_error_code(code: &str) -> &str {
    if (1..=128).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        code
    } else {
        "handler_failed"
    }
}

fn new_id() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| {
        Error::custom(
            "queue_id_generation_failed",
            "cannot generate a job identifier",
        )
    })?;
    let mut encoded = Vec::with_capacity(32);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)]);
        encoded.push(HEX[usize::from(byte & 0x0f)]);
    }
    String::from_utf8(encoded).map_err(Error::internal)
}

#[allow(clippy::print_stdout)]
fn print_requeued(id: &str) {
    println!("Requeued job {id}");
}

#[cfg(all(test, feature = "queue"))]
mod tests {
    use super::{
        Job, JobContext, JobRegistry, QueueContributions, persist_execution_metadata,
        restore_execution_metadata,
    };
    use crate::core::{Cancellation, ExecutionContext, ExecutionMetadata};
    use crate::schedule::{CronOptions, MissedRunPolicy};
    use crate::support::BoxFuture;
    use serde::Deserialize;
    use std::time::Duration;

    #[derive(Deserialize)]
    struct ScopedJob;

    impl Job for ScopedJob {
        const NAME: &'static str = "tests.scoped";

        fn handle(&self, _context: JobContext) -> BoxFuture<'_, crate::core::Result<()>> {
            assert!(ExecutionContext::current().is_some());
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn registry_execute_installs_scope_and_enforces_deadline() -> crate::core::Result<()> {
        let mut registry = JobRegistry::default();
        registry.register_job::<ScopedJob>()?;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(10);
        let context = JobContext::new("job-1", 1, 1, Cancellation::new())?
            .with_execution_metadata(ExecutionMetadata::default().with_request_id("request-1")?)
            .with_deadline(deadline);

        assert!(
            registry
                .execute("tests.scoped", serde_json::json!({}), context)
                .await
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn execution_metadata_round_trips_and_legacy_payloads_stay_raw() -> crate::core::Result<()> {
        let metadata = crate::core::ExecutionMetadata::default()
            .with_request_id("request-42")?
            .with_locale(crate::translation::Locale::new("zh-CN")?)
            .with_tenant_id(crate::tenant::TenantId::new("tenant-a")?);
        let payload = serde_json::json!({"value": 7});
        let stored = persist_execution_metadata(payload.clone(), &metadata)?;
        let (restored_payload, restored_metadata) = restore_execution_metadata(stored)?;
        assert_eq!(restored_payload, payload);
        assert_eq!(restored_metadata, metadata);

        let legacy = serde_json::json!({"value": 8});
        let (restored_payload, restored_metadata) = restore_execution_metadata(legacy.clone())?;
        assert_eq!(restored_payload, legacy);
        assert!(restored_metadata.is_empty());
        Ok(())
    }

    #[test]
    fn malformed_tenant_metadata_is_rejected_without_running_as_legacy_payload() {
        let envelope = serde_json::json!({
            "__openrust_execution": "v1",
            "metadata": { "tenant_id": "../other" },
            "payload": {}
        });
        assert_eq!(
            restore_execution_metadata(envelope)
                .err()
                .map(|error| error.code().to_owned()),
            Some("queue_execution_metadata_invalid".to_owned())
        );
    }

    #[test]
    fn malformed_tenant_envelope_shape_is_rejected() {
        let envelope = serde_json::json!({
            "__openrust_execution": "v1",
            "metadata": { "tenant_id": "tenant-a" },
            "payload": {},
            "unexpected": true
        });
        assert_eq!(
            restore_execution_metadata(envelope)
                .err()
                .map(|error| error.code().to_owned()),
            Some("queue_execution_metadata_invalid".to_owned())
        );
    }

    #[tokio::test]
    async fn queue_dispatch_rejects_conflicting_execution_and_task_tenants()
    -> crate::core::Result<()> {
        let execution = ExecutionContext::new(
            ExecutionMetadata::default().with_tenant_id(crate::tenant::TenantId::new("tenant-a")?),
            None,
            Cancellation::new(),
        );
        crate::tenant::TenantContext::scope(
            crate::tenant::TenantId::new("tenant-b")?,
            crate::core::with_execution_context(execution, async {
                assert_eq!(
                    super::current_execution_metadata()
                        .err()
                        .map(|error| error.code().to_owned()),
                    Some("tenant_scope_switch_denied".to_owned())
                );
                Ok::<_, crate::core::Error>(())
            }),
        )
        .await??;
        Ok(())
    }

    #[test]
    fn metadata_envelope_rejects_reserved_payload_key() -> crate::core::Result<()> {
        let metadata = crate::core::ExecutionMetadata::default().with_request_id("request-42")?;
        assert!(
            persist_execution_metadata(
                serde_json::json!({"__openrust_execution": "application-value"}),
                &metadata
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn malformed_legacy_envelope_shape_remains_an_application_payload() {
        let legacy = serde_json::json!({
            "__openrust_execution": "v1",
            "metadata": {"request_id": "contains spaces"},
            "payload": {"application": true}
        });
        assert_eq!(
            restore_execution_metadata(legacy)
                .err()
                .map(|error| error.code().to_owned()),
            Some("queue_execution_metadata_invalid".to_owned())
        );
    }

    #[test]
    fn cron_registration_rejects_invalid_schedule_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let plugin = QueueContributions::default();
        plugin.schedule_cron(
            "reports.daily",
            "reports",
            "reports.daily.v1",
            &serde_json::json!({}),
            "30 9 * * 1-5",
            CronOptions::default(),
        )?;
        assert!(
            plugin
                .schedule_cron(
                    "reports.invalid-expression",
                    "reports",
                    "reports.daily.v1",
                    &serde_json::json!({}),
                    "0 0 31 2 *",
                    CronOptions::default(),
                )
                .is_err()
        );
        assert!(
            plugin
                .schedule_cron(
                    "reports.invalid-time-zone",
                    "reports",
                    "reports.daily.v1",
                    &serde_json::json!({}),
                    "* * * * *",
                    CronOptions {
                        time_zone: "Mars/Olympus".to_owned(),
                        missed_runs: MissedRunPolicy::Skip
                    },
                )
                .is_err()
        );
        assert!(
            plugin
                .schedule_cron(
                    "reports.invalid-catch-up",
                    "reports",
                    "reports.daily.v1",
                    &serde_json::json!({}),
                    "* * * * *",
                    CronOptions {
                        time_zone: "UTC".to_owned(),
                        missed_runs: MissedRunPolicy::CatchUp { max_slots: 101 }
                    },
                )
                .is_err()
        );
        assert!(
            plugin
                .schedule_cron(
                    "reports.daily",
                    "reports",
                    "reports.daily.v1",
                    &serde_json::json!({}),
                    "* * * * *",
                    CronOptions::default(),
                )
                .is_err()
        );
        Ok(())
    }
}
