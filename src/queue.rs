//! Durable, database-backed job dispatch.
//!
//! Applications must register [`QueueMigration`] in their migration crate. Dispatch is durable
//! once the surrounding database transaction commits; handlers and workers are introduced by
//! the queue worker API.
//!
//! ```no_run
//! use openrust::{db::Db, queue::Queue};
//! # async fn example(db: Db) -> openrust::Result<()> {
//! let queue = Queue::new(db);
//! queue.dispatch("emails", "welcome.v1", &serde_json::json!({"user_id": 42})).await?;
//! # Ok(())
//! # }
//! ```

use crate::core::{AppBuilder, BoxFuture, Error, Plugin, Result};
use crate::db::{DatabaseTransaction, Db, DbBackend};
use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const MAX_QUEUE_NAME: usize = 64;
const MAX_JOB_NAME: usize = 128;
const MAX_PAYLOAD_BYTES: usize = 1_048_576;
const DEFAULT_MAX_ATTEMPTS: u32 = 5;
const MAX_DELAY_SECS: u64 = 2_592_000;
const HEX: &[u8; 16] = b"0123456789abcdef";

/// A typed job handler registered explicitly with [`QueuePlugin`].
pub trait Job: DeserializeOwned + Send + Sync + 'static {
    /// Stable persisted job name. Keep this name when the Rust type is renamed.
    const NAME: &'static str;

    /// Runs the task. Handlers must be idempotent because database queues provide at-least-once execution.
    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>>;
}

/// Runtime metadata and cooperative cancellation signal for a job attempt.
#[derive(Clone)]
pub struct JobContext {
    id: String,
    attempt: u64,
    max_attempts: u64,
    cancelled: tokio::sync::watch::Receiver<bool>,
    db: Db,
    owner: String,
    token: String,
}

impl JobContext {
    /// Durable job identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Current one-based attempt number.
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }
    /// Maximum attempts configured for this job.
    pub const fn max_attempts(&self) -> u64 {
        self.max_attempts
    }
    /// Returns whether worker shutdown requested cooperative cancellation.
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    /// Waits until worker shutdown requests cooperative cancellation.
    pub async fn cancelled(&mut self) {
        if !self.is_cancelled() {
            let _ = self.cancelled.changed().await;
        }
    }

    /// Stores bounded JSON progress while this attempt still owns the job lease.
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
        let connection = self.db.connection()?;
        let backend = self.db.backend()?;
        let table = self.db.quoted_table_name("jobs")?;
        let (sql, values) = match backend {
            DbBackend::Postgres => (
                format!(
                    "UPDATE {table} SET progress = $1::jsonb WHERE id = $2 AND reserved_by = $3 AND ownership_token = $4"
                ),
                vec![
                    encoded.into(),
                    self.id.clone().into(),
                    self.owner.clone().into(),
                    self.token.clone().into(),
                ],
            ),
            DbBackend::MySql => (
                format!(
                    "UPDATE {table} SET progress = CAST(? AS JSON) WHERE id = ? AND reserved_by = ? AND ownership_token = ?"
                ),
                vec![
                    encoded.into(),
                    self.id.clone().into(),
                    self.owner.clone().into(),
                    self.token.clone().into(),
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
}

type JobFn = fn(Value, JobContext) -> BoxFuture<'static, Result<()>>;

#[derive(Default, Clone)]
pub(crate) struct JobRegistry(HashMap<&'static str, JobFn>);

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

/// Creates the durable jobs table. Register this migration explicitly in the application migrator.
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
        let mut timestamp = crate::db::dialect::timestamp_utc("available_at", backend)?;
        let mut created = crate::db::dialect::timestamp_utc("created_at", backend)?;
        let mut reserved_until = crate::db::dialect::timestamp_utc("reserved_until", backend)?;
        let mut failed_at = crate::db::dialect::timestamp_utc("failed_at", backend)?;
        let mut ready_index = Index::create();
        ready_index
            .name("idx_openrust_jobs_ready")
            .table(table.clone())
            .col(Jobs::Queue)
            .col(Jobs::AvailableAt)
            .col(Jobs::Id);
        manager
            .create_table(
                Table::create()
                    .table(table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Jobs::Id)
                            .string_len(32)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Jobs::Queue).string_len(64).not_null())
                    .col(ColumnDef::new(Jobs::JobName).string_len(128).not_null())
                    .col(crate::db::dialect::json("payload", backend)?.not_null())
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
                    .col(crate::db::dialect::json("progress", backend)?.null())
                    .col(created.not_null())
                    .to_owned(),
            )
            .await?;
        manager.create_index(ready_index.clone()).await?;
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
                    .col(crate::db::dialect::json("payload", backend)?.not_null())
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
        let mut schedule_created = crate::db::dialect::timestamp_utc("created_at", backend)?;
        let mut scheduled_at = crate::db::dialect::timestamp_utc("scheduled_at", backend)?;
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
            .drop_table(Table::drop().table(table).if_exists().to_owned())
            .await
    }
}

