use std::{sync::Arc, time::Duration};

use crate::{cache::Cache, contracts::queue::JobHandler};

use super::{JobContext, Payload, QueueFuture, QueueResult, backend};

/// Wraps execution of an application job.
pub trait JobMiddleware: Send + Sync {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>>;
}

/// The remaining middleware and handler in the current attempt.
pub struct JobNext<'a> {
    handler: &'a dyn JobHandler,
    middleware: &'a [Arc<dyn JobMiddleware>],
}

impl<'a> JobNext<'a> {
    pub(super) fn new(
        handler: &'a dyn JobHandler,
        middleware: &'a [Arc<dyn JobMiddleware>],
    ) -> Self {
        Self {
            handler,
            middleware,
        }
    }

    pub fn run(
        self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        if let Some((middleware, remaining)) = self.middleware.split_first() {
            middleware.handle(context, payload, Self::new(self.handler, remaining))
        } else {
            self.handler.handle(context, payload)
        }
    }
}

/// Releases overlapping jobs with the configured delay while an owner holds the cache lock.
pub struct WithoutOverlapping {
    key: String,
    release_after: Option<Duration>,
    expires_after: Duration,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel queue middleware configuration names"
)]
impl WithoutOverlapping {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            release_after: Some(Duration::ZERO),
            expires_after: Duration::from_secs(90),
        }
    }

    pub fn releaseAfter(mut self, delay: Duration) -> Self {
        self.release_after = Some(delay);
        self
    }

    pub fn dontRelease(mut self) -> Self {
        self.release_after = None;
        self
    }

    pub fn expireAfter(mut self, delay: Duration) -> Self {
        self.expires_after = delay;
        self
    }
}

impl JobMiddleware for WithoutOverlapping {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let key = format!("laravel-queue-overlap:{}:{}", payload.name(), self.key);
            let mut lock = Cache::lock(&key, Some(self.expires_after)).map_err(backend)?;
            if !lock.get().await.map_err(backend)? {
                if let Some(delay) = self.release_after {
                    context.release(delay);
                }
                return Ok(());
            }
            let result = next.run(context, payload).await;
            let release = lock.release().await.map_err(backend);
            result.and(release.map(|_| ()))
        })
    }
}
