use std::{ops::ControlFlow, sync::Arc, time::Duration};

use crate::{
    cache::{
        CacheError, RateLimiter,
        rate_limiting::counters::{self, Admission, Quota},
    },
    contracts::{
        container::Container as _,
        debug::{Exception, ExceptionHandler},
    },
    redis::{Redis, limiters::DurationLimiter},
    support::facades::{self, Facade, current_container},
};

use super::{JobMiddleware, JobNext};
use crate::queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult, backend};

type ErrorPredicate = dyn Fn(&QueueError) -> bool + Send + Sync;
type LimiterPredicate = dyn Fn(&QueueError, &ExceptionThrottleLimiter<'_>) -> bool + Send + Sync;

/// Limiter context supplied to exception throttling callbacks.
pub enum ExceptionThrottleLimiter<'a> {
    Cache(&'a RateLimiter),
    Redis {
        /// Remaining failure budget at the start of the attempt.
        remaining: u64,
        /// Unix timestamp in seconds when the current window ends.
        decays_at: u64,
    },
}
type Backoff = dyn Fn(&QueueError) -> u64 + Send + Sync;

/// Delays jobs after repeated exceptions using the configured limiter cache.
pub struct ThrottlesExceptions {
    max_attempts: u64,
    decay: u64,
    prefix: String,
    key: Option<String>,
    by_job: bool,
    backoff: Box<Backoff>,
    when: Option<Box<LimiterPredicate>>,
    report: Option<Box<LimiterPredicate>>,
    delete_when: Vec<Box<ErrorPredicate>>,
    fail_when: Vec<Box<ErrorPredicate>>,
}

impl Default for ThrottlesExceptions {
    fn default() -> Self {
        Self::new(10, 600)
    }
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel queue middleware configuration names"
)]
impl ThrottlesExceptions {
    pub fn new(max_attempts: u64, decay_seconds: u64) -> Self {
        Self {
            max_attempts,
            decay: decay_seconds,
            prefix: "laravel_throttles_exceptions:".into(),
            key: None,
            by_job: false,
            backoff: Box::new(|_| 0),
            when: None,
            report: None,
            delete_when: Vec::new(),
            fail_when: Vec::new(),
        }
    }

    pub fn withPrefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn by(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn byJob(mut self) -> Self {
        self.by_job = true;
        self
    }

    pub fn backoff(self, minutes: u64) -> Self {
        self.backoffWhen(move |_| minutes)
    }

    pub fn backoffWhen(
        mut self,
        callback: impl Fn(&QueueError) -> u64 + Send + Sync + 'static,
    ) -> Self {
        self.backoff = Box::new(callback);
        self
    }

    pub fn when(
        mut self,
        callback: impl Fn(&QueueError, &ExceptionThrottleLimiter<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.when = Some(Box::new(callback));
        self
    }

    pub fn report(self) -> Self {
        self.reportWhen(|_, _| true)
    }

    pub fn reportWhen(
        mut self,
        callback: impl Fn(&QueueError, &ExceptionThrottleLimiter<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.report = Some(Box::new(callback));
        self
    }

    pub fn deleteWhen(
        mut self,
        callback: impl Fn(&QueueError) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.delete_when.push(Box::new(callback));
        self
    }

    pub fn failWhen(
        mut self,
        callback: impl Fn(&QueueError) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.fail_when.push(Box::new(callback));
        self
    }

    async fn run(
        &self,
        limiter: Backend,
        context: &mut JobContext<'_>,
        payload: &Payload,
        next: JobNext<'_>,
    ) -> QueueResult<()> {
        let id = self
            .key
            .as_deref()
            .filter(|key| !matches!(*key, "" | "0"))
            .unwrap_or_else(|| {
                if self.by_job {
                    payload.uuid()
                } else {
                    payload.name()
                }
            });
        let key = counters::key("queue-exceptions", &self.prefix, id);
        let mut quota = Quota {
            key,
            max: self.max_attempts,
            decay: self.decay,
            charge: false,
        };
        let state = match limiter.check(&quota).await? {
            ControlFlow::Continue(state) => state,
            ControlFlow::Break(delay) => {
                context.release(delay);
                return Ok(());
            }
        };

        let error = match next.run(context, payload).await {
            Ok(()) => return limiter.clear(&quota.key).await,
            Err(error) => error,
        };
        if self.when.as_ref().is_some_and(|when| !when(&error, &state)) {
            return Err(error);
        }
        if self
            .report
            .as_ref()
            .is_some_and(|report| report(&error, &state))
            && let Some(container) = current_container()
            && let Ok(handler) = container.make::<dyn ExceptionHandler>()
        {
            let _ = handler.report(&Exception::Error(&error)).await;
        }
        if self.delete_when.iter().any(|callback| callback(&error)) {
            context.release_after = None;
            return Ok(());
        }
        if self.fail_when.iter().any(|callback| callback(&error)) {
            context.fail_immediately = true;
            return Err(error);
        }
        quota.charge = true;
        limiter.hit(&quota).await?;
        context.release(Duration::from_secs(
            (self.backoff)(&error).saturating_mul(60),
        ));
        Ok(())
    }
}

enum Backend {
    Cache(Arc<RateLimiter>),
    Redis(DurationLimiter),
}

impl Backend {
    async fn check(
        &self,
        quota: &Quota,
    ) -> QueueResult<ControlFlow<Duration, ExceptionThrottleLimiter<'_>>> {
        match self {
            Self::Cache(limiter) => {
                if limiter
                    .tooManyAttempts(&quota.key, quota.max)
                    .await
                    .map_err(backend)?
                {
                    let seconds = limiter.availableIn(&quota.key).await.map_err(backend)?;
                    return Ok(ControlFlow::Break(Duration::from_secs(
                        seconds.saturating_add(3),
                    )));
                }
                Ok(ControlFlow::Continue(ExceptionThrottleLimiter::Cache(
                    limiter,
                )))
            }
            Self::Redis(limiter) => {
                let admission = limiter
                    .admit(std::slice::from_ref(quota))
                    .await
                    .map_err(backend)?;
                Ok(match admission {
                    Admission::Allowed(buckets) => {
                        let bucket = buckets[0];
                        let seconds = if bucket.count == 0 {
                            quota.decay
                        } else {
                            bucket.remaining_seconds
                        };
                        ControlFlow::Continue(ExceptionThrottleLimiter::Redis {
                            remaining: quota.max.saturating_sub(bucket.count),
                            decays_at: counters::clock().map_err(backend)?.saturating_add(seconds),
                        })
                    }
                    Admission::Denied { bucket, .. } => {
                        ControlFlow::Break(Duration::from_secs(bucket.remaining_seconds))
                    }
                })
            }
        }
    }

    async fn hit(&self, quota: &Quota) -> QueueResult<()> {
        match self {
            Self::Cache(limiter) => limiter.hit(&quota.key, quota.decay).await.map(|_| ()),
            Self::Redis(limiter) => limiter.admit(std::slice::from_ref(quota)).await.map(|_| ()),
        }
        .map_err(backend)
    }

    async fn clear(&self, key: &str) -> QueueResult<()> {
        match self {
            Self::Cache(limiter) => limiter.clear(key).await,
            Self::Redis(limiter) => limiter.clear(key).await,
        }
        .map_err(backend)
    }
}

impl JobMiddleware for ThrottlesExceptions {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let limiter = facades::RateLimiter::getFacadeRoot()
                .map_err(CacheError::Unavailable)
                .map_err(backend)?;
            self.run(Backend::Cache(limiter), context, payload, next)
                .await
        })
    }
}

