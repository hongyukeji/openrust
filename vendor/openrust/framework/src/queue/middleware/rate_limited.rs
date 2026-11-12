use std::time::{Duration, SystemTime};

use crate::{
    cache::{
        CacheError,
        rate_limiting::counters::{self, Admission, Quota},
    },
    redis::{Redis, limiters::DurationLimiter},
    support::facades::{Facade, RateLimiter},
};

use super::{JobMiddleware, JobNext};
use crate::queue::{Delay, JobContext, Payload, QueueFuture, QueueResult, backend};

#[derive(Clone, Copy)]
enum Backend<'a> {
    Cache,
    Redis(Option<&'a str>),
}

/// Limits jobs using the application's configured rate limiter cache.
pub struct RateLimited {
    name: String,
    release_after: Option<Delay>,
    should_release: bool,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel queue middleware configuration names"
)]
impl RateLimited {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            release_after: None,
            should_release: true,
        }
    }

    pub fn releaseAfter(mut self, delay: impl Into<Delay>) -> Self {
        self.release_after = Some(delay.into());
        self
    }

    pub fn dontRelease(mut self) -> Self {
        self.should_release = false;
        self
    }

    async fn run(
        &self,
        backend_store: Backend<'_>,
        context: &mut JobContext<'_>,
        payload: &Payload,
        next: JobNext<'_>,
    ) -> QueueResult<()> {
        let limiter = RateLimiter::getFacadeRoot()
            .map_err(CacheError::Unavailable)
            .map_err(backend)?;
        let Some(policy) = limiter.job_limiter(&self.name) else {
            return next.run(context, payload).await;
        };
        let limits = policy(payload).0;
        if limits.is_empty() {
            return next.run(context, payload).await;
        }
        // A named policy may apply several counters in one Redis command.
        let hash_tag = matches!(backend_store, Backend::Redis(_))
            .then(|| counters::key("queue-redis-slot", &self.name, ""));
        let quotas: Vec<_> = limits
            .into_iter()
            .map(|limit| {
                let key = if let Some(tag) = &hash_tag {
                    let key = counters::key("queue-redis", &self.name, &limit.key);
                    format!("{{{tag}}}:{key}")
                } else {
                    counters::key("queue", &self.name, &limit.key)
                };
                Quota {
                    key,
                    max: limit.maxAttempts,
                    decay: limit.decaySeconds,
                    charge: true,
                }
            })
            .collect();
        let admission = if let Backend::Redis(connection) = backend_store {
            DurationLimiter::new(Redis::connection(connection).map_err(backend)?)
                .admit(&quotas)
                .await
        } else {
            counters::admit(limiter.repository().map_err(backend)?.as_ref(), &quotas).await
        }
        .map_err(backend)?;
        if let Admission::Denied { bucket, .. } = admission {
            if self.should_release {
                let delay = match self.release_after {
                    Some(Delay::For(delay)) if !delay.is_zero() => delay,
                    Some(Delay::Until(time)) => {
                        time.duration_since(SystemTime::now()).unwrap_or_default()
                    }
                    _ => Duration::from_secs(bucket.remaining_seconds.saturating_add(3)),
                };
                context.release(delay);
            }
            return Ok(());
        }
        next.run(context, payload).await
    }
}

impl JobMiddleware for RateLimited {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(self.run(Backend::Cache, context, payload, next))
    }
}

/// Limits jobs atomically on a selected Redis connection.
pub struct RateLimitedWithRedis {
    limiter: RateLimited,
    connection: Option<String>,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel queue middleware configuration names"
)]
impl RateLimitedWithRedis {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            limiter: RateLimited::new(name),
            connection: None,
        }
    }

    pub fn connection(mut self, name: impl Into<String>) -> Self {
        self.connection = Some(name.into());
        self
    }

    pub fn releaseAfter(mut self, delay: impl Into<Delay>) -> Self {
        self.limiter = self.limiter.releaseAfter(delay);
        self
    }

    pub fn dontRelease(mut self) -> Self {
        self.limiter = self.limiter.dontRelease();
        self
    }
}

impl JobMiddleware for RateLimitedWithRedis {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(self.limiter.run(
            Backend::Redis(self.connection.as_deref()),
            context,
            payload,
            next,
        ))
    }
}
