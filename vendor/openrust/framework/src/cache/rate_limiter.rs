use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use crate::http::{HttpError, HttpFuture, Request};

use super::{
    CacheError, CacheManager, CacheResult, Repository,
    rate_limiting::{
        AsyncLimiter, JobLimiter, JobLimiterResponse, Limit, Limiter, LimiterResponse,
        counters::{self, Admission, Quota},
    },
};

#[derive(Clone)]
pub(crate) enum NamedLimiter {
    Sync(Arc<Limiter>),
    Async(Arc<AsyncLimiter>),
}

/// Registers application-specific rate limit policies.
#[derive(Default)]
pub struct RateLimiter {
    limiters: RwLock<HashMap<String, NamedLimiter>>,
    job_limiters: RwLock<HashMap<String, Arc<JobLimiter>>>,
    cache: Option<(Arc<CacheManager>, Option<String>)>,
}

impl RateLimiter {
    pub(crate) fn with_cache(cache: Arc<CacheManager>, store: Option<String>) -> Self {
        Self {
            cache: Some((cache, store)),
            ..Self::default()
        }
    }

    pub(crate) fn repository(&self) -> CacheResult<Arc<Repository>> {
        let (cache, store) = self
            .cache
            .as_ref()
            .ok_or(CacheError::InvalidConfiguration)?;
        cache.store(store.as_deref())
    }

    pub async fn attempt<T>(
        &self,
        key: &str,
        max_attempts: u64,
        action: impl std::future::Future<Output = CacheResult<T>>,
        decay_seconds: u64,
    ) -> CacheResult<super::rate_limiting::Attempt<T>> {
        use super::rate_limiting::Attempt;
        let quota = Quota {
            key: counters::key("manual", "", key),
            max: max_attempts,
            decay: decay_seconds,
            charge: false,
        };
        let repository = self.repository()?;
        match counters::admit(repository.as_ref(), &[quota]).await? {
            Admission::Denied { .. } => Ok(Attempt::Throttled),
            Admission::Allowed(_) => {
                let result = action.await?;
                self.hit(key, decay_seconds).await?;
                Ok(Attempt::Executed(result))
            }
        }
    }

    /// 原子扣减一次额度。达到限额时返回距离下次可用的秒数。
    pub async fn reserve(&self, key: &str, max: u64, decay: u64) -> CacheResult<Option<u64>> {
        let quota = Quota {
            key: counters::key("manual", "", key),
            max,
            decay,
            charge: true,
        };
        let repository = self.repository()?;
        Ok(
            match counters::admit(repository.as_ref(), &[quota]).await? {
                Admission::Allowed(_) => None,
                Admission::Denied { bucket, .. } => Some(bucket.remaining_seconds),
            },
        )
    }

    pub async fn hit(&self, key: &str, decay_seconds: u64) -> CacheResult<u64> {
        let repository = self.repository()?;
        let quota = Quota {
            key: counters::key("manual", "", key),
            max: i64::MAX as u64,
            decay: decay_seconds,
            charge: true,
        };
        match counters::admit(repository.as_ref(), &[quota]).await? {
            Admission::Allowed(buckets) => Ok(buckets[0].count),
            Admission::Denied { .. } => Err(CacheError::InvalidValue),
        }
    }

    pub async fn attempts(&self, key: &str) -> CacheResult<u64> {
        Ok(counters::snapshot(
            self.repository()?.as_ref(),
            &counters::key("manual", "", key),
        )
        .await?
        .count)
    }

    pub async fn too_many_attempts(&self, key: &str, max_attempts: u64) -> CacheResult<bool> {
        Ok(self.attempts(key).await? >= max_attempts)
    }

    pub async fn remaining(&self, key: &str, max_attempts: u64) -> CacheResult<u64> {
        Ok(max_attempts.saturating_sub(self.attempts(key).await?))
    }

    pub async fn available_in(&self, key: &str) -> CacheResult<u64> {
        Ok(counters::snapshot(
            self.repository()?.as_ref(),
            &counters::key("manual", "", key),
        )
        .await?
        .remaining_seconds)
    }

    pub async fn clear(&self, key: &str) -> CacheResult<()> {
        counters::clear(
            self.repository()?.as_ref(),
            &counters::key("manual", "", key),
        )
        .await
    }

    pub fn r#for<R>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&Request) -> R + Send + Sync + 'static,
    ) -> &Self
    where
        R: Into<LimiterResponse>,
    {
        let limiter = move |request: &Request| {
            let mut result = callback(request).into();
            if let LimiterResponse::Limits(limits) = &mut result {
                normalize_limits(limits);
            }
            result
        };
        self.limiters
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.into(), NamedLimiter::Sync(Arc::new(limiter)));
        self
    }

    /// Registers a request policy that can await body parsing and propagate HTTP errors.
    pub fn for_async(
        &self,
        name: impl Into<String>,
        callback: impl for<'a> Fn(&'a mut Request) -> HttpFuture<'a, Result<LimiterResponse, HttpError>>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        let limiter: Arc<AsyncLimiter> = Arc::new(move |request| {
            let future = callback(request);
            Box::pin(async move {
                let mut result = future.await?;
                if let LimiterResponse::Limits(limits) = &mut result {
                    normalize_limits(limits);
                }
                Ok(result)
            })
        });
        self.limiters
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.into(), NamedLimiter::Async(limiter));
        self
    }

    pub fn for_job<R>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&crate::queue::Payload) -> R + Send + Sync + 'static,
    ) -> &Self
    where
        R: Into<JobLimiterResponse>,
    {
        let limiter = move |payload: &crate::queue::Payload| {
            let mut result = callback(payload).into();
            normalize_limits(&mut result.0);
            result
        };
        self.job_limiters
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.into(), Arc::new(limiter));
        self
    }

    pub(crate) fn job_limiter(&self, name: &str) -> Option<Arc<JobLimiter>> {
        self.job_limiters
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    /// Returns the synchronous policy, or None for an absent or asynchronous registration.
    pub fn limiter(&self, name: &str) -> Option<Arc<Limiter>> {
        match self.named_limiter(name)? {
            NamedLimiter::Sync(callback) => Some(callback),
            NamedLimiter::Async(_) => None,
        }
    }

    /// Returns the asynchronous policy, or None for an absent or synchronous registration.
    pub fn async_limiter(&self, name: &str) -> Option<Arc<AsyncLimiter>> {
        match self.named_limiter(name)? {
            NamedLimiter::Async(callback) => Some(callback),
            NamedLimiter::Sync(_) => None,
        }
    }

    pub(crate) fn named_limiter(&self, name: &str) -> Option<NamedLimiter> {
        self.limiters
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }
}

fn normalize_limits(limits: &mut [Limit]) {
    let mut seen = HashSet::new();
    let mut duplicates = HashSet::new();
    for limit in &*limits {
        if !seen.insert(limit.key.as_str()) {
            duplicates.insert(limit.key.clone());
        }
    }
    for limit in limits {
        if duplicates.contains(&limit.key) {
            limit.key = limit.fallback_key();
        }
    }
}
