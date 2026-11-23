#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    contracts::queue::{Job, Queue as Contract, QueueMetrics},
    database::Transaction,
};

use super::{Delay, Payload, Queue, QueueError, QueueFuture, QueueResult, events::QueueFailedOver};

tokio::task_local! {
    static ACTIVE: Vec<String>;
}

pub(super) struct FailoverQueue {
    name: String,
    queue: String,
    connections: Vec<String>,
    registry: Arc<Queue>,
    failing: Mutex<HashSet<String>>,
}

impl FailoverQueue {
    pub(super) fn new(
        name: &str,
        queue: String,
        connections: Vec<String>,
        registry: Arc<Queue>,
    ) -> QueueResult<Self> {
        let mut names = HashSet::new();
        if connections.is_empty()
            || connections
                .iter()
                .any(|target| target == name || target.trim().is_empty() || !names.insert(target))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(Self {
            name: name.into(),
            queue,
            connections,
            registry,
            failing: Mutex::default(),
        })
    }

    async fn scoped<T>(&self, operation: impl Future<Output = QueueResult<T>>) -> QueueResult<T> {
        let mut active = ACTIVE.try_with(Clone::clone).unwrap_or_default();
        if active.contains(&self.name) {
            return Err(QueueError::InvalidConfiguration);
        }
        active.push(self.name.clone());
        ACTIVE.scope(active, operation).await
    }

    async fn send(
        &self,
        payload: &Payload,
        queue: Option<&str>,
        delay: Option<Delay>,
        mut transaction: Option<&mut Transaction>,
    ) -> QueueResult<Option<String>> {
        self.scoped(async {
            let manager = self.registry.manager()?;
            let mut failed = HashSet::new();
            let mut result = Err(QueueError::InvalidConfiguration);
            for connection in &self.connections {
                result = match manager.connection(Some(connection)) {
                    Ok(target) => {
                        if let Some(transaction) = transaction.as_deref_mut() {
                            target
                                .push_in_transaction(payload, queue, delay, transaction)
                                .await
                        } else if let Some(delay) = delay {
                            target.later(delay, payload, queue).await
                        } else {
                            target.push(payload, queue).await
                        }
                    }
                    Err(error) => Err(error),
                };
                let Err(error) = &result else { break };
                failed.insert(connection.clone());
                let notify = !self
                    .failing
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .contains(connection);
                if notify {
                    let _ = manager
                        .emit_control_event(QueueFailedOver {
                            connection: connection.clone(),
                            job: payload.clone(),
                            exception: super::events::attempt_exception_message(error),
                        })
                        .await;
                }
            }
            *self
                .failing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = failed;
            result
        })
        .await
    }

    fn primary(&self) -> QueueResult<Arc<dyn Contract>> {
        self.registry
            .manager()?
            .connection(Some(&self.connections[0]))
    }
}

impl Contract for FailoverQueue {
    fn getConnectionName(&self) -> &str {
        &self.name
    }

    fn default_queue(&self) -> &str {
        &self.queue
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.size) })
    }

    fn metrics<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(self.scoped(async move { self.primary()?.metrics(queue).await }))
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.send(payload, queue, None, None))
    }

    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.send(payload, queue, Some(delay), None))
    }

    fn push_in_transaction<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
        transaction: &'a mut Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.send(payload, queue, delay, Some(transaction)))
    }

    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.scoped(async move { self.primary()?.pop(queue).await }))
    }

    fn pop_within<'a>(
        &'a self,
        queue: Option<&'a str>,
        wait: Duration,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.scoped(async move { self.primary()?.pop_within(queue, wait).await }))
    }

    fn pop_now<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.scoped(async move { self.primary()?.pop_now(queue).await }))
    }
}