fn queue_table(logical: &str, backend: DbBackend) -> std::result::Result<Alias, DbErr> {
    let names = crate::db::DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration("queue migration requires MigrationPlugin or TestDatabase scope".into())
    })?;
    names
        .quote(logical, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(logical)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

/// Backend contract for queue storage providers.
pub trait QueueDriver: Send + Sync + 'static {
    /// Persists serialized job parameters with the supplied retry policy.
    fn dispatch<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>>;
    /// Persists a job in the caller's SeaORM transaction.
    fn dispatch_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>>;
    /// Runs at most one ready job from the named queue.
    fn run_next<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<bool>>;
    /// Requeues a failed job and returns the stable ID of the created job.
    fn retry_failed<'a>(&'a self, failed_id: &'a str) -> BoxFuture<'a, Result<String>>;
    /// Clears the shutdown state before a worker process starts.
    fn reset_shutdown(&self);
    /// Requests cooperative cancellation for active handlers.
    fn request_shutdown(&self);
    /// Reports whether shutdown has been requested.
    fn is_shutdown_requested(&self) -> bool;
}

/// Database queue driver backed by the application's shared primary connection.
#[derive(Clone)]
pub struct DatabaseQueue {
    db: Db,
    cancelled: tokio::sync::watch::Sender<bool>,
    registry: Arc<JobRegistry>,
    config: crate::config::QueueConfig,
}

/// Queue facade which delegates job persistence and execution to a registered driver.
#[derive(Clone)]
pub struct Queue {
    driver: Arc<dyn QueueDriver>,
}

/// Per-job retry and delay settings.
#[derive(Debug, Clone, Copy)]
pub struct DispatchOptions {
    /// Maximum number of handler attempts, including the first attempt.
    pub max_attempts: u32,
    /// Delay before the first attempt.
    pub delay: Duration,
}

impl Default for DispatchOptions {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            delay: Duration::ZERO,
        }
    }
}

/// Registers queue services and explicitly allowed job handlers.
#[derive(Debug, Default, Clone)]
pub struct QueuePlugin {
    handlers: HashMap<&'static str, JobFn>,
    schedules: Vec<crate::schedule::ScheduleDefinition>,
}

impl QueuePlugin {
    /// Registers a typed handler under its stable [`Job::NAME`].
    pub fn register_job<J: Job>(&mut self) -> Result<&mut Self> {
        validate_name("job name", J::NAME, MAX_JOB_NAME)?;
        if self.handlers.contains_key(J::NAME) {
            return Err(Error::custom(
                "duplicate_queue_job",
                format!("job handler `{}` is registered more than once", J::NAME),
            ));
        }
        self.handlers.insert(J::NAME, run_job::<J>);
        Ok(self)
    }

    /// Registers a job to be dispatched at each bounded UTC interval slot.
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

