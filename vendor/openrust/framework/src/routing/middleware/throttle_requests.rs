use std::sync::Arc;

use ::http::{HeaderName, HeaderValue, header::RETRY_AFTER};

use crate::{
    cache::{
        CacheError, CacheResult, Repository,
        rate_limiting::{
            Limit, LimiterResponse,
            counters::{self, Admission, Bucket, Quota},
        },
    },
    contracts::http::Middleware,
    http::{HeaderMap, HttpError, HttpFuture, Next, Request, Response},
    redis::limiters::DurationLimiter,
    support::facades::{Facade, RateLimiter, Redis},
};

const LIMIT: HeaderName = HeaderName::from_static("x-ratelimit-limit");
const REMAINING: HeaderName = HeaderName::from_static("x-ratelimit-remaining");
const RESET: HeaderName = HeaderName::from_static("x-ratelimit-reset");

/// Applies a named cache-backed limiter to a route.
pub struct ThrottleRequests {
    name: String,
}

impl ThrottleRequests {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Middleware for ThrottleRequests {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(handle(&self.name, false, request, next))
    }
}

enum Backend {
    Cache(Arc<Repository>),
    Redis(DurationLimiter),
}

impl Backend {
    async fn admit(&self, quotas: &[Quota]) -> CacheResult<Admission> {
        match self {
            Self::Cache(repository) => counters::admit(repository.as_ref(), quotas).await,
            Self::Redis(limiter) => limiter.admit(quotas).await,
        }
    }
}

pub(super) async fn handle(
    name: &str,
    redis: bool,
    request: &mut Request,
    next: Next<'_>,
) -> Result<Response, HttpError> {
    let callback = RateLimiter::limiter(name)
        .map_err(internal)?
        .ok_or_else(|| internal(std::io::Error::other("named rate limiter is unavailable")))?;
    let limits = match callback(request) {
        LimiterResponse::Limit(limit) => vec![limit],
        LimiterResponse::Limits(limits) => limits,
        LimiterResponse::Response(response) => return Ok(response),
        LimiterResponse::Unlimited(_) => return Ok(next.run(request).await),
    };
    if limits.is_empty() {
        return Ok(next.run(request).await);
    }
    let backend = if redis {
        Backend::Redis(DurationLimiter::new(
            Redis::connection(None).map_err(internal)?,
        ))
    } else {
        let limiter = RateLimiter::getFacadeRoot()
            .map_err(CacheError::Unavailable)
            .map_err(internal)?;
        Backend::Cache(limiter.repository().map_err(internal)?)
    };
    let mut quotas: Vec<_> = limits
        .iter()
        .map(|limit| Quota {
            key: counters::key("http", name, &limit.key),
            max: limit.maxAttempts,
            decay: limit.decaySeconds,
            charge: limit.afterCallback.is_none(),
        })
        .collect();
    let mut buckets = match backend.admit(&quotas).await.map_err(internal)? {
        Admission::Allowed(buckets) => buckets,
        Admission::Denied { index, bucket } => {
            let limit = &limits[index];
            let retry = if bucket.remaining_seconds == 0 {
                limit.decaySeconds
            } else {
                bucket.remaining_seconds
            };
            let headers = headers(limit, bucket, Some(retry))?;
            return if let Some(callback) = &limit.responseCallback {
                Ok(callback(request, headers))
            } else {
                Err(HttpError::Throttled { headers })
            };
        }
    };
    let mut response = next.run(request).await;
    for ((limit, quota), bucket) in limits.iter().zip(quotas.iter_mut()).zip(&mut buckets) {
        if limit
            .afterCallback
            .as_ref()
            .is_some_and(|callback| callback(&response))
        {
            quota.max = i64::MAX as u64;
            quota.charge = true;
            *bucket = match backend
                .admit(std::slice::from_ref(quota))
                .await
                .map_err(internal)?
            {
                Admission::Allowed(buckets) => buckets[0],
                Admission::Denied { bucket, .. } => bucket,
            };
        }
        let remaining = limit.maxAttempts.saturating_sub(bucket.count);
        let response_headers = &mut response.parts_mut().headers;
        if response_headers
            .get(&REMAINING)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_none_or(|existing| existing > remaining)
        {
            response_headers.extend(headers(limit, *bucket, None)?);
        }
    }
    Ok(response)
}

fn headers(limit: &Limit, bucket: Bucket, retry: Option<u64>) -> Result<HeaderMap, HttpError> {
    let mut headers = HeaderMap::new();
    headers.insert(LIMIT, HeaderValue::from(limit.maxAttempts));
    headers.insert(
        REMAINING,
        HeaderValue::from(limit.maxAttempts.saturating_sub(bucket.count)),
    );
    if let Some(retry) = retry {
        headers.insert(RETRY_AFTER, HeaderValue::from(retry));
        headers.insert(
            RESET,
            HeaderValue::from(counters::clock().map_err(internal)?.saturating_add(retry)),
        );
    }
    Ok(headers)
}

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
