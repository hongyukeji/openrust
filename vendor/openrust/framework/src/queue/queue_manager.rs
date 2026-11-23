#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    bus::{
        Batch, BatchCallback, BatchContinuation, BatchJob, BatchPhase, ChainCallback,
        DatabaseBatchRepository, new_batch,
    },
    cache::{CacheManager, Ttl},
    container::Container,
    contracts::{
        config::Repository,
        container::Container as _,
        events::{Dispatcher as _, Event},
        queue::{Factory, Job, JobHandler, Queue as Contract},
    },
    database::{Connection, DatabaseError, DatabaseManager, Transaction},
    events::{Dispatcher, EventContext, ListenerResponse},
    redis::RedisManager,
    support::facades::{application_scope, scope},
};

use super::{
    DatabaseQueue, DispatchOptions, DispatchResult, JobPopCallback, Payload, Queue, QueueError,
    QueueResult, RedisQueue, SyncQueue, backend,
    events::{
        JobRetryRequested, QueuePaused, QueueResumed, QueuesPaused, QueuesResumed, UniqueJobSkipped,
    },
    failed::{FailedJob, FailedJobProviderInterface},
    new_id, queue_name,
    worker_control::WorkerControl,
};

/// Resolves and caches configured queue connections for one application.
pub struct QueueManager {
    config: Arc<dyn Repository>,
    owner: Weak<Container>,
    registry: Arc<Queue>,
    bus_pipes: RwLock<Vec<Arc<dyn super::middleware::JobMiddleware>>>,
    bus_after_response: AtomicBool,
    connections: Mutex<HashMap<String, Arc<ConfiguredQueue>>>,
    connectors: RwLock<HashMap<String, Arc<Connector>>>,
    batch_callbacks: RwLock<HashMap<String, BatchCallback>>,
    chain_callbacks: RwLock<HashMap<String, ChainCallback>>,
    pop_callbacks: RwLock<HashMap<String, JobPopCallback>>,
    interruption_polling: AtomicBool,
    clear_prohibited: AtomicBool,
}

type Connector = dyn Fn(&Container, &str, &Value) -> QueueResult<Arc<dyn Contract>> + Send + Sync;

struct ConfiguredQueue {
    queue: Arc<dyn Contract>,
    after_commit: bool,
    capture_context_on_dispatch: bool,
    synchronous: bool,
    default_queue: String,
    batch_backend: BatchBackend,
}

enum BatchBackend {
    Database(Connection),
    Redis(Arc<RedisQueue>),
    Beanstalkd,
    Sqs(Arc<super::sqs_queue::SqsQueue>),
    Unsupported,
}

#[derive(Deserialize)]
struct Settings {
    driver: String,
    host: Option<String>,
    port: Option<u16>,
    connection: Option<String>,
    connections: Option<Vec<String>>,
    table: Option<String>,
    queue: Option<String>,
    retry_after: Option<i64>,
    block_for: Option<u32>,
    #[serde(default)]
    after_commit: bool,
}

