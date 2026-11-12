mod conditional;
mod rate_limited;
mod throttles_exceptions;

pub use conditional::{MiddlewareCondition, Release, Skip};
pub use rate_limited::{RateLimited, RateLimitedWithRedis};
pub use throttles_exceptions::{
    ExceptionThrottleLimiter, ThrottlesExceptions, ThrottlesExceptionsWithRedis,
};

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use crate::{cache::Cache, contracts::queue::JobHandler};

use super::{Delay, JobContext, Payload, QueueFuture, QueueResult, backend};

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

/// Skips jobs whose batch has been cancelled.
pub struct SkipIfBatchCancelled;

impl JobMiddleware for SkipIfBatchCancelled {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            if let Some(id) = payload.batchId()
                && crate::bus::Bus::findBatch(id)
                    .await?
                    .is_some_and(|batch| batch.cancelled())
            {
                return Ok(());
            }
            next.run(context, payload).await
        })
    }
}

/// Releases overlapping jobs with the configured delay while an owner holds the cache lock.
pub struct WithoutOverlapping {
    key: String,
    release_after: Option<Delay>,
    expires_after: Duration,
    prefix: String,
    shared: bool,
}

impl Default for WithoutOverlapping {
    fn default() -> Self {
        Self::new("")
    }
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel queue middleware configuration names"
)]
impl WithoutOverlapping {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            release_after: Some(Delay::For(Duration::ZERO)),
            expires_after: Duration::ZERO,
            prefix: "laravel-queue-overlap:".into(),
            shared: false,
        }
    }

    pub fn releaseAfter(mut self, delay: impl Into<Delay>) -> Self {
        self.release_after = Some(delay.into());
        self
    }

    pub fn dontRelease(mut self) -> Self {
        self.release_after = None;
        self
    }

    pub fn expireAfter(mut self, delay: impl Into<Delay>) -> Self {
        self.expires_after = overlap_delay(delay.into());
        self
    }

    pub fn withPrefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn shared(mut self) -> Self {
        self.shared = true;
        self
    }

    pub fn getLockKey(&self, payload: &Payload) -> String {
        if self.shared {
            format!("{}{}", self.prefix, self.key)
        } else {
            format!("{}{}:{}", self.prefix, payload.name(), self.key)
        }
    }
}

fn overlap_delay(delay: Delay) -> Duration {
    match delay {
        Delay::For(duration) => duration,
        Delay::Until(time) => time.duration_since(SystemTime::now()).unwrap_or_default(),
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
            let key = self.getLockKey(payload);
            let mut lock = Cache::lock(&key, Some(self.expires_after)).map_err(backend)?;
            if !lock.get().await.map_err(backend)? {
                if let Some(delay) = self.release_after {
                    context.release(overlap_delay(delay));
                }
                return Ok(());
            }
            let result = next.run(context, payload).await;
            let release = lock.release().await.map_err(backend);
            result.and(release.map(|_| ()))
        })
    }
}