    /// Registers a job at a daily UTC hour and minute, skipping missed daily slots.
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

impl Plugin for QueuePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let db = app.get::<Db>().ok_or_else(|| {
            Error::custom(
                "queue_requires_database",
                "register DatabasePlugin before QueuePlugin",
            )
        })?;
        let registry = JobRegistry(self.handlers.clone());
        let config = match app.section::<crate::config::QueueConfig>("queue") {
            Ok(config) => config,
            Err(crate::config::ConfigError::Source(source)) if source.missing() => {
                crate::config::QueueConfig::default()
            }
            Err(error) => return Err(Error::Config(error)),
        };
        validate_config(&config)?;
        for schedule in &self.schedules {
            if !self.handlers.contains_key(schedule.job_name()) {
                return Err(Error::custom(
                    "unregistered_schedule_job",
                    format!(
                        "schedule `{}` references unregistered job `{}`",
                        schedule.key(),
                        schedule.job_name()
                    ),
                ));
            }
        }
        let driver = DatabaseQueue::configured((*db).clone(), registry, config.clone());
        let queue = Queue::with_driver(driver, &config)?;
        app.insert(crate::schedule::Scheduler::new(
            (*db).clone(),
            queue.clone(),
            self.schedules.clone(),
            config.clone(),
        ));
        app.insert(queue);
        app.insert(config);
        app.command(QueueWorkCommand);
        app.command(QueueRetryCommand);
        app.command(crate::schedule::ScheduleRunCommand);
        app.command(crate::schedule::ScheduleListCommand);
        Ok(())
    }
}

fn run_job<J: Job>(payload: Value, context: JobContext) -> BoxFuture<'static, Result<()>> {
    Box::pin(async move {
        let job = serde_json::from_value::<J>(payload).map_err(Error::internal)?;
        job.handle(context).await
    })
}

