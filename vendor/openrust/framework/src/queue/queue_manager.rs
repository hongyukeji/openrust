use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use serde::Deserialize;

use crate::{
    container::Container,
    contracts::{
        config::Repository,
        container::Container as _,
        queue::{Factory, JobHandler, Queue as Contract},
    },
    database::{DatabaseError, DatabaseManager, Transaction},
    redis::RedisManager,
};

use super::{
    DatabaseQueue, DispatchOptions, DispatchResult, Payload, Queue, QueueError, QueueResult,
    RedisQueue, SyncQueue, backend, queue_name,
};

/// Resolves and caches configured queue connections for one application.
pub struct QueueManager {
    config: Arc<dyn Repository>,
    owner: Weak<Container>,
    registry: Arc<Queue>,
    connections: Mutex<HashMap<String, Arc<ConfiguredQueue>>>,
}

struct ConfiguredQueue {
    queue: Arc<dyn Contract>,
    after_commit: bool,
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
        }
    }

    pub fn register(&self, name: &str, handler: Arc<dyn JobHandler>) -> QueueResult<()> {
        self.registry.register(name, handler)
    }

    pub fn connection(&self, name: Option<&str>) -> QueueResult<Arc<dyn Contract>> {
        Ok(self.resolve(name)?.queue.clone())
    }

    pub async fn dispatch(
        &self,
        payload: Payload,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<DispatchResult> {
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
        let queue = self.make(name, &settings)?;
        let configured = Arc::new(ConfiguredQueue {
            queue,
            after_commit: settings.after_commit,
        });
        connections.insert(name.into(), configured.clone());
        Ok(configured)
    }

    fn make(&self, name: &str, settings: &Settings) -> QueueResult<Arc<dyn Contract>> {
        let registry = self.registry.clone();
        let queue = queue_name(settings.queue.as_deref(), "default")?.to_owned();
        let retry_after = settings.retry_after.unwrap_or(90);
        if !(1..=i64::from(i32::MAX)).contains(&retry_after) {
            return Err(QueueError::InvalidConfiguration);
        }
        let driver: Arc<dyn Contract> = match settings.driver.as_str() {
            "sync" => Arc::new(SyncQueue {
                name: name.into(),
                registry,
            }),
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
                Arc::new(DatabaseQueue {
                    name: name.into(),
                    queue,
                    table,
                    retry_after,
                    connection,
                    registry,
                })
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
                Arc::new(RedisQueue {
                    name: name.into(),
                    queue,
                    retry_after,
                    block_for: settings.block_for,
                    connection,
                    registry,
                })
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