impl QueueManager {
    pub(super) async fn run_deferred(
        &self,
        name: String,
        queue: String,
        payload: Payload,
    ) -> QueueResult<()> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        scope(owner, async {
            use crate::log::context::Context;
            let (repository, hooks) = match Context::repository()
                .and_then(|repository| Ok((repository, Context::hooks()?)))
            {
                Ok(context) => context,
                Err(error) => {
                    let _ = self.release_lifecycle_lock(&payload).await;
                    return Err(backend(error));
                }
            };
            repository
                .scope(
                    async {
                        repository.flush();
                        hooks.hydrate(&payload.context, &repository);
                        self.run_background(name, queue, payload).await
                    },
                    Default::default(),
                    Default::default(),
                )
                .await
        })
        .await
    }

    pub(super) async fn run_background(
        &self,
        name: String,
        queue: String,
        payload: Payload,
    ) -> QueueResult<()> {
        let result = SyncQueue {
            name,
            registry: self.registry.clone(),
        }
        .push(&payload, Some(&queue))
        .await
        .map(|_| ());
        result.and(self.release_lifecycle_lock(&payload).await)
    }

    pub(super) async fn release_lifecycle_lock(&self, payload: &Payload) -> QueueResult<()> {
        if payload.unique_lock.is_none() {
            return Ok(());
        }
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        super::unique_job::release(&owner, payload).await
    }

    pub(super) fn background_driver(&self) -> QueueResult<crate::concurrency::Driver> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        owner
            .make::<crate::concurrency::Manager>()?
            .driver(Some("process"))
            .map_err(backend)
    }
    pub(super) fn new(config: Arc<dyn Repository>, owner: Weak<Container>) -> Self {
        Self {
            config,
            registry: Arc::new(Queue::new(owner.clone())),
            bus_pipes: RwLock::default(),
            bus_after_response: AtomicBool::new(true),
            owner,
            connections: Mutex::default(),
            connectors: RwLock::default(),
            batch_callbacks: RwLock::default(),
            chain_callbacks: RwLock::default(),
            pop_callbacks: RwLock::default(),
            interruption_polling: AtomicBool::new(true),
            clear_prohibited: AtomicBool::new(false),
        }
    }

    pub(super) async fn fire_isolated_job(
        &self,
        context: &mut super::JobContext<'_>,
        payload: &Payload,
    ) -> QueueResult<()> {
        self.registry.fire(context, payload).await
    }

    pub(super) async fn run_failed_handler(
        &self,
        context: &mut super::JobContext<'_>,
        payload: &Payload,
        error: &QueueError,
    ) -> QueueResult<()> {
        self.registry
            .run_failed_handler(context, payload, error)
            .await
    }

    pub fn register(&self, name: &str, handler: Arc<dyn JobHandler>) -> QueueResult<()> {
        self.registry.register(name, handler)
    }

    pub(crate) fn command_handler(&self, name: &str) -> Option<Arc<dyn JobHandler>> {
        self.registry.handler(name)
    }

    pub(crate) async fn dispatch_now(&self, payload: &Payload) -> QueueResult<()> {
        let handler = self
            .command_handler(payload.name())
            .ok_or_else(|| QueueError::UnknownJob(payload.name().into()))?;
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let mut context = super::JobContext {
            connection: "sync".into(),
            queue: "sync".into(),
            attempts: 1,
            ..Default::default()
        };
        let pipes = self
            .bus_pipes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        application_scope(owner.clone(), async {
            super::middleware::JobNext::new(handler.as_ref(), &pipes, &owner)
                .run(&mut context, payload)
                .await
        })
        .await
    }

    pub(crate) fn set_bus_pipes(&self, pipes: Vec<Arc<dyn super::middleware::JobMiddleware>>) {
        let old = std::mem::replace(
            &mut *self
                .bus_pipes
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            pipes,
        );
        drop(old);
    }

    pub(crate) fn set_bus_after_response(&self, enabled: bool) {
        self.bus_after_response.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn bus_after_response(&self) -> bool {
        self.bus_after_response.load(Ordering::Relaxed)
    }

    pub(crate) fn register_listeners(
        &self,
        registrations: Vec<(String, Arc<dyn JobHandler>)>,
    ) -> QueueResult<()> {
        self.registry.register_listeners(registrations)
    }

    /// Sets or removes a custom reservation callback for a worker name.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn popUsing(&self, worker: &str, callback: Option<JobPopCallback>) {
        let previous = {
            let mut callbacks = self
                .pop_callbacks
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match callback {
                Some(callback) => callbacks.insert(worker.into(), callback),
                None => callbacks.remove(worker),
            }
        };
        drop(previous);
    }

    pub(super) fn pop_callback(&self, worker: &str) -> Option<JobPopCallback> {
        self.pop_callbacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(worker)
            .cloned()
    }

    pub fn connection(&self, name: Option<&str>) -> QueueResult<Arc<dyn Contract>> {
        Ok(self.resolve(name)?.queue.clone())
    }

    /// Registers a driver factory for connections which have not been resolved.
    /// Concurrent first lookups can build candidates, but only one is cached.
    pub fn extend(
        &self,
        driver: impl Into<String>,
        connector: impl Fn(&Container, &str, &Value) -> QueueResult<Arc<dyn Contract>>
        + Send
        + Sync
        + 'static,
    ) {
        let previous = self
            .connectors
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(driver.into(), Arc::new(connector));
        drop(previous);
    }

    pub(crate) fn default_is_synchronous(&self) -> QueueResult<bool> {
        Ok(self.resolve(None)?.synchronous)
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn withoutInterruptionPolling(&self) {
        self.interruption_polling.store(false, Ordering::Relaxed);
    }

    /// Pauses one queue until it is resumed.
    pub async fn pause(&self, connection: &str, queue: &str) -> QueueResult<()> {
        self.set_queue_pause(connection, queue, None).await
    }

    /// Pauses one queue until the supplied cache deadline or duration expires.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn pauseFor(&self, connection: &str, queue: &str, ttl: Ttl) -> QueueResult<()> {
        self.set_queue_pause(connection, queue, Some(ttl)).await
    }

    async fn set_queue_pause(
        &self,
        connection: &str,
        queue: &str,
        ttl: Option<Ttl>,
    ) -> QueueResult<()> {
        self.worker_control()?
            .pause(Some((connection, queue)), ttl)
            .await?;
        self.emit_control_event(QueuePaused {
            connection: connection.into(),
            queue: queue.into(),
            ttl,
        })
        .await
    }

    /// Pauses all queues on all connections in this application's cache.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn pauseAll(&self) -> QueueResult<()> {
        self.worker_control()?.pause(None, None).await?;
        self.emit_control_event(QueuesPaused {}).await
    }

    /// Clears an individual queue's pause.
    pub async fn resume(&self, connection: &str, queue: &str) -> QueueResult<()> {
        self.worker_control()?
            .resume(Some((connection, queue)))
            .await?;
        self.emit_control_event(QueueResumed {
            connection: connection.into(),
            queue: queue.into(),
        })
        .await
    }

    /// Clears the global pause while preserving individual pauses.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn resumeAll(&self) -> QueueResult<()> {
        self.worker_control()?.resume(None).await?;
        self.emit_control_event(QueuesResumed {}).await
    }

    /// Checks both the global and individual pause state.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn isPaused(&self, connection: &str, queue: &str) -> QueueResult<bool> {
        let control = self.worker_control()?;
        Ok(control.all_paused().await? || control.queue_paused(connection, queue).await?)
    }

    /// Returns the paused queues in the requested order.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub async fn getPausedQueues<'a>(
        &self,
        connection: &str,
        queues: &[&'a str],
    ) -> QueueResult<Vec<&'a str>> {
        self.worker_control()?
            .paused_queues(connection, queues)
            .await
    }

    pub(super) fn worker_dispatcher(&self) -> QueueResult<(Arc<Container>, Arc<Dispatcher>)> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let dispatcher = owner.make::<Dispatcher>()?;
        Ok((owner, dispatcher))
    }

    pub(super) async fn emit_control_event(&self, event: impl Event) -> QueueResult<()> {
        let (owner, dispatcher) = self.worker_dispatcher()?;
        application_scope(owner, async {
            dispatcher
                .dispatch(Arc::new(event), &mut EventContext::new())
                .await
        })
        .await
        .map_err(backend)?;
        Ok(())
    }

    pub(super) fn observes_worker_event(&self, name: &str) -> QueueResult<bool> {
        let (_, dispatcher) = self.worker_dispatcher()?;
        Ok(dispatcher.observes(name))
    }

    pub(super) fn job_popped_event(
        &self,
        connection: &str,
        job: &dyn Job,
    ) -> QueueResult<Option<super::events::JobPopped>> {
        use super::events::{JobPopped, JobSnapshot};
        let (_, dispatcher) = self.worker_dispatcher()?;
        Ok(dispatcher.observes(JobPopped::NAME).then(|| JobPopped {
            connection_name: connection.into(),
            job: JobSnapshot::from_job(job),
        }))
    }

    pub(super) async fn allows_polling(&self, event: super::events::Looping) -> QueueResult<bool> {
        let (owner, dispatcher) = self.worker_dispatcher()?;
        let response = application_scope(owner, async {
            dispatcher
                .until(Arc::new(event), &mut EventContext::new())
                .await
        })
        .await
        .map_err(backend)?;
        Ok(
            !matches!(response, Some(ListenerResponse::Value(value)) if value.downcast_ref::<bool>() == Some(&false)),
        )
    }

    pub(super) fn polls_interruptions(&self) -> bool {
        self.interruption_polling.load(Ordering::Relaxed)
    }

    pub(super) fn set_clear_prohibited(&self, prohibited: bool) {
        self.clear_prohibited.store(prohibited, Ordering::Relaxed);
    }

    pub fn clear_prohibited(&self) -> bool {
        self.clear_prohibited.load(Ordering::Relaxed)
    }

    pub(super) fn worker_control(&self) -> QueueResult<WorkerControl> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let cache = owner.make::<CacheManager>()?;
        Ok(WorkerControl::new(cache.store(None).map_err(backend)?))
    }

    pub(super) fn control_target(&self, argument: &str) -> QueueResult<(String, String)> {
        let (connection, queue) = argument
            .split_once(':')
            .map_or((None, argument), |(connection, queue)| {
                (Some(connection), queue)
            });
        let queue = if queue.is_empty() { "default" } else { queue };
        let configured = self.resolve(connection)?;
        configured.queue_name(Some(queue))?;
        Ok((configured.queue.getConnectionName().into(), queue.into()))
    }

    pub async fn dispatch(
        &self,
        mut payload: Payload,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
        let configured = self.resolve(options.connection.as_deref())?;
        if configured.capture_context_on_dispatch {
            self.capture_context(&mut payload)?;
        }
        let queue = configured.queue.clone();
        payload.unique_lock = None;
        let unique = if payload.batch_id.is_none() && payload.chained_batch.is_none() {
            self.registry
                .handler(payload.name())
                .and_then(|handler| handler.uniqueJob(&payload))
        } else {
            None
        };
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let selected = options.queue.as_deref();
        let transaction = transaction.filter(|transaction| transaction.transactionLevel() != 0);
        if let Some(transaction) = transaction {
            if unique.is_some() && options.after_commit == Some(false) {
                return Err(QueueError::InvalidConfiguration);
            }
            if unique.is_some() || options.after_commit.unwrap_or(configured.after_commit) {
                transaction
                    .afterCommit(move || {
                        Box::pin(async move {
                            dispatch_unique(&*queue, payload, &options, owner, unique)
                                .await
                                .map(|_| ())
                                .map_err(|source| DatabaseError::AfterCommit {
                                    source: Box::new(source),
                                })
                        })
                    })
                    .map_err(backend)?;
                return Ok(DispatchResult::Deferred);
            }
            emit_queueing(&owner, &*queue, &payload, &options).await?;
            return queue
                .push_in_transaction(&payload, selected, options.delay, transaction)
                .await
                .map(dispatch_result);
        }
        dispatch_unique(&*queue, payload, &options, owner, unique).await
    }

    /// Retries a failure through the configured transport and reports whether it was claimed.
    pub async fn retry_failed(
        &self,
        failures: &dyn FailedJobProviderInterface,
        key: &str,
        job: &FailedJob,
    ) -> QueueResult<bool> {
        self.retry_failed_result(failures, key, job)
            .await
            .map(|(claimed, _)| claimed)
    }
    /// Returns the newly assigned job ID after a successful retry claim.
    pub async fn retry_failed_with_id(
        &self,
        failures: &dyn FailedJobProviderInterface,
        key: &str,
        job: &FailedJob,
    ) -> QueueResult<Option<String>> {
        self.retry_failed_result(failures, key, job)
            .await
            .map(|(_, id)| id)
    }
    async fn retry_failed_result(
        &self,
        failures: &dyn FailedJobProviderInterface,
        key: &str,
        job: &FailedJob,
    ) -> QueueResult<(bool, Option<String>)> {
        if self.observes_worker_event(JobRetryRequested::NAME)? {
            self.emit_control_event(JobRetryRequested { job: job.clone() })
                .await?;
        }
        let configured = self.resolve(Some(&job.connection))?;
        let mut payload = job.payload.clone();
        self.registry.refresh_retry_deadline(&mut payload).await?;
        let payload = payload.renewed()?;
        let claim = if key == job.id {
            Cow::Borrowed(job)
        } else {
            Cow::Owned(FailedJob {
                id: key.to_owned(),
                ..job.clone()
            })
        };
        let queue = Some(job.queue.as_str());
        if let BatchBackend::Database(connection) = &configured.batch_backend {
            let mut transaction = connection.beginTransaction().await.map_err(backend)?;
            match failures
                .forget_retry_in_transaction(&claim, &mut transaction)
                .await?
            {
                Some(claimed) => {
                    let id = if claimed {
                        configured
                            .queue
                            .push_in_transaction(&payload, queue, None, &mut transaction)
                            .await?
                    } else {
                        None
                    };
                    transaction.commit().await.map_err(backend)?;
                    return Ok((claimed, id));
                }
                None => transaction.rollBack(None).await.map_err(backend)?,
            }
        }
        let id = configured.queue.push(&payload, queue).await?;
        failures.forget_retry(&claim).await?;
        Ok((true, id))
    }

    fn capture_context(&self, payload: &mut Payload) -> QueueResult<()> {
        self.context_snapshot()?(payload);
        Ok(())
    }

    pub(super) fn context_snapshot(&self) -> QueueResult<impl FnOnce(&mut Payload) + Send + use<>> {
        use crate::log::context::Context;
        let source = if let Some((owner, active)) = self
            .owner
            .upgrade()
            .zip(crate::support::facades::current_container())
            && Arc::ptr_eq(&owner, &active)
            && let Ok(context) = Context::repository()
        {
            Some((context, Context::hooks().map_err(backend)?))
        } else {
            None
        };
        Ok(move |payload: &mut Payload| {
            if let Some((context, hooks)) = source {
                payload.context = hooks.dehydrate(&context);
            }
        })
    }

    pub fn batch_repository(&self) -> QueueResult<DatabaseBatchRepository> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let manager = owner.make::<DatabaseManager>()?;
        let name = self.config.get("queue.batching.database");
        let connection = manager
            .connection(name.as_ref().and_then(serde_json::Value::as_str))
            .map_err(backend)?;
        let table = self
            .config
            .get("queue.batching.table")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "job_batches".into());
        DatabaseBatchRepository::new(connection, table)
    }

    pub(crate) fn register_chain_callback(
        &self,
        name: String,
        callback: ChainCallback,
    ) -> QueueResult<()> {
        use std::collections::hash_map::Entry;
        if name.trim().is_empty() {
            return Err(QueueError::InvalidConfiguration);
        }
        let mut callbacks = self
            .chain_callbacks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Entry::Vacant(entry) = callbacks.entry(name) else {
            return Err(QueueError::InvalidConfiguration);
        };
        entry.insert(callback);
        Ok(())
    }

    pub(crate) fn chain_callback(&self, name: &str) -> QueueResult<ChainCallback> {
        self.chain_callbacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .copied()
            .ok_or_else(|| QueueError::UnknownChainCallback(name.into()))
    }

    pub(super) async fn run_chain_catches(&self, job: &Payload, cause: &QueueError) {
        self.run_chain_callbacks(job, cause, job.chain_catches())
            .await;
    }

    pub(super) async fn run_chain_callbacks(
        &self,
        job: &Payload,
        cause: &QueueError,
        names: &[String],
    ) {
        for name in names {
            let result = match self.chain_callback(name) {
                Ok(callback) => callback(job, cause).await,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                let context = serde_json::Map::from_iter([
                    ("job_uuid".into(), job.uuid().into()),
                    ("callback".into(), name.as_str().into()),
                    ("error".into(), error.to_string().into()),
                ]);
                let _ = crate::log::Log::error("Chain callback failed", &context);
            }
        }
    }

    pub(crate) fn register_batch_callback(
        &self,
        name: String,
        callback: BatchCallback,
    ) -> QueueResult<()> {
        use std::collections::hash_map::Entry;
        if name.trim().is_empty() {
            return Err(QueueError::InvalidConfiguration);
        }
        let mut registry = self
            .batch_callbacks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match registry.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(callback);
                Ok(())
            }
            Entry::Occupied(_) => Err(QueueError::InvalidConfiguration),
        }
    }

    fn batch_callback(&self, name: &str) -> QueueResult<BatchCallback> {
        self.batch_callbacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .copied()
            .ok_or_else(|| QueueError::UnknownBatchCallback(name.into()))
    }

    pub(crate) async fn run_batch_callbacks(
        &self,
        batch: &Batch,
        phases: &[BatchPhase],
        failure: Option<&str>,
    ) {
        for phase in phases {
            for name in phase.names(batch) {
                let result = match self.batch_callback(name) {
                    Ok(callback) => {
                        callback(batch, failure.filter(|_| phase.takes_failure())).await
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    let context = serde_json::Map::from_iter([
                        ("batch_id".into(), batch.id.as_str().into()),
                        ("phase".into(), phase.key().into()),
                        ("callback".into(), name.into()),
                        ("error".into(), error.to_string().into()),
                    ]);
                    let _ = crate::log::Log::error("Batch callback failed", &context);
                }
            }
        }
    }

    pub(crate) fn validate_chain_routes<'a>(
        &self,
        jobs: impl IntoIterator<Item = &'a Payload>,
        options: &DispatchOptions,
    ) -> QueueResult<()> {
        for job in jobs {
            let destination = job.first_chain_options(DispatchOptions {
                connection: options.connection.clone(),
                queue: options.queue.clone(),
                ..Default::default()
            });
            let configured = self.resolve(destination.connection.as_deref())?;
            configured.queue_name(destination.queue.as_deref())?;
        }
        Ok(())
    }

    pub(crate) fn validate_chained_batches<'a>(
        &self,
        jobs: impl IntoIterator<Item = &'a Payload>,
        options: &DispatchOptions,
    ) -> QueueResult<()> {
        let mut batches = jobs
            .into_iter()
            .filter_map(|job| job.chained_batch.as_deref())
            .peekable();
        if batches.peek().is_none() {
            return Ok(());
        }
        let mut outer = serde_json::Map::new();
        if let Some(connection) = &options.connection {
            outer.insert("connection".into(), connection.as_str().into());
        }
        if let Some(queue) = &options.queue {
            outer.insert("queue".into(), queue.as_str().into());
        }
        let destination = self.resolve_batch_destination(&outer)?;
        let queue = queue_name(options.queue.as_deref(), "default")?;
        for batch in batches {
            if batch.pending.jobs.is_empty() {
                return Err(QueueError::InvalidPayload);
            }
            self.resolve_batch_destination(
                &batch.options(destination.queue.getConnectionName(), queue),
            )?;
        }
        Ok(())
    }

    pub(super) async fn execute_chained_batch(
        &self,
        payload: &Payload,
        connection: &str,
        queue: &str,
    ) -> QueueResult<()> {
        let spec = payload
            .chained_batch
            .as_deref()
            .ok_or(QueueError::InvalidPayload)?;
        let options = spec.options(connection, queue);
        let next = payload.next_chain_job();
        let continuation = BatchContinuation {
            connection: connection.to_owned(),
            queue: queue.to_owned(),
            next,
            catches: payload.chain_catches().to_vec(),
        };
        self.dispatch_batch_definition(
            spec.pending.name.clone(),
            options,
            spec.pending.jobs.clone(),
            Some(&continuation),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn dispatch_batch(
        &self,
        name: String,
        options: serde_json::Map<String, serde_json::Value>,
        jobs: Vec<BatchJob>,
    ) -> QueueResult<Batch> {
        self.dispatch_batch_definition(name, options, jobs, None)
            .await
    }

    async fn dispatch_batch_definition(
        &self,
        name: String,
        options: serde_json::Map<String, serde_json::Value>,
        mut jobs: Vec<BatchJob>,
        continuation: Option<&BatchContinuation>,
    ) -> QueueResult<Batch> {
        let count = prepare_batch_jobs(&mut jobs, false)?;
        let repository = self.batch_repository()?;
        let configured = self.resolve_batch_destination(&options)?;
        let mut batch = new_batch(name, options, 0)?;
        for phase in BatchPhase::ALL {
            for callback in phase.names(&batch) {
                self.batch_callback(callback)?;
            }
        }
        self.run_batch_callbacks(&batch, &[BatchPhase::Before], None)
            .await;
        batch.total_jobs = count;
        batch.pending_jobs = count;

        if let Some(continuation) = continuation {
            batch.options.insert(
                BatchContinuation::OPTION.into(),
                serde_json::to_value(continuation).map_err(|_| QueueError::InvalidPayload)?,
            );
        }
        let mut transaction = repository.begin().await?;
        repository
            .store_in_transaction(&mut transaction, &batch)
            .await?;
        self.enqueue_batch_jobs(&repository, &mut transaction, &configured, &batch, jobs)
            .await?;
        transaction.commit().await.map_err(backend)?;
        crate::bus::events::emit(crate::bus::events::BatchDispatched {
            batch: batch.clone(),
        })
        .await;
        Ok(batch)
    }

    pub(crate) async fn append_batch(
        &self,
        id: &str,
        mut jobs: Vec<BatchJob>,
    ) -> QueueResult<Option<Batch>> {
        let count = prepare_batch_jobs(&mut jobs, true)?;
        let repository = self.batch_repository()?;
        let mut transaction = repository.begin().await?;
        let Some(batch) = repository
            .append_in_transaction(&mut transaction, id, count)
            .await?
        else {
            transaction.rollBack(None).await.map_err(backend)?;
            return Ok(None);
        };
        if jobs.is_empty() {
            transaction.commit().await.map_err(backend)?;
            return Ok(Some(batch));
        }
        let configured = self.resolve_batch_destination(&batch.options)?;
        self.enqueue_batch_jobs(&repository, &mut transaction, &configured, &batch, jobs)
            .await?;
        transaction.commit().await.map_err(backend)?;
        Ok(Some(batch))
    }

    fn resolve_batch_destination(
        &self,
        options: &serde_json::Map<String, serde_json::Value>,
    ) -> QueueResult<Arc<ConfiguredQueue>> {
        let configured = self.resolve(
            options
                .get("connection")
                .and_then(serde_json::Value::as_str),
        )?;
        match &configured.batch_backend {
            BatchBackend::Database(_)
            | BatchBackend::Redis(_)
            | BatchBackend::Beanstalkd
            | BatchBackend::Sqs(_) => {}
            _ => return Err(QueueError::InvalidBatchConnection),
        }
        configured.queue_name(options.get("queue").and_then(Value::as_str))?;
        Ok(configured)
    }

    async fn enqueue_batch_jobs(
        &self,
        repository: &DatabaseBatchRepository,
        transaction: &mut Transaction,
        configured: &ConfiguredQueue,
        batch: &Batch,
        jobs: Vec<BatchJob>,
    ) -> QueueResult<()> {
        let mut heads = Vec::with_capacity(jobs.len());
        for entry in jobs {
            let mut head = entry.into_head(&batch.id)?;
            self.capture_context(&mut head)?;
            heads.push(head);
        }
        let queue = batch
            .options
            .get("queue")
            .and_then(serde_json::Value::as_str);
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let options = DispatchOptions {
            queue: queue.map(str::to_owned),
            ..Default::default()
        };
        if let BatchBackend::Sqs(sqs) = &configured.batch_backend {
            for job in &heads {
                emit_queueing(&owner, configured.queue.as_ref(), job, &options).await?;
            }
            return sqs.send_batch(configured.queue_name(queue)?, &heads).await;
        }
        for job in &heads {
            emit_queueing(&owner, configured.queue.as_ref(), job, &options).await?;
            if let BatchBackend::Database(connection) = &configured.batch_backend
                && repository.uses_connection(connection)
            {
                configured
                    .queue
                    .push_in_transaction(job, queue, None, transaction)
                    .await?;
            } else {
                configured.queue.push(job, queue).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn dispatch_batch_continuation(&self, continuation: BatchContinuation) {
        if let Some(next) = continuation.next {
            let options = next.chain_options(&continuation.connection, &continuation.queue);
            let result = async {
                use crate::log::context::Context;
                let context = Context::repository().map_err(backend)?;
                Context::hooks()
                    .map_err(backend)?
                    .hydrate(&next.context, &context);
                self.dispatch(next, options, None).await
            }
            .await;
            if let Err(error) = result {
                let context = serde_json::Map::from_iter([
                    ("connection".into(), continuation.connection.into()),
                    ("error".into(), error.to_string().into()),
                ]);
                let _ = crate::log::Log::error("Batch continuation failed", &context);
            }
        }
    }

    fn resolve(&self, name: Option<&str>) -> QueueResult<Arc<ConfiguredQueue>> {
        let default;
        let name = match name {
            Some(name) => name,
            None => {
                default = self
                    .config
                    .get("queue.default")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "sync".into());
                &default
            }
        };
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(connection) = connections.get(name) {
            return Ok(connection.clone());
        }
        let raw = self
            .config
            .get("queue.connections")
            .and_then(|connections| connections.get(name).cloned());
        let raw = match raw {
            Some(raw) => raw,
            None if name == "sync" => serde_json::json!({"driver": "sync"}),
            None => return Err(QueueError::UnknownConnection { name: name.into() }),
        };
        let settings = Settings::deserialize(&raw).map_err(|_| QueueError::InvalidConfiguration)?;
        let connector = self
            .connectors
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&settings.driver)
            .cloned();
        if let Some(connector) = connector {
            drop(connections);
            let owner = self
                .owner
                .upgrade()
                .ok_or(QueueError::InvalidConfiguration)?;
            let queue = connector(&owner, name, &raw)?;
            if queue.getConnectionName() != name
                || settings
                    .queue
                    .as_deref()
                    .is_some_and(|configured| configured != queue.default_queue())
            {
                return Err(QueueError::InvalidConfiguration);
            }
            queue_name(None, queue.default_queue())?;
            let configured = Arc::new(ConfiguredQueue {
                default_queue: queue.default_queue().into(),
                queue,
                after_commit: settings.after_commit,
                capture_context_on_dispatch: true,
                synchronous: settings.driver == "sync",
                batch_backend: BatchBackend::Unsupported,
            });
            let mut connections = self
                .connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let resolved = connections
                .entry(name.into())
                .or_insert_with(|| configured.clone())
                .clone();
            drop(connections);
            return Ok(resolved);
        }
        let (queue, batch_backend) = self.make(name, &settings, &raw)?;
        let configured = Arc::new(ConfiguredQueue {
            queue,
            after_commit: settings.after_commit,
            capture_context_on_dispatch: !matches!(
                settings.driver.as_str(),
                "deferred" | "background"
            ),
            synchronous: matches!(
                settings.driver.as_str(),
                "sync" | "null" | "deferred" | "background"
            ),
            default_queue: settings.queue.clone().unwrap_or_else(|| "default".into()),
            batch_backend,
        });
        connections.insert(name.into(), configured.clone());
        Ok(configured)
    }

    fn make(
        &self,
        name: &str,
        settings: &Settings,
        raw: &Value,
    ) -> QueueResult<(Arc<dyn Contract>, BatchBackend)> {
        let registry = self.registry.clone();
        let queue = queue_name(settings.queue.as_deref(), "default")?.to_owned();
        let retry_after = settings.retry_after.unwrap_or(90);
        if !(1..=i64::from(i32::MAX)).contains(&retry_after) {
            return Err(QueueError::InvalidConfiguration);
        }
        let driver: (Arc<dyn Contract>, BatchBackend) = match settings.driver.as_str() {
            "sqs" => {
                let options = super::sqs_queue::Options::deserialize(raw)
                    .map_err(|_| QueueError::InvalidConfiguration)?;
                let sqs = Arc::new(super::sqs_queue::SqsQueue::new(
                    name.into(),
                    queue,
                    registry,
                    options,
                    |store| {
                        let owner = self
                            .owner
                            .upgrade()
                            .ok_or(QueueError::InvalidConfiguration)?;
                        owner.make::<CacheManager>()?.store(store).map_err(backend)
                    },
                )?);
                (sqs.clone(), BatchBackend::Sqs(sqs))
            }
            "null" | "deferred" | "background" => {
                use super::lifecycle_queue::{Lifecycle, LifecycleQueue};
                let lifecycle = match settings.driver.as_str() {
                    "null" => Lifecycle::Null,
                    "deferred" => Lifecycle::Deferred,
                    _ => Lifecycle::Background,
                };
                (
                    Arc::new(LifecycleQueue {
                        name: name.into(),
                        queue,
                        registry,
                        lifecycle,
                    }),
                    BatchBackend::Unsupported,
                )
            }
            "failover" => (
                Arc::new(super::failover_queue::FailoverQueue::new(
                    name,
                    queue,
                    settings.connections.clone().unwrap_or_default(),
                    registry,
                )?),
                BatchBackend::Unsupported,
            ),
            "sync" => (
                Arc::new(SyncQueue {
                    name: name.into(),
                    registry,
                }),
                BatchBackend::Unsupported,
            ),
            "database" => {
                let owner = self
                    .owner
                    .upgrade()
                    .ok_or(QueueError::InvalidConfiguration)?;
                let manager = owner.make::<DatabaseManager>()?;
                let connection = manager
                    .connection(settings.connection.as_deref())
                    .map_err(backend)?;
                let table = settings.table.clone().unwrap_or_else(|| "jobs".into());
                connection.queue_table(&table).map_err(backend)?;
                (
                    Arc::new(DatabaseQueue {
                        name: name.into(),
                        queue,
                        table,
                        retry_after,
                        connection: connection.clone(),
                        registry,
                    }),
                    BatchBackend::Database(connection),
                )
            }
            "beanstalkd" => {
                super::beanstalkd_queue::tube(&queue)?;
                let host = settings.host.clone().unwrap_or_else(|| "localhost".into());
                let port = settings.port.unwrap_or(11300);
                if host.trim().is_empty() || port == 0 {
                    return Err(QueueError::InvalidConfiguration);
                }
                (
                    Arc::new(super::beanstalkd_queue::BeanstalkdQueue {
                        name: name.into(),
                        queue,
                        host,
                        port,
                        retry_after: u32::try_from(retry_after)
                            .map_err(|_| QueueError::InvalidConfiguration)?,
                        block_for: settings.block_for.unwrap_or_default(),
                        registry,
                    }),
                    BatchBackend::Beanstalkd,
                )
            }
            "redis" => {
                let owner = self
                    .owner
                    .upgrade()
                    .ok_or(QueueError::InvalidConfiguration)?;
                let manager = owner.make::<RedisManager>()?;
                let connection = manager
                    .connection(settings.connection.as_deref())
                    .map_err(backend)?;
                let redis = Arc::new(RedisQueue {
                    name: name.into(),
                    queue,
                    retry_after,
                    block_for: settings.block_for,
                    connection,
                    registry,
                });
                (redis.clone(), BatchBackend::Redis(redis))
            }
            driver => return Err(QueueError::UnsupportedDriver(driver.into())),
        };
        Ok(driver)
    }
}

impl Factory for QueueManager {
    fn connection(&self, name: Option<&str>) -> QueueResult<Arc<dyn Contract>> {
        self.connection(name)
    }
}

impl ConfiguredQueue {
    fn queue_name<'a>(&'a self, selected: Option<&'a str>) -> QueueResult<&'a str> {
        match &self.batch_backend {
            BatchBackend::Redis(redis) => redis.queue_name(selected),
            _ => queue_name(selected, &self.default_queue),
        }
    }
}

fn prepare_batch_jobs(jobs: &mut [BatchJob], renew: bool) -> QueueResult<u32> {
    let mut count = 0_u32;
    let mut ids = HashSet::new();
    for entry in jobs {
        let mut members = entry.members().peekable();
        if members.peek().is_none() {
            return Err(QueueError::InvalidPayload);
        }
        for job in members {
            if !job.is_chain_member() {
                return Err(QueueError::InvalidPayload);
            }
            if renew {
                job.uuid = new_id()?;
                job.delivery_id = None;
                job.context = Default::default();
            } else if !ids.insert(job.uuid()) {
                return Err(QueueError::InvalidPayload);
            }
            count = count.checked_add(1).ok_or(QueueError::InvalidPayload)?;
        }
    }
    i32::try_from(count).map_err(|_| QueueError::InvalidPayload)?;
    Ok(count)
}

async fn emit_queueing(
    owner: &Arc<Container>,
    queue: &dyn Contract,
    payload: &Payload,
    options: &DispatchOptions,
) -> QueueResult<()> {
    let dispatcher = owner.make::<Dispatcher>()?;
    if dispatcher.observes(super::events::JobQueueing::NAME) {
        let event = super::events::JobQueueing {
            connection_name: queue.getConnectionName().into(),
            queue: super::queue_name(options.queue.as_deref(), queue.default_queue())?.into(),
            job: payload.clone(),
        };
        application_scope(
            owner.clone(),
            dispatcher.dispatch(Arc::new(event), &mut EventContext::new()),
        )
        .await
        .map_err(backend)?;
    }
    Ok(())
}

async fn dispatch(
    queue: &dyn Contract,
    payload: &Payload,
    options: &DispatchOptions,
    owner: &Arc<Container>,
) -> QueueResult<DispatchResult> {
    emit_queueing(owner, queue, payload, options).await?;
    let selected = options.queue.as_deref();
    match options.delay {
        Some(delay) => queue.later(delay, payload, selected).await,
        None => queue.push(payload, selected).await,
    }
    .map(dispatch_result)
}

async fn dispatch_unique(
    queue: &dyn Contract,
    mut payload: Payload,
    options: &DispatchOptions,
    owner: Arc<Container>,
    unique: Option<super::UniqueJob>,
) -> QueueResult<DispatchResult> {
    let Some(policy) = unique else {
        return dispatch(queue, &payload, options, &owner).await;
    };
    if !super::unique_job::acquire(&owner, &mut payload, policy).await? {
        let dispatcher = owner.make::<Dispatcher>()?;
        if dispatcher.observes(UniqueJobSkipped::NAME) {
            application_scope(owner, async {
                dispatcher
                    .dispatch(
                        Arc::new(UniqueJobSkipped { job: payload }),
                        &mut EventContext::new(),
                    )
                    .await
            })
            .await
            .map_err(backend)?;
        }
        return Ok(DispatchResult::Duplicate);
    }
    let result = dispatch(queue, &payload, options, &owner).await;
    if !matches!(result, Ok(DispatchResult::Queued(_))) {
        let released = super::unique_job::release(&owner, &payload).await;
        return result.and(released.map(|()| DispatchResult::Completed));
    }
    result
}

fn dispatch_result(id: Option<String>) -> DispatchResult {
    id.map_or(DispatchResult::Completed, DispatchResult::Queued)
}
