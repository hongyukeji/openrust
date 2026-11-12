use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio_util::sync::CancellationToken;

use crate::{
    bus::{
        BatchPhase, DatabaseBatchRepository, PublicationKey, RecordedBatchJob,
        events::{BatchCanceled, BatchFinished, BatchStarted, emit},
    },
    container::Container,
    contracts::{
        container::Container as _,
        queue::{Job, Queue},
    },
    support::facades::isolated_scope,
};

use super::{
    ChainUpdate, JobContext, Payload, QueueError, QueueManager, QueueResult, WorkerOptions,
    WorkerStopReason,
    events::{WorkerQueuePaused, WorkerQueueResumed, WorkerStopping},
    failed::{FailedJob, FailedJobProviderInterface},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkResult {
    Idle,
    Processed,
    Released,
    Failed,
}

pub(super) enum WorkerStatus<'a> {
    Paused(&'a WorkerQueuePaused),
    Resumed(&'a WorkerQueueResumed),
    Job(&'a dyn Job, Option<JobOutcome<'a>>),
    Stopped(&'a WorkerStopping),
}

pub(super) struct JobOutcome<'a> {
    pub result: WorkResult,
    pub elapsed: Duration,
    pub cause: Option<&'a QueueError>,
}

/// Processes reserved jobs and coordinates retry and shutdown.
pub struct Worker {
    app: Arc<Container>,
    failed: Arc<dyn FailedJobProviderInterface>,
}

impl Worker {
    pub fn new(app: Arc<Container>) -> QueueResult<Self> {
        let failed = app
            .get::<dyn FailedJobProviderInterface>("queue.failer")
            .map_err(QueueError::Unavailable)?;
        Ok(Self { app, failed })
    }

    pub async fn run_next_job(
        &self,
        connection: &dyn Queue,
        queue: Option<&str>,
        options: &WorkerOptions,
    ) -> QueueResult<WorkResult> {
        let manager = self
            .app
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)?;
        if manager.polls_interruptions() {
            let name = connection.get_connection_name();
            let queue_name = queue.unwrap_or_else(|| connection.default_queue());
            if manager.isPaused(name, queue_name).await? {
                manager
                    .emit_control_event(WorkerQueuePaused {
                        connection: name.into(),
                        queue: queue_name.into(),
                    })
                    .await?;
                return Ok(WorkResult::Idle);
            }
        }
        let Some(mut job) = connection.pop(queue).await? else {
            return Ok(WorkResult::Idle);
        };
        self.process(job.as_mut(), options, &mut None).await
    }

    pub async fn daemon(
        &self,
        connection: &dyn Queue,
        queues: &[&str],
        options: &WorkerOptions,
        shutdown: &CancellationToken,
    ) -> QueueResult<u64> {
        self.daemon_with_status(connection, queues, options, shutdown, |_| {})
            .await
    }

    pub(super) async fn daemon_with_status(
        &self,
        connection: &dyn Queue,
        queues: &[&str],
        options: &WorkerOptions,
        shutdown: &CancellationToken,
        mut status: impl FnMut(WorkerStatus<'_>) + Send,
    ) -> QueueResult<u64> {
        let manager = self
            .app
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)?;
        let control = manager
            .polls_interruptions()
            .then(|| manager.worker_control())
            .transpose()?;
        let initial_restart = if let Some(control) = &control {
            control.restart_generation().await?
        } else {
            None
        };
        let default_queue = [connection.default_queue()];
        let queues = if queues.is_empty() {
            &default_queue[..]
        } else {
            queues
        };
        let name = connection.get_connection_name();
        let start = tokio::time::Instant::now();
        let deadline = options.max_time.and_then(|limit| start.checked_add(limit));
        let mut processed = 0;
        let mut last_job_processed_at = None;
        let mut paused: Vec<&str> = Vec::new();
        let reason = loop {
            if shutdown.is_cancelled() {
                break WorkerStopReason::Interrupted;
            }
            if let Some(control) = &control
                && control.restart_generation().await? != initial_restart
            {
                break WorkerStopReason::ReceivedRestartSignal;
            }
            if options
                .max_time
                .is_some_and(|limit| start.elapsed() >= limit)
            {
                break WorkerStopReason::MaxTimeExceeded;
            }
            if options.max_jobs.is_some_and(|limit| processed >= limit) {
                break WorkerStopReason::MaxJobsExceeded;
            }
            let reservation = async {
                if let Some(control) = &control {
                    let observed = control.paused_queues(name, queues).await?;
                    for queue in observed
                        .iter()
                        .copied()
                        .filter(|queue| !paused.contains(queue))
                    {
                        let event = WorkerQueuePaused {
                            connection: name.into(),
                            queue: queue.into(),
                        };
                        status(WorkerStatus::Paused(&event));
                        manager.emit_control_event(event).await?;
                    }
                    for queue in paused
                        .iter()
                        .copied()
                        .filter(|queue| !observed.contains(queue))
                    {
                        let event = WorkerQueueResumed {
                            connection: name.into(),
                            queue: queue.into(),
                        };
                        status(WorkerStatus::Resumed(&event));
                        manager.emit_control_event(event).await?;
                    }
                    paused = observed;
                }
                for queue in queues {
                    if paused.contains(queue) {
                        continue;
                    }
                    let job = if options.stop_when_empty || queues.len() > 1 {
                        connection.pop_now(Some(queue)).await?
                    } else {
                        connection.pop(Some(queue)).await?
                    };
                    if let Some(job) = job {
                        return Ok(Some(job));
                    }
                }
                Ok(None)
            };
            let job = tokio::select! {
                biased;
                () = shutdown.cancelled() => break WorkerStopReason::Interrupted,
                () = wait_until(deadline) => break WorkerStopReason::MaxTimeExceeded,
                result = reservation => result?,
            };
            if let Some(mut job) = job {
                let started = tokio::time::Instant::now();
                status(WorkerStatus::Job(job.as_ref(), None));
                let mut cause = None;
                let result = self.process(job.as_mut(), options, &mut cause).await?;
                status(WorkerStatus::Job(
                    job.as_ref(),
                    Some(JobOutcome {
                        result,
                        elapsed: started.elapsed(),
                        cause: cause.as_ref(),
                    }),
                ));
                processed += 1;
                last_job_processed_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|elapsed| elapsed.as_secs());
            } else if options.stop_when_empty {
                break WorkerStopReason::QueueEmpty;
            } else {
                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break WorkerStopReason::Interrupted,
                    () = wait_until(deadline) => break WorkerStopReason::MaxTimeExceeded,
                    () = tokio::time::sleep(options.sleep) => {},
                }
            }
        };
        let event = WorkerStopping {
            status: 0,
            worker_options: options.clone(),
            reason,
            jobs_processed: processed,
            last_job_processed_at,
            connection_name: name.into(),
            queue: queues.join(","),
        };
        status(WorkerStatus::Stopped(&event));
        manager.emit_control_event(event).await?;
        Ok(processed)
    }

    async fn process(
        &self,
        job: &mut dyn Job,
        options: &WorkerOptions,
        cause: &mut Option<QueueError>,
    ) -> QueueResult<WorkResult> {
        if job.payload().chain_delivery.is_some() {
            return self.forward_chain(job).await;
        }
        let batch = BatchCompletion::resolve(&self.app, job)?;
        if let Some(batch) = &batch
            && let Some(recorded) = batch.recorded().await?
        {
            return self.complete_recorded(job, recorded).await;
        }
        if let Some(failed) = self.failed.find(job.uuid()).await?
            && failed.payload.delivery_id == job.payload().delivery_id
        {
            if let Some(batch) = &batch {
                batch
                    .record(&self.app, job.payload(), Some(&failed.exception), None)
                    .await?;
            }
            job.delete().await?;
            return Ok(WorkResult::Failed);
        }
        let tries = job.options().max_tries.unwrap_or(options.tries);
        if exceeds_attempts(job, tries, false) {
            let error = QueueError::MaxAttempts;
            let result = self.fail(job, &error, batch.as_ref()).await;
            *cause = Some(error);
            return result;
        }
        let timeout = job.options().timeout.unwrap_or(options.timeout);
        let mut context = JobContext::default();
        let hydrated = job.payload().context.clone();
        let attempt = isolated_scope(self.app.clone(), async {
            use crate::log::context::Context;
            let context_repository = Context::repository().map_err(super::backend)?;
            Context::hooks()
                .map_err(super::backend)?
                .hydrate(&hydrated, &context_repository);
            let execution = async {
                if job.payload().chained_batch.is_some() {
                    self.app
                        .make::<QueueManager>()
                        .map_err(QueueError::Unavailable)?
                        .execute_chained_batch(
                            job.payload(),
                            job.get_connection_name(),
                            job.get_queue(),
                        )
                        .await
                } else {
                    job.fire(&mut context).await
                }
            };
            if timeout.is_zero() {
                execution.await
            } else {
                tokio::time::timeout(timeout, execution)
                    .await
                    .unwrap_or(Err(QueueError::Timeout))
            }
        });
        let result = attempt.await;
        if let Some(delay) = context.release_after {
            job.release(delay).await?;
            return Ok(WorkResult::Released);
        }
        match result {
            Ok(()) => {
                let edit = context.chain_update.take();
                if let Some(batch) = &batch
                    && !batch
                        .record(&self.app, job.payload(), None, edit.as_ref())
                        .await?
                    && let Some(recorded) = batch.recorded().await?
                {
                    return self.complete_recorded(job, recorded).await;
                }
                self.complete(job, edit.map(|edit| edit.next)).await?;
                Ok(WorkResult::Processed)
            }
            Err(error)
                if exceeds_attempts(job, tries, true)
                    || matches!(error, QueueError::Timeout) && job.options().fail_on_timeout =>
            {
                let result = self.fail(job, &error, batch.as_ref()).await;
                *cause = Some(error);
                result
            }
            Err(error) => {
                *cause = Some(error);
                let configured = &job.options().backoff;
                let backoff = if configured.is_empty() {
                    &options.backoff
                } else {
                    configured
                };
                let index = usize::try_from(job.attempts().saturating_sub(1)).unwrap_or(usize::MAX);
                let delay = backoff
                    .get(index)
                    .or_else(|| backoff.last())
                    .copied()
                    .unwrap_or(Duration::ZERO);
                job.release(delay).await?;
                Ok(WorkResult::Released)
            }
        }
    }

    async fn forward_chain(&self, job: &mut dyn Job) -> QueueResult<WorkResult> {
        let manager = self
            .app
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)?;
        let forwarded =
            isolated_scope(self.app.clone(), manager.forward_chain(job.payload())).await;
        let sync_failure = match forwarded {
            Ok(failure) => failure,
            Err(error) => {
                job.release(Duration::from_secs(1)).await?;
                return Err(error);
            }
        };
        job.delete().await?;
        let relay = job.payload();
        let result = isolated_scope(self.app.clone(), async {
            if let Err(error) = manager.acknowledge_chain_delivery(relay).await {
                let context = serde_json::Map::from_iter([
                    ("delivery_id".into(), relay.uuid().into()),
                    ("error".into(), error.to_string().into()),
                ]);
                let _ =
                    crate::log::Log::error("Could not confirm chain delivery receipt", &context);
            }
            if let Some(error) = sync_failure {
                let context = serde_json::Map::from_iter([
                    ("delivery_id".into(), relay.uuid().into()),
                    ("error".into(), error.to_string().into()),
                ]);
                let _ = crate::log::Log::error("Synchronous chain job failed", &context);
                WorkResult::Failed
            } else {
                WorkResult::Processed
            }
        })
        .await;
        Ok(result)
    }

    async fn complete_recorded(
        &self,
        job: &mut dyn Job,
        recorded: RecordedBatchJob,
    ) -> QueueResult<WorkResult> {
        if recorded.succeeded {
            self.complete(job, recorded.successor).await?;
            return Ok(WorkResult::Processed);
        }
        job.delete().await?;
        Ok(WorkResult::Failed)
    }

    async fn complete(&self, job: &mut dyn Job, edited_next: Option<Payload>) -> QueueResult<()> {
        if job.payload().chained_batch.is_none()
            && let Some(next) = edited_next.or_else(|| job.payload().next_chain_job())
        {
            if next.chain_route.is_none() && next.dispatch_options.is_none() {
                return job.advance_chain(&next).await;
            }
            let manager = self
                .app
                .make::<QueueManager>()
                .map_err(QueueError::Unavailable)?;
            let (next, queue, delay) =
                manager.chain_successor(job.get_connection_name(), job.get_queue(), next)?;
            return job.advance_chain_to(&next, queue.as_deref(), delay).await;
        }
        job.delete().await
    }

    async fn fail(
        &self,
        job: &mut dyn Job,
        error: &QueueError,
        batch: Option<&BatchCompletion>,
    ) -> QueueResult<WorkResult> {
        self.failed.log(&FailedJob::new(job, error)).await?;
        if let Some(batch) = batch {
            batch
                .record(&self.app, job.payload(), Some(&error.to_string()), None)
                .await?;
        }
        job.fail(Some(error)).await?;
        if !job.payload().chain_catches().is_empty() {
            let manager = self
                .app
                .make::<QueueManager>()
                .map_err(QueueError::Unavailable)?;
            isolated_scope(
                self.app.clone(),
                manager.run_chain_catches(job.payload(), error),
            )
            .await;
        }
        Ok(WorkResult::Failed)
    }
}

