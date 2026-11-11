use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use tokio_util::sync::CancellationToken;

use crate::{
    container::Container,
    contracts::{
        container::Container as _,
        queue::{Job, Queue},
    },
    support::facades::isolated_scope,
};

use super::{
    JobContext, QueueError, QueueResult, WorkerOptions,
    failed::{FailedJob, FailedJobProviderInterface},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkResult {
    Idle,
    Processed,
    Released,
    Failed,
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
        let Some(mut job) = connection.pop(queue).await? else {
            return Ok(WorkResult::Idle);
        };
        self.process(job.as_mut(), options).await
    }

    pub async fn daemon(
        &self,
        connection: &dyn Queue,
        queues: &[&str],
        options: &WorkerOptions,
        shutdown: &CancellationToken,
    ) -> QueueResult<u64> {
        let start = tokio::time::Instant::now();
        let deadline = options.max_time.and_then(|limit| start.checked_add(limit));
        let mut processed = 0;
        while !shutdown.is_cancelled()
            && options.max_jobs.is_none_or(|limit| processed < limit)
            && options.max_time.is_none_or(|limit| start.elapsed() < limit)
        {
            let reservation = async {
                if queues.is_empty() {
                    return if options.stop_when_empty {
                        connection.pop_now(None).await
                    } else {
                        connection.pop(None).await
                    };
                }
                for queue in queues {
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
                () = shutdown.cancelled() => break,
                () = wait_until(deadline) => break,
                result = reservation => result?,
            };
            if let Some(mut job) = job {
                self.process(job.as_mut(), options).await?;
                processed += 1;
            } else if options.stop_when_empty {
                break;
            } else {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = wait_until(deadline) => break,
                    () = tokio::time::sleep(options.sleep) => {},
                }
            }
        }
        Ok(processed)
    }

    async fn process(&self, job: &mut dyn Job, options: &WorkerOptions) -> QueueResult<WorkResult> {
        if self
            .failed
            .find(job.uuid())
            .await?
            .is_some_and(|failed| failed.payload.delivery_id == job.payload().delivery_id)
        {
            job.delete().await?;
            return Ok(WorkResult::Failed);
        }
        let tries = job.options().max_tries.unwrap_or(options.tries);
        if exceeds_attempts(job, tries, false) {
            return self.fail(job, &QueueError::MaxAttempts).await;
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
            let execution = job.fire(&mut context);
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
                job.delete().await?;
                Ok(WorkResult::Processed)
            }
            Err(error)
                if exceeds_attempts(job, tries, true)
                    || matches!(error, QueueError::Timeout) && job.options().fail_on_timeout =>
            {
                self.fail(job, &error).await
            }
            Err(_) => {
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

    async fn fail(&self, job: &mut dyn Job, error: &QueueError) -> QueueResult<WorkResult> {
        self.failed.log(&FailedJob::new(job, error)).await?;
        job.fail(Some(error)).await?;
        Ok(WorkResult::Failed)
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
