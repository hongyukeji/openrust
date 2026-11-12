use std::sync::Arc;

use crate::{
    contracts::{
        events::{Dispatcher as _, Event},
        queue::{Job, Queue as Contract, QueueMetrics},
    },
    events::{Dispatcher, EventContext},
    support::facades::scope,
};

use super::{
    Delay, JobContext, Payload, Queue, QueueError, QueueFuture, QueueResult, backend,
    events::{
        JobAttempted, JobExceptionOccurred, JobFailed, JobProcessed, JobProcessing, JobSnapshot,
        attempt_duration, attempt_exception_message,
    },
    queue_name,
};

/// Executes dispatched jobs in the current process.
pub struct SyncQueue {
    pub(super) name: String,
    pub(super) registry: Arc<Queue>,
}

impl SyncQueue {
    async fn execute(
        &self,
        payload: &Payload,
        queue: Option<&str>,
        transaction: Option<&mut crate::database::Transaction>,
    ) -> QueueResult<Option<String>> {
        let queue = queue_name(queue, "default")?;
        let manager = self.registry.manager()?;
        let (owner, dispatcher) = manager.worker_dispatcher()?;
        scope(owner, async {
            let mut context = JobContext {
                connection: self.name.clone(),
                queue: queue.into(),
                attempts: 1,
                transaction,
                ..JobContext::default()
            };
            let mut next = None;
            loop {
                let job = next.as_ref().unwrap_or(payload);
                self.execute_attempt(&dispatcher, &mut context, job).await?;
                if context.release_after.is_some() {
                    return Ok(None);
                }
                next = context
                    .chain_update
                    .take()
                    .map_or_else(|| job.next_chain_job(), |update| Some(update.next));
                let Some(successor) = &next else {
                    return Ok(None);
                };
                if successor.chain_route.is_some() || successor.dispatch_options.is_some() {
                    let mut options = successor.chain_options(&self.name, &context.queue);
                    let connection = manager.connection(options.connection.as_deref())?;
                    if connection.get_connection_name() != self.name {
                        options.connection = Some(connection.get_connection_name().to_owned());
                        let successor = next.take().expect("successor is present");
                        manager
                            .dispatch(successor, options, context.transaction())
                            .await?;
                        return Ok(None);
                    }
                    context.queue = queue_name(options.queue.as_deref(), "default")?.to_owned();
                }
            }
        })
        .await
    }

    async fn execute_attempt(
        &self,
        dispatcher: &Dispatcher,
        context: &mut JobContext<'_>,
        payload: &Payload,
    ) -> QueueResult<()> {
        let mut duration = None;
        let execution = async {
            emit(dispatcher, JobProcessing::NAME, || JobProcessing {
                connection_name: self.name.clone(),
                job: snapshot(payload),
            })
            .await?;
            let started = tokio::time::Instant::now();
            let execution = self.registry.fire(context, payload).await;
            let elapsed = attempt_duration(started.elapsed());
            duration = Some(elapsed);
            execution?;
            emit(dispatcher, JobProcessed::NAME, || JobProcessed {
                connection_name: self.name.clone(),
                job: snapshot(payload),
                duration: elapsed,
            })
            .await
        }
        .await;
        let (exception, outcome) = match execution {
            Ok(()) => (None, Ok(())),
            Err(error) => {
                let message = attempt_exception_message(&error);
                let failure = self
                    .fail(dispatcher, context, payload, &error, &message)
                    .await;
                (Some(message), failure.and(Err(error)))
            }
        };
        emit(dispatcher, JobAttempted::NAME, || JobAttempted {
            connection_name: self.name.clone(),
            job: snapshot(payload),
            exception,
            duration,
        })
        .await?;
        outcome
    }

    async fn fail(
        &self,
        dispatcher: &Dispatcher,
        context: &mut JobContext<'_>,
        payload: &Payload,
        error: &QueueError,
        message: &str,
    ) -> QueueResult<()> {
        emit(dispatcher, JobExceptionOccurred::NAME, || {
            JobExceptionOccurred {
                connection_name: self.name.clone(),
                job: snapshot(payload),
                exception: message.to_owned(),
            }
        })
        .await?;
        let failure = async {
            if !payload.chain_catches().is_empty() {
                self.registry.run_chain_catches(payload, error).await?;
            }
            let mut failed_context = JobContext {
                connection: context.connection.clone(),
                queue: context.queue.clone(),
                attempts: context.attempts,
                transaction: context.transaction(),
                ..JobContext::default()
            };
            self.registry
                .run_failed_handler(&mut failed_context, payload, error)
                .await
        }
        .await;
        emit(dispatcher, JobFailed::NAME, || JobFailed {
            connection_name: self.name.clone(),
            job: snapshot(payload),
            exception: message.to_owned(),
        })
        .await?;
        failure
    }
}

fn snapshot(payload: &Payload) -> JobSnapshot {
    JobSnapshot {
        id: String::new(),
        queue: "sync".into(),
        attempts: 1,
        payload: payload.clone(),
    }
}

async fn emit<E: Event>(
    dispatcher: &Dispatcher,
    name: &str,
    event: impl FnOnce() -> E,
) -> QueueResult<()> {
    if dispatcher.observes(name) {
        dispatcher
            .dispatch(Arc::new(event()), &mut EventContext::new())
            .await
            .map_err(backend)?;
    }
    Ok(())
}

impl Contract for SyncQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }

    fn size<'a>(&'a self, _queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async { Ok(0) })
    }

    fn metrics<'a>(
        &'a self,
        _queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async {
            Ok(QueueMetrics {
                size: 0,
                pending: Some(0),
                delayed: Some(0),
                reserved: Some(0),
                oldest_pending: None,
            })
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.execute(payload, queue, None))
    }

    fn push_in_transaction<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
        _delay: Option<Delay>,
        transaction: &'a mut crate::database::Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.execute(payload, queue, Some(transaction)))
    }

    fn later<'a>(
        &'a self,
        _delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        self.push(payload, queue)
    }

    fn pop<'a>(
        &'a self,
        _queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async { Ok(None) })
    }
}