struct BatchCompletion {
    repository: DatabaseBatchRepository,
    queue: Arc<QueueManager>,
    id: String,
    job: String,
    delivery: String,
}

impl BatchCompletion {
    fn resolve(app: &Container, job: &dyn Job) -> QueueResult<Option<Self>> {
        let Some(id) = job.payload().batchId() else {
            return Ok(None);
        };
        let queue = app
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)?;
        Ok(Some(Self {
            repository: queue.batch_repository()?,
            queue,
            id: id.to_owned(),
            job: job.uuid().to_owned(),
            delivery: job.payload().delivery_id.clone().unwrap_or_default(),
        }))
    }

    async fn recorded(&self) -> QueueResult<Option<RecordedBatchJob>> {
        self.repository
            .recorded(&self.id, &self.job, &self.delivery)
            .await
    }

    async fn record(
        &self,
        app: &Arc<Container>,
        job: &super::Payload,
        failure: Option<&str>,
        edit: Option<&ChainUpdate>,
    ) -> QueueResult<bool> {
        let Some(transition) = self
            .repository
            .record(&self.id, &self.job, &self.delivery, failure.is_none(), edit)
            .await?
        else {
            return Ok(false);
        };
        isolated_scope(app.clone(), async {
            if let Some(batch) = transition.started {
                emit(BatchStarted { batch }).await;
            }
            if transition.canceled {
                emit(BatchCanceled {
                    batch: transition.batch.clone(),
                    exception: failure.map(str::to_owned),
                })
                .await;
            }
            for phase in transition.phases {
                if matches!(phase, BatchPhase::Then) {
                    emit(BatchFinished {
                        batch: transition.batch.clone(),
                    })
                    .await;
                }
                self.queue
                    .run_batch_callbacks(&transition.batch, &[phase], failure)
                    .await;
                if matches!(phase, BatchPhase::Catch)
                    && let Some(cause) = failure
                    && !transition.chain_catches.is_empty()
                {
                    let error = QueueError::ChainedBatchFailed(cause.to_owned());
                    self.queue
                        .run_chain_callbacks(job, &error, &transition.chain_catches)
                        .await;
                }
            }
            if transition.continuation_ready {
                self.queue
                    .publish_after_commit(
                        &self.repository,
                        &PublicationKey::Continuation(transition.batch.id),
                    )
                    .await;
            }
        })
        .await;
        Ok(true)
    }
}

fn exceeds_attempts(job: &dyn Job, tries: u32, including_current: bool) -> bool {
    if let Some(deadline) = job.options().retry_until {
        return SystemTime::now() >= deadline;
    }
    tries != 0 && (job.attempts() > tries || including_current && job.attempts() == tries)
}

async fn wait_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