impl DatabaseQueue {
    /// Creates a dispatcher using the provided database service.
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
        validate_options(options)?;
        let connection = self.db.connection()?;
        let transaction = connection.begin().await.map_err(Error::internal)?;
        let id = new_id()?;
        insert_job(
            &self.db,
            &transaction,
            &id,
            queue,
            job_name,
            &payload,
            options,
        )
        .await?;
        transaction.commit().await.map_err(Error::internal)?;
        Ok(id)
    }

    async fn work_once(&self, queue: &str) -> Result<bool> {
        validate_name("queue", queue, MAX_QUEUE_NAME)?;
        let Some(claim) = claim_job(&self.db, queue, self.config.lease_secs).await? else {
            return Ok(false);
        };
        let Some(handler) = self.registry.0.get(claim.job_name.as_str()).copied() else {
            fail_job(&self.db, &claim, "unregistered_job_type", 1).await?;
            return Err(Error::custom(
                "unregistered_queue_job",
                format!("no handler is registered for job `{}`", claim.job_name),
            ));
        };
        let (job_cancelled, job_cancellation) = tokio::sync::watch::channel(false);
        let context = JobContext {
            id: claim.id.clone(),
            attempt: claim.attempts,
            max_attempts: claim.max_attempts,
            cancelled: job_cancellation,
            db: self.db.clone(),
            owner: claim.owner.clone(),
            token: claim.token.clone(),
        };
        let mut cancellation = self.cancelled.subscribe();
        let handler_future = handler(claim.payload.clone(), context);
        tokio::pin!(handler_future);
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
                    if !renew_job(&self.db, &claim, self.config.lease_secs).await? {
                        job_cancelled.send_replace(true);
                        break Some(Err(Error::custom("queue_lease_lost", "job lease ownership changed")));
                    }
                }
                changed = cancellation.changed() => {
                    if changed.is_ok() {
                        job_cancelled.send_replace(true);
                        break Some(Err(Error::custom("queue_job_cancelled", "worker shutdown cancelled this attempt")));
                    }
                }
                () = &mut timeout => {
                    job_cancelled.send_replace(true);
                    break None;
                }
            }
        };
        match result {
            Some(Ok(())) => acknowledge_job(&self.db, &claim).await?,
            Some(Err(error)) => {
                let shift = <u32 as TryFrom<u64>>::try_from(claim.attempts.saturating_sub(1))
                    .unwrap_or(u32::MAX);
                let delay = std::cmp::Ord::min(1_u64.checked_shl(shift).unwrap_or(300), 300);
                fail_job(&self.db, &claim, safe_error_code(error.code()), delay).await?;
            }
            None => fail_job(&self.db, &claim, "job_timeout", 1).await?,
        }
        Ok(true)
    }

    /// Inserts a job in the caller's business transaction, so rollback removes both changes.
    async fn dispatch_value_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        job_name: &str,
        payload: Value,
        options: DispatchOptions,
    ) -> Result<String> {
        validate_options(options)?;
        let id = new_id()?;
        insert_job(
            &self.db,
            transaction,
            &id,
            queue,
            job_name,
            &payload,
            options,
        )
        .await?;
        Ok(id)
    }

    /// Requeues a failed job under a new ID while retaining its original failed record.
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
            DispatchOptions::default(),
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
    /// Creates a database-backed dispatcher using the application's shared database service.
    pub fn new(db: Db) -> Self {
        let config = crate::config::QueueConfig::default();
        Self {
            driver: Arc::new(DatabaseQueue::configured(
                db,
                JobRegistry::default(),
                config,
            )),
        }
    }

    /// Creates a dispatcher with an application-provided queue driver.
    pub fn with_driver(
        driver: impl QueueDriver,
        config: &crate::config::QueueConfig,
    ) -> Result<Self> {
        validate_config(config)?;
        Ok(Self {
            driver: Arc::new(driver),
        })
    }

    /// Inserts a job which becomes available immediately after its transaction commits.
    pub async fn dispatch<T: Serialize>(
        &self,
        queue: &str,
        job_name: &str,
        payload: &T,
    ) -> Result<String> {
        self.dispatch_with_options(queue, job_name, payload, DispatchOptions::default())
            .await
    }

    /// Inserts a job with an explicit bounded initial delay and retry limit.
    pub async fn dispatch_with_options<T: Serialize>(
        &self,
        queue: &str,
        job_name: &str,
        payload: &T,
        options: DispatchOptions,
    ) -> Result<String> {
        validate_options(options)?;
        self.driver
            .dispatch(
                queue,
                job_name,
                serde_json::to_value(payload).map_err(Error::internal)?,
                options,
            )
            .await
    }

    /// Inserts a job in the caller's business transaction, so rollback removes both changes.
    pub async fn dispatch_in_transaction<T: Serialize>(
        &self,
        transaction: &DatabaseTransaction,
        queue: &str,
        job_name: &str,
        payload: &T,
    ) -> Result<String> {
        self.driver
            .dispatch_in_transaction(
                transaction,
                queue,
                job_name,
                serde_json::to_value(payload).map_err(Error::internal)?,
                DispatchOptions::default(),
            )
            .await
    }

    /// Claims and runs one ready job from the named queue, returning whether one was claimed.
    pub async fn run_next(&self, queue: &str) -> Result<bool> {
        self.driver.run_next(queue).await
    }

    /// Requeues a failed job and returns its stable replacement ID.
    pub async fn retry_failed(&self, failed_id: &str) -> Result<String> {
        self.driver.retry_failed(failed_id).await
    }

    fn reset_shutdown(&self) {
        self.driver.reset_shutdown();
    }
    fn request_shutdown(&self) {
        self.driver.request_shutdown();
    }
    fn is_shutdown_requested(&self) -> bool {
        self.driver.is_shutdown_requested()
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

    fn run_next<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { self.work_once(queue).await })
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

struct QueueWorkCommand;

impl crate::core::Command for QueueWorkCommand {
    fn name(&self) -> &'static str {
        "queue work"
    }

    fn about(&self) -> &'static str {
        "Run the database queue worker"
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(
        &'a self,
        app: &'a crate::core::App,
        args: Vec<String>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if args.len() > 1 {
                return Err(Error::custom(
                    "invalid_queue_command",
                    "usage: queue work [queue]",
                ));
            }
            let queue_name = args.first().map_or("default", String::as_str);
            validate_name("queue", queue_name, MAX_QUEUE_NAME)?;
            let queue = app.state().try_get::<Queue>()?;
            let config = app.state().try_get::<crate::config::QueueConfig>()?;
            queue.reset_shutdown();
            let mut workers = tokio::task::JoinSet::new();
            for _ in 0..config.worker_concurrency {
                let queue = (*queue).clone();
                let queue_name = queue_name.to_owned();
                let poll_interval = config.poll_interval_millis;
                workers.spawn(async move { worker_loop(queue, queue_name, poll_interval).await });
            }
            loop {
                tokio::select! {
                    signal = shutdown_signal() => {
                        signal?;
                        queue.request_shutdown();
                        let drain = join_workers(&mut workers);
                        if tokio::time::timeout(Duration::from_secs(config.shutdown_timeout_secs), drain).await.is_err() {
                            workers.abort_all();
                            while workers.join_next().await.is_some() {}
                        }
                        return Ok(());
                    }
                    result = workers.join_next() => {
                        if let Some(result) = result {
                            queue.request_shutdown();
                            workers.abort_all();
                            while workers.join_next().await.is_some() {}
                            return match result.map_err(Error::internal)? {
                                Ok(()) => Err(Error::custom("queue_worker_stopped", "queue worker exited unexpectedly")),
                                Err(error) => Err(error),
                            };
                        }
                    }
                }
            }
        })
    }
}

