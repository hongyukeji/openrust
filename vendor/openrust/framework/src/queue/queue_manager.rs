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
        DatabaseBatchRepository, PublicationKey, new_batch,
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
    support::facades::application_scope,
};

use super::{
    DatabaseQueue, DispatchOptions, DispatchResult, JobPopCallback, Payload, Queue, QueueError,
    QueueResult, RedisQueue, SyncQueue, backend,
    events::{JobRetryRequested, QueuePaused, QueueResumed, QueuesPaused, QueuesResumed},
    failed::{FailedJob, FailedJobProviderInterface},
    new_id, queue_name,
    worker_control::WorkerControl,
};

/// Resolves and caches configured queue connections for one application.
pub struct QueueManager {
    config: Arc<dyn Repository>,
    owner: Weak<Container>,
    registry: Arc<Queue>,
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
    synchronous: bool,
    default_queue: String,
    batch_backend: BatchBackend,
}

enum BatchBackend {
    Database(Connection),
    Redis(Arc<RedisQueue>),
    Unsupported,
}

#[derive(Deserialize)]
struct Settings {
    driver: String,
    connection: Option<String>,
    table: Option<String>,
    queue: Option<String>,
    retry_after: Option<i64>,
    block_for: Option<u32>,
    #[serde(default)]
    after_commit: bool,
}

impl QueueManager {
    pub(super) fn new(config: Arc<dyn Repository>, owner: Weak<Container>) -> Self {
        Self {
            config,
            registry: Arc::new(Queue::new(owner.clone())),
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

    pub(crate) fn register_listeners(
        &self,
        registrations: Vec<(String, Arc<dyn JobHandler>)>,
    ) -> QueueResult<()> {
        self.registry.register_listeners(registrations)
    }

    /// Sets or removes a custom reservation callback for a worker name.
    #[allow(non_snake_case, reason = "Matches Laravel Worker::popUsing")]
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

    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
    pub fn withoutInterruptionPolling(&self) {
        self.interruption_polling.store(false, Ordering::Relaxed);
    }

    /// Pauses one queue until it is resumed.
    pub async fn pause(&self, connection: &str, queue: &str) -> QueueResult<()> {
        self.set_queue_pause(connection, queue, None).await
    }

    /// Pauses one queue until the supplied cache deadline or duration expires.
    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
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
    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
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
    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
    pub async fn resumeAll(&self) -> QueueResult<()> {
        self.worker_control()?.resume(None).await?;
        self.emit_control_event(QueuesResumed {}).await
    }

    /// Checks both the global and individual pause state.
    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
    pub async fn isPaused(&self, connection: &str, queue: &str) -> QueueResult<bool> {
        let control = self.worker_control()?;
        Ok(control.all_paused().await? || control.queue_paused(connection, queue).await?)
    }

    /// Returns the paused queues in the requested order.
    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
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
        let dispatcher = owner
            .make::<Dispatcher>()
            .map_err(QueueError::Unavailable)?;
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

    pub(super) fn clear_prohibited(&self) -> bool {
        self.clear_prohibited.load(Ordering::Relaxed)
    }

    pub(super) fn worker_control(&self) -> QueueResult<WorkerControl> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let cache = owner
            .make::<CacheManager>()
            .map_err(QueueError::Unavailable)?;
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
        Ok((configured.queue.get_connection_name().into(), queue.into()))
    }

    pub async fn dispatch(
        &self,
        mut payload: Payload,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
        self.capture_context(&mut payload)?;
        let configured = self.resolve(options.connection.as_deref())?;
        let queue = configured.queue.clone();
        payload.unique_lock = None;
        let unique = if payload.batch_id.is_none() && payload.chained_batch.is_none() {
            self.registry
                .handler(payload.name())
                .and_then(|handler| handler.uniqueJob(&payload))
        } else {
            None
        };
        let owner = unique
            .as_ref()
            .map(|_| self.owner.upgrade().ok_or(QueueError::InvalidConfiguration))
            .transpose()?;
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
                            dispatch_unique(&*queue, payload, &options, owner.zip(unique))
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
            return queue
                .push_in_transaction(&payload, selected, options.delay, transaction)
                .await
                .map(dispatch_result);
        }
        dispatch_unique(&*queue, payload, &options, owner.zip(unique)).await
    }

