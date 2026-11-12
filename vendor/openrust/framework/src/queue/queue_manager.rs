use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock, Weak},
};

use serde::Deserialize;

use crate::{
    bus::{Batch, BatchCallback, BatchPhase, DatabaseBatchRepository, PublicationKey, new_batch},
    container::Container,
    contracts::{
        config::Repository,
        container::Container as _,
        queue::{Factory, JobHandler, Queue as Contract},
    },
    database::{Connection, DatabaseError, DatabaseManager, Transaction},
    redis::RedisManager,
};

use super::{
    DatabaseQueue, DispatchOptions, DispatchResult, Payload, Queue, QueueError, QueueResult,
    RedisQueue, SyncQueue, backend,
    failed::{FailedJob, FailedJobProviderInterface},
    new_id, queue_name,
};

/// Resolves and caches configured queue connections for one application.
pub struct QueueManager {
    config: Arc<dyn Repository>,
    owner: Weak<Container>,
    registry: Arc<Queue>,
    connections: Mutex<HashMap<String, Arc<ConfiguredQueue>>>,
    batch_callbacks: RwLock<HashMap<String, BatchCallback>>,
}

struct ConfiguredQueue {
    queue: Arc<dyn Contract>,
    after_commit: bool,
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
            batch_callbacks: RwLock::default(),
        }
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

    pub fn connection(&self, name: Option<&str>) -> QueueResult<Arc<dyn Contract>> {
        Ok(self.resolve(name)?.queue.clone())
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
        let selected = options.queue.as_deref();
        let transaction = transaction.filter(|transaction| transaction.transactionLevel() != 0);
        if let Some(transaction) = transaction {
            if options.after_commit.unwrap_or(configured.after_commit) {
                transaction
                    .afterCommit(move || {
                        Box::pin(async move {
                            dispatch(&*queue, &payload, &options)
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
        dispatch(&*queue, &payload, &options).await
    }

    pub(crate) async fn retry_failed(
        &self,
        failures: &dyn FailedJobProviderInterface,
        job: &FailedJob,
    ) -> QueueResult<bool> {
        let configured = self.resolve(Some(&job.connection))?;
        let payload = job.payload.clone().renewed()?;
        let queue = Some(job.queue.as_str());
        if let BatchBackend::Database(connection) = &configured.batch_backend {
            let mut transaction = connection.beginTransaction().await.map_err(backend)?;
            match failures
                .forget_retry_in_transaction(job, &mut transaction)
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
        failures.forget_retry(job).await?;
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

    pub(crate) async fn dispatch_batch(
        &self,
        name: String,
        options: serde_json::Map<String, serde_json::Value>,
        mut jobs: Vec<Payload>,
    ) -> QueueResult<Batch> {
        let count = validate_batch_jobs(&jobs)?;
        let repository = self.batch_repository()?;
        let configured = self.resolve_batch_destination(&repository, &options)?;
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

        let publication = PublicationKey::Initial(batch.id.clone());
        let mut transaction = repository.begin().await?;
        repository
            .store_in_transaction(&mut transaction, &batch)
            .await?;
        self.enqueue_batch_jobs(
            &repository,
            &mut transaction,
            &configured,
            &batch,
            &publication,
            &mut jobs,
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        if matches!(configured.batch_backend, BatchBackend::Redis(_)) {
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
        mut jobs: Vec<Payload>,
    ) -> QueueResult<Option<Batch>> {
        let count = batch_job_count(&jobs)?;
        for job in &mut jobs {
            if job.batch_id.is_some() {
                return Err(QueueError::InvalidPayload);
            }
            job.uuid = new_id()?;
            job.delivery_id = None;
            job.context = Default::default();
        }
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
        let configured = self.resolve_batch_destination(&repository, &batch.options)?;
        self.enqueue_batch_jobs(
            &repository,
            &mut transaction,
            &configured,
            &batch,
            &publication,
            &mut jobs,
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        if matches!(configured.batch_backend, BatchBackend::Redis(_)) {
            self.publish_after_commit(&repository, &publication).await;
        }
        Ok(Some(batch))
    }

    fn resolve_batch_destination(
        &self,
        repository: &DatabaseBatchRepository,
        options: &serde_json::Map<String, serde_json::Value>,
    ) -> QueueResult<Arc<ConfiguredQueue>> {
        let configured = self.resolve(
            options
                .get("connection")
                .and_then(serde_json::Value::as_str),
        )?;
        match &configured.batch_backend {
            BatchBackend::Database(connection) if repository.uses_connection(connection) => {}
            BatchBackend::Redis(redis) => {
                redis.queue_name(options.get("queue").and_then(serde_json::Value::as_str))?;
            }
            _ => return Err(QueueError::InvalidBatchConnection),
        }
        Ok(configured)
    }

    async fn enqueue_batch_jobs(
        &self,
        repository: &DatabaseBatchRepository,
        transaction: &mut Transaction,
        configured: &ConfiguredQueue,
        batch: &Batch,
        publication: &PublicationKey,
        jobs: &mut [Payload],
    ) -> QueueResult<()> {
        for job in &mut *jobs {
            self.capture_context(job)?;
            job.batch_id = Some(batch.id.clone());
        }
        let queue = batch
            .options
            .get("queue")
            .and_then(serde_json::Value::as_str);
        if let BatchBackend::Redis(redis) = &configured.batch_backend {
            repository
                .stage_publication(
                    transaction,
                    publication,
                    &batch.id,
                    &redis.name,
                    redis.queue_name(queue)?,
                    jobs,
                )
                .await?;
        } else {
            for job in jobs {
                configured
                    .queue
                    .push_in_transaction(job, queue, None, transaction)
                    .await?;
            }
        }
        Ok(())
    }

    async fn publish_after_commit(
        &self,
        repository: &DatabaseBatchRepository,
        publication: &PublicationKey,
    ) {
        if let Err(error) = self.publish_batch(repository, publication).await {
            let context = serde_json::Map::from_iter([
                ("publication_id".into(), publication.id().into()),
                ("error".into(), error.to_string().into()),
            ]);
            let _ = crate::log::Log::error("Redis batch publication is pending", &context);
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
        if matches!(key, PublicationKey::Addition(_))
            && repository
                .initial_publication_pending(&mut transaction, &publication.batch_id)
                .await?
        {
            transaction.rollBack(None).await.map_err(backend)?;
            return Ok(false);
        }
        let configured = self.resolve(Some(&publication.connection))?;
        let BatchBackend::Redis(redis) = &configured.batch_backend else {
            return Err(QueueError::InvalidBatchConnection);
        };
        let batch = repository
            .find_in_transaction(&mut transaction, &publication.batch_id)
            .await?
            .ok_or(QueueError::InvalidPayload)?;
        redis
            .publish_batch(key.id(), &publication.queue, &publication.payloads)
            .await?;
        repository
            .acknowledge_publication(&mut transaction, key)
            .await?;
        transaction.commit().await.map_err(backend)?;

        if matches!(key, PublicationKey::Initial(_)) {
            crate::bus::events::emit(crate::bus::events::BatchDispatched { batch }).await;
        }
        redis.expire_publication(key.id()).await;
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
        let settings = match raw {
            Some(settings) => serde_json::from_value::<Settings>(settings)
                .map_err(|_| QueueError::InvalidConfiguration)?,
            None if name == "sync" => Settings {
                driver: "sync".into(),
                connection: None,
                table: None,
                queue: None,
                retry_after: None,
                block_for: None,
                after_commit: false,
            },
            None => return Err(QueueError::UnknownConnection { name: name.into() }),
        };
        let (queue, batch_backend) = self.make(name, &settings)?;
        let configured = Arc::new(ConfiguredQueue {
            queue,
            after_commit: settings.after_commit,
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

fn batch_job_count(jobs: &[Payload]) -> QueueResult<u32> {
    u32::try_from(jobs.len())
        .ok()
        .filter(|count| i32::try_from(*count).is_ok())
        .ok_or(QueueError::InvalidPayload)
}

fn validate_batch_jobs(jobs: &[Payload]) -> QueueResult<u32> {
    let count = batch_job_count(jobs)?;
    let mut ids = HashSet::with_capacity(jobs.len());
    for job in jobs {
        if job.batch_id.is_some() || !ids.insert(job.uuid()) {
            return Err(QueueError::InvalidPayload);
        }
    }
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

fn dispatch_result(id: Option<String>) -> DispatchResult {
    id.map_or(DispatchResult::Completed, DispatchResult::Queued)
}