async fn worker_loop(queue: Queue, queue_name: String, poll_interval_millis: u64) -> Result<()> {
    loop {
        if queue.is_shutdown_requested() {
            return Ok(());
        }
        if !queue.run_next(&queue_name).await? {
            tokio::time::sleep(Duration::from_millis(poll_interval_millis)).await;
        }
    }
}

async fn join_workers(workers: &mut tokio::task::JoinSet<Result<()>>) -> Result<()> {
    while let Some(result) = workers.join_next().await {
        result.map_err(Error::internal)??;
    }
    Ok(())
}

struct QueueRetryCommand;

impl crate::core::Command for QueueRetryCommand {
    fn name(&self) -> &'static str {
        "queue retry"
    }
    fn about(&self) -> &'static str {
        "Retry a failed database job"
    }
    fn accepts_args(&self) -> bool {
        true
    }
    fn run<'a>(
        &'a self,
        app: &'a crate::core::App,
        args: Vec<String>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let [failed_id] = args.as_slice() else {
                return Err(Error::custom(
                    "invalid_queue_command",
                    "usage: queue retry <failed-job-id>",
                ));
            };
            let queue = app.state().try_get::<Queue>()?;
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
    options: DispatchOptions,
) -> Result<()> {
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
    let delay_secs = options
        .delay
        .as_secs()
        .saturating_add(u64::from(options.delay.subsec_nanos() > 0));
    let table = db.quoted_table_name("jobs")?;
    let backend = db.backend()?;
    let (sql, values) = match backend {
        DbBackend::Postgres => (
            format!(
                "INSERT INTO {table} (id, queue, job_name, payload, max_attempts, available_at, created_at) VALUES ($1, $2, $3, $4::jsonb, $5, CURRENT_TIMESTAMP + ($6 * INTERVAL '1 second'), CURRENT_TIMESTAMP)"
            ),
            vec![
                id.into(),
                queue.into(),
                job_name.into(),
                payload.into(),
                i64::from(options.max_attempts).into(),
                delay_secs.into(),
            ],
        ),
        DbBackend::MySql => (
            format!(
                "INSERT INTO {table} (id, queue, job_name, payload, max_attempts, available_at, created_at) VALUES (?, ?, ?, CAST(? AS JSON), ?, DATE_ADD(UTC_TIMESTAMP(6), INTERVAL ? SECOND), UTC_TIMESTAMP(6))"
            ),
            vec![
                id.into(),
                queue.into(),
                job_name.into(),
                payload.into(),
                i64::from(options.max_attempts).into(),
                delay_secs.into(),
            ],
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_queue_backend",
                "database queue requires PostgreSQL or MySQL",
            ));
        }
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

fn validate_options(options: DispatchOptions) -> Result<()> {
    if options.max_attempts == 0
        || options.max_attempts > 100
        || options.delay > Duration::from_secs(MAX_DELAY_SECS)
    {
        return Err(Error::custom(
            "invalid_queue_dispatch_options",
            "max_attempts must be 1..=100 and delay must not exceed 30 days",
        ));
    }
    Ok(())
}

fn validate_payload(payload: &Value) -> Result<()> {
    let encoded = serde_json::to_vec(payload).map_err(Error::internal)?;
    if encoded.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::custom(
            "queue_payload_too_large",
            "job payload exceeds 1 MiB",
        ));
    }
    Ok(())
}

fn validate_name(kind: &str, value: &str, limit: usize) -> Result<()> {
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
