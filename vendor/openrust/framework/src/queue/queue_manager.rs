use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock, Weak},
};

use serde::Deserialize;

use crate::{
    bus::{Batch, BatchCallback, BatchPhase, DatabaseBatchRepository, new_batch},
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
    queue_name,
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
    database: Option<Connection>,
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
        if let Some(connection) = &configured.database {
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
        let count = u32::try_from(jobs.len())
            .ok()
            .filter(|count| i32::try_from(*count).is_ok())
            .ok_or(QueueError::InvalidPayload)?;
        let mut ids = HashSet::with_capacity(jobs.len());
        for job in &jobs {
            if job.batch_id.is_some() || !ids.insert(job.uuid()) {
                return Err(QueueError::InvalidPayload);
            }
        }
        let configured = self.resolve(
            options
                .get("connection")
                .and_then(serde_json::Value::as_str),
        )?;
        let connection = configured
            .database
            .as_ref()
            .ok_or(QueueError::InvalidBatchConnection)?;
        let repository = self.batch_repository()?;
        let queue = options
            .get("queue")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if !repository.uses_connection(connection) {
            return Err(QueueError::InvalidBatchConnection);
        }
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

        let mut transaction = repository.begin().await?;
        let result = async {
            repository
                .store_in_transaction(&mut transaction, &batch)
                .await?;
            for job in &mut jobs {
                self.capture_context(job)?;
                job.batch_id = Some(batch.id.clone());
                configured
                    .queue
                    .push_in_transaction(job, queue.as_deref(), None, &mut transaction)
                    .await?;
            }
            Ok(batch)
        }
        .await;
        match result {
            Ok(batch) => {
                transaction.commit().await.map_err(backend)?;
                crate::bus::events::emit(crate::bus::events::BatchDispatched {
                    batch: batch.clone(),
                })
                .await;
                Ok(batch)
            }
            Err(error) => {
                transaction.rollBack(None).await.map_err(backend)?;
                Err(error)
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
        let (queue, database) = self.make(name, &settings)?;
        let configured = Arc::new(ConfiguredQueue {
            queue,
            after_commit: settings.after_commit,
            database,
        });
        connections.insert(name.into(), configured.clone());
        Ok(configured)
    }

    fn make(
        &self,
        name: &str,
        settings: &Settings,
    ) -> QueueResult<(Arc<dyn Contract>, Option<Connection>)> {
        let registry = self.registry.clone();
        let queue = queue_name(settings.queue.as_deref(), "default")?.to_owned();
        let retry_after = settings.retry_after.unwrap_or(90);
        if !(1..=i64::from(i32::MAX)).contains(&retry_after) {
            return Err(QueueError::InvalidConfiguration);
        }
        let driver: (Arc<dyn Contract>, Option<Connection>) = match settings.driver.as_str() {
            "sync" => (
                Arc::new(SyncQueue {
                    name: name.into(),
                    registry,
                }),
                None,
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
                    Some(connection),
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
                (
                    Arc::new(RedisQueue {
                        name: name.into(),
                        queue,
                        retry_after,
                        block_for: settings.block_for,
                        connection,
                        registry,
                    }),
                    None,
                )
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