/// Delays jobs after repeated exceptions on a selected Redis connection.
pub struct ThrottlesExceptionsWithRedis {
    throttle: ThrottlesExceptions,
    connection: Option<String>,
}

impl Default for ThrottlesExceptionsWithRedis {
    fn default() -> Self {
        Self::from(ThrottlesExceptions::default())
    }
}

impl From<ThrottlesExceptions> for ThrottlesExceptionsWithRedis {
    fn from(throttle: ThrottlesExceptions) -> Self {
        Self {
            throttle,
            connection: None,
        }
    }
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel queue middleware configuration names"
)]
impl ThrottlesExceptionsWithRedis {
    pub fn new(max_attempts: u64, decay_seconds: u64) -> Self {
        Self::from(ThrottlesExceptions::new(max_attempts, decay_seconds))
    }

    pub fn connection(mut self, name: impl Into<String>) -> Self {
        self.connection = Some(name.into());
        self
    }

    pub fn withPrefix(mut self, prefix: impl Into<String>) -> Self {
        self.throttle = self.throttle.withPrefix(prefix);
        self
    }

    pub fn by(mut self, key: impl Into<String>) -> Self {
        self.throttle = self.throttle.by(key);
        self
    }

    pub fn byJob(mut self) -> Self {
        self.throttle = self.throttle.byJob();
        self
    }

    pub fn backoff(self, minutes: u64) -> Self {
        self.backoffWhen(move |_| minutes)
    }

    pub fn backoffWhen(
        mut self,
        callback: impl Fn(&QueueError) -> u64 + Send + Sync + 'static,
    ) -> Self {
        self.throttle = self.throttle.backoffWhen(callback);
        self
    }

    pub fn when(
        mut self,
        callback: impl Fn(&QueueError, &ExceptionThrottleLimiter<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.throttle = self.throttle.when(callback);
        self
    }

    pub fn report(self) -> Self {
        self.reportWhen(|_, _| true)
    }

    pub fn reportWhen(
        mut self,
        callback: impl Fn(&QueueError, &ExceptionThrottleLimiter<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.throttle = self.throttle.reportWhen(callback);
        self
    }

    pub fn deleteWhen(
        mut self,
        callback: impl Fn(&QueueError) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.throttle = self.throttle.deleteWhen(callback);
        self
    }

    pub fn failWhen(
        mut self,
        callback: impl Fn(&QueueError) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.throttle = self.throttle.failWhen(callback);
        self
    }
}

impl JobMiddleware for ThrottlesExceptionsWithRedis {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let connection = Redis::connection(self.connection.as_deref()).map_err(backend)?;
            self.throttle
                .run(
                    Backend::Redis(DurationLimiter::new(connection)),
                    context,
                    payload,
                    next,
                )
                .await
        })
    }
}