    pub(crate) async fn retry_failed(
        &self,
        failures: &dyn FailedJobProviderInterface,
        key: &str,
        job: &FailedJob,
    ) -> QueueResult<bool> {
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
                    if claimed {
                        configured
                            .queue
                            .push_in_transaction(&payload, queue, None, &mut transaction)
                            .await?;
                    }
                    transaction.commit().await.map_err(backend)?;
                    return Ok(claimed);
                }
                None => transaction.rollBack(None).await.map_err(backend)?,
            }
        }
        configured.queue.push(&payload, queue).await?;
        failures.forget_retry(&claim).await?;
        Ok(true)
    }

    fn capture_context(&self, payload: &mut Payload) -> QueueResult<()> {
        if let Some((owner, active)) = self
            .owner
            .upgrade()
            .zip(crate::support::facades::current_container())
            && Arc::ptr_eq(&owner, &active)
            && let Ok(context) = crate::log::context::Context::repository()
        {
            payload.context = crate::log::context::Context::hooks()
                .map_err(backend)?
                .dehydrate(&context);
        }
        Ok(())
    }

    pub(crate) fn batch_repository(&self) -> QueueResult<DatabaseBatchRepository> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let manager = owner
            .make::<DatabaseManager>()
            .map_err(QueueError::Unavailable)?;
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

    pub(super) fn chain_successor(
        &self,
        source: &str,
        source_queue: &str,
        next: Payload,
    ) -> QueueResult<(Payload, Option<String>, Option<super::Delay>)> {
        if next.chain_route.is_none() && next.dispatch_options.is_none() {
            return Ok((next, None, None));
        }
        let options = next.chain_options(source, source_queue);
        let target = self.resolve(options.connection.as_deref())?;
        let target_name = target.queue.get_connection_name();
        let queue = target.queue_name(options.queue.as_deref())?;
        if target_name == source {
            let queue = (queue != source_queue).then(|| queue.to_owned());
            return Ok((next, queue, options.delay));
        }
        Ok((
            next.chain_delivery(options, target_name.to_owned())?,
            None,
            None,
        ))
    }

    pub(super) async fn forward_chain(&self, relay: &Payload) -> QueueResult<Option<QueueError>> {
        let delivery = relay
            .chain_delivery
            .as_deref()
            .ok_or(QueueError::InvalidPayload)?;
        let target = self.resolve(Some(&delivery.connection))?;
        match &target.batch_backend {
            BatchBackend::Redis(redis) => {
                redis
                    .forward_chain(
                        relay.uuid(),
                        &delivery.job,
                        delivery.queue.as_deref(),
                        delivery.delay,
                    )
                    .await?;
                Ok(None)
            }
            BatchBackend::Database(connection) => {
                let mut transaction = connection.beginTransaction().await.map_err(backend)?;
                let marker = [("id".into(), relay.uuid().into())].into_iter().collect();
                match transaction
                    .table("job_chain_deliveries")
                    .insert(&[marker])
                    .await
                {
                    Ok(_) => {}
                    Err(error) if is_duplicate(&error) => {
                        transaction.rollBack(None).await.map_err(backend)?;
                        return Ok(None);
                    }
                    Err(error) => return Err(backend(error)),
                }
                target
                    .queue
                    .push_in_transaction(
                        &delivery.job,
                        delivery.queue.as_deref(),
                        delivery.delay,
                        &mut transaction,
                    )
                    .await?;
                transaction.commit().await.map_err(backend)?;
                Ok(None)
            }
            BatchBackend::Unsupported => {
                use crate::log::context::Context;
                let repository = Context::repository().map_err(backend)?;
                Context::hooks()
                    .map_err(backend)?
                    .hydrate(&delivery.job.context, &repository);
                Ok(target
                    .queue
                    .push(&delivery.job, delivery.queue.as_deref())
                    .await
                    .err())
            }
        }
    }

    pub(super) async fn acknowledge_chain_delivery(&self, relay: &Payload) -> QueueResult<()> {
        let delivery = relay
            .chain_delivery
            .as_deref()
            .ok_or(QueueError::InvalidPayload)?;
        let target = self.resolve(Some(&delivery.connection))?;
        match &target.batch_backend {
            BatchBackend::Redis(redis) => redis.acknowledge_chain_delivery(relay.uuid()).await,
            BatchBackend::Database(connection) => {
                connection
                    .table("job_chain_deliveries")
                    .r#where("id", "=", relay.uuid())
                    .whereNull("acknowledged_at")
                    .update(
                        [(
                            "acknowledged_at".into(),
                            chrono::Utc::now().timestamp().into(),
                        )]
                        .into_iter()
                        .collect(),
                    )
                    .await
                    .map_err(backend)?;
                Ok(())
            }
            BatchBackend::Unsupported => Ok(()),
        }
    }

    pub(super) async fn prune_chain_deliveries(
        &self,
        queue_connection: Option<&str>,
        cutoff: i64,
    ) -> QueueResult<u64> {
        let configured = self.resolve(queue_connection)?;
        let BatchBackend::Database(connection) = &configured.batch_backend else {
            return Err(QueueError::InvalidConfiguration);
        };
        let mut total = 0;
        loop {
            let deleted = connection
                .table("job_chain_deliveries")
                .r#where("acknowledged_at", "<", cutoff)
                .orderBy("acknowledged_at", "asc")
                .orderBy("id", "asc")
                .limit(1000)
                .delete()
                .await
                .map_err(backend)?;
            total += deleted;
            if deleted == 0 {
                return Ok(total);
            }
        }
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
                &batch.options(destination.queue.get_connection_name(), queue),
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
        let next = match payload.next_chain_job() {
            Some(next) => {
                let (next, target_queue, delay) = self.chain_successor(connection, queue, next)?;
                // The batch publication writes to the wrapper's queue. Preserve a different
                // queue or delay with the same durable forwarding step used across connections.
                let next = if target_queue.is_some() || !super::chain_routing::no_delay(delay) {
                    next.chain_delivery(
                        DispatchOptions {
                            queue: target_queue.or_else(|| Some(queue.to_owned())),
                            delay,
                            ..Default::default()
                        },
                        connection.to_owned(),
                    )?
                } else {
                    next
                };
                Some(next)
            }
            None => None,
        };
        let continuation = BatchContinuation {
            id: payload.uuid().to_owned(),
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
        if let Some(continuation) = continuation {
            if let Some(existing) = repository.find(&continuation.id).await? {
                return Ok(existing);
            }
            batch.id.clone_from(&continuation.id);
        }
        for phase in BatchPhase::ALL {
            for callback in phase.names(&batch) {
                self.batch_callback(callback)?;
            }
        }
        self.run_batch_callbacks(&batch, &[BatchPhase::Before], None)
            .await;
        batch.total_jobs = count;
        batch.pending_jobs = count;

        let publication = PublicationKey::Initial(batch.id.clone());
        let mut transaction = repository.begin().await?;
        if let Err(error) = repository
            .store_in_transaction(&mut transaction, &batch)
            .await
        {
            if continuation.is_some() && is_duplicate_batch(&error) {
                transaction.rollBack(None).await.map_err(backend)?;
                return repository.find(&batch.id).await?.ok_or(error);
            }
            return Err(error);
        }
        if let Some(continuation) = continuation {
            repository
                .store_continuation(&mut transaction, continuation)
                .await?;
        }
        self.enqueue_batch_jobs(
            &repository,
            &mut transaction,
            &configured,
            &batch,
            &publication,
            jobs,
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        if configured.needs_batch_publication(&repository) {
            self.publish_after_commit(&repository, &publication).await;
        } else {
            crate::bus::events::emit(crate::bus::events::BatchDispatched {
                batch: batch.clone(),
            })
            .await;
        }
        Ok(batch)
    }

    pub(crate) async fn append_batch(
        &self,
        id: &str,
        mut jobs: Vec<BatchJob>,
    ) -> QueueResult<Option<Batch>> {
        let count = prepare_batch_jobs(&mut jobs, true)?;
        let publication = PublicationKey::Addition(new_id()?);
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
        self.enqueue_batch_jobs(
            &repository,
            &mut transaction,
            &configured,
            &batch,
            &publication,
            jobs,
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        if configured.needs_batch_publication(&repository) {
            self.publish_after_commit(&repository, &publication).await;
        }
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
            BatchBackend::Database(_) | BatchBackend::Redis(_) => {}
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
        publication: &PublicationKey,
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
        if configured.needs_batch_publication(repository) {
            repository
                .stage_publication(
                    transaction,
                    publication,
                    &batch.id,
                    configured.queue.get_connection_name(),
                    configured.queue_name(queue)?,
                    &heads,
                )
                .await?;
        } else {
            for job in &heads {
                configured
                    .queue
                    .push_in_transaction(job, queue, None, transaction)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn publish_after_commit(
        &self,
        repository: &DatabaseBatchRepository,
        publication: &PublicationKey,
    ) {
        if let Err(error) = self.publish_batch(repository, publication).await {
            let context = serde_json::Map::from_iter([
                ("publication_id".into(), publication.id().into()),
                ("error".into(), error.to_string().into()),
            ]);
            let _ = crate::log::Log::error("Batch publication is pending", &context);
        }
    }

    pub(crate) async fn publish_batch(
        &self,
        repository: &DatabaseBatchRepository,
        key: &PublicationKey,
    ) -> QueueResult<bool> {
        let mut transaction = repository.begin().await?;
        let Some(publication) = repository.claim_publication(&mut transaction, key).await? else {
            transaction.rollBack(None).await.map_err(backend)?;
            return Ok(false);
        };
        if !matches!(key, PublicationKey::Initial(_))
            && repository
                .initial_publication_pending(&mut transaction, &publication.batch_id)
                .await?
        {
            transaction.rollBack(None).await.map_err(backend)?;
            return Ok(false);
        }
        let configured = self.resolve(Some(&publication.connection))?;
        let batch = repository
            .find_in_transaction(&mut transaction, &publication.batch_id)
            .await?
            .ok_or(QueueError::InvalidPayload)?;
        if matches!(key, PublicationKey::Continuation(_)) && batch.cancelled() {
            repository
                .acknowledge_publication(&mut transaction, key)
                .await?;
            transaction.commit().await.map_err(backend)?;
            return Ok(false);
        }
        match &configured.batch_backend {
            BatchBackend::Redis(redis) => {
                let publication_id = match key {
                    PublicationKey::Continuation(id) => format!("continuation:{id}"),
                    _ => key.id().to_owned(),
                };
                redis
                    .publish_batch(&publication_id, &publication.queue, &publication.payloads)
                    .await?;
                repository
                    .acknowledge_publication(&mut transaction, key)
                    .await?;
                transaction.commit().await.map_err(backend)?;
                if matches!(key, PublicationKey::Initial(_)) {
                    crate::bus::events::emit(crate::bus::events::BatchDispatched { batch }).await;
                }
                redis.expire_publication(&publication_id).await;
            }
            BatchBackend::Database(connection) => {
                if repository.uses_connection(connection) {
                    enqueue_batch_publication(&configured, &publication, &mut transaction).await?;
                } else {
                    let mut destination = connection.beginTransaction().await.map_err(backend)?;
                    let receipt = key.database_receipt();
                    let marker = [("id".into(), receipt.as_str().into())]
                        .into_iter()
                        .collect();
                    match destination
                        .table("job_chain_deliveries")
                        .insert(&[marker])
                        .await
                    {
                        Ok(_) => {
                            enqueue_batch_publication(&configured, &publication, &mut destination)
                                .await?;
                            destination.commit().await.map_err(backend)?;
                        }
                        Err(error) if is_duplicate(&error) => {
                            destination.rollBack(None).await.map_err(backend)?;
                        }
                        Err(error) => return Err(backend(error)),
                    }
                }
                repository
                    .acknowledge_publication(&mut transaction, key)
                    .await?;
                transaction.commit().await.map_err(backend)?;
                if !repository.uses_connection(connection) {
                    let _ = connection
                        .table("job_chain_deliveries")
                        .r#where("id", "=", key.database_receipt())
                        .update(
                            [(
                                "acknowledged_at".into(),
                                chrono::Utc::now().timestamp().into(),
                            )]
                            .into_iter()
                            .collect(),
                        )
                        .await;
                }
                if matches!(key, PublicationKey::Initial(_)) {
                    crate::bus::events::emit(crate::bus::events::BatchDispatched { batch }).await;
                }
            }
            _ => return Err(QueueError::InvalidBatchConnection),
        }
        Ok(true)
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
            if queue.get_connection_name() != name
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
        let (queue, batch_backend) = self.make(name, &settings)?;
        let configured = Arc::new(ConfiguredQueue {
            queue,
            after_commit: settings.after_commit,
            synchronous: settings.driver == "sync",
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
    ) -> QueueResult<(Arc<dyn Contract>, BatchBackend)> {
        let registry = self.registry.clone();
        let queue = queue_name(settings.queue.as_deref(), "default")?.to_owned();
        let retry_after = settings.retry_after.unwrap_or(90);
        if !(1..=i64::from(i32::MAX)).contains(&retry_after) {
            return Err(QueueError::InvalidConfiguration);
        }
        let driver: (Arc<dyn Contract>, BatchBackend) = match settings.driver.as_str() {
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
                let manager = owner
                    .make::<DatabaseManager>()
                    .map_err(QueueError::Unavailable)?;
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
            "redis" => {
                let owner = self
                    .owner
                    .upgrade()
                    .ok_or(QueueError::InvalidConfiguration)?;
                let manager = owner
                    .make::<RedisManager>()
                    .map_err(QueueError::Unavailable)?;
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
    fn needs_batch_publication(&self, repository: &DatabaseBatchRepository) -> bool {
        match &self.batch_backend {
            BatchBackend::Database(connection) => !repository.uses_connection(connection),
            BatchBackend::Redis(_) => true,
            BatchBackend::Unsupported => false,
        }
    }

    fn queue_name<'a>(&'a self, selected: Option<&'a str>) -> QueueResult<&'a str> {
        match &self.batch_backend {
            BatchBackend::Redis(redis) => redis.queue_name(selected),
            _ => queue_name(selected, &self.default_queue),
        }
    }
}

async fn enqueue_batch_publication(
    configured: &ConfiguredQueue,
    publication: &crate::bus::BatchPublication,
    transaction: &mut Transaction,
) -> QueueResult<()> {
    for payload in &publication.payloads {
        configured
            .queue
            .push_in_transaction(payload, Some(&publication.queue), None, transaction)
            .await?;
    }
    Ok(())
}

fn is_duplicate(error: &DatabaseError) -> bool {
    matches!(error, DatabaseError::Driver { source: sqlx::Error::Database(error), .. } if error.is_unique_violation())
}

fn is_duplicate_batch(error: &QueueError) -> bool {
    let QueueError::Backend { source } = error else {
        return false;
    };
    source
        .downcast_ref::<DatabaseError>()
        .is_some_and(is_duplicate)
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

async fn dispatch(
    queue: &dyn Contract,
    payload: &Payload,
    options: &DispatchOptions,
) -> QueueResult<DispatchResult> {
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
    unique: Option<(Arc<Container>, super::UniqueJob)>,
) -> QueueResult<DispatchResult> {
    let Some((owner, policy)) = unique else {
        return dispatch(queue, &payload, options).await;
    };
    if !super::unique_job::acquire(&owner, &mut payload, policy).await? {
        return Ok(DispatchResult::Duplicate);
    }
    let result = dispatch(queue, &payload, options).await;
    if !matches!(result, Ok(DispatchResult::Queued(_))) {
        let released = super::unique_job::release(&owner, &payload).await;
        return result.and(released.map(|()| DispatchResult::Completed));
    }
    result
}

fn dispatch_result(id: Option<String>) -> DispatchResult {
    id.map_or(DispatchResult::Completed, DispatchResult::Queued)
}
