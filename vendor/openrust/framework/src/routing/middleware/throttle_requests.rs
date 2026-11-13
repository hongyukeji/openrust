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

/// Applies a named or numeric cache-backed limiter to a route.
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
    let named = if name.is_empty() || name.contains(',') {
        None
    } else {
        RateLimiter::limiter(name).map_err(internal)?
    };
    let (namespace, limits) = if let Some(callback) = named {
        let limits = match callback(request) {
            LimiterResponse::Limit(limit) => vec![limit],
            LimiterResponse::Limits(limits) => limits,
            LimiterResponse::Response(response) => return Ok(response),
            LimiterResponse::Unlimited(_) => return Ok(next.run(request).await),
        };
        (name, limits)
    } else {
        ("numeric", vec![numeric_limit(name, request).await?])
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
            key: counters::key("http", namespace, &limit.key),
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

async fn numeric_limit(parameters: &str, request: &mut Request) -> Result<Limit, HttpError> {
    use crate::auth::{Auth, AuthIdentifier};

    let invalid = || {
        internal(std::io::Error::other(
            "named rate limiter is unavailable or numeric throttle parameters are invalid",
        ))
    };
    let mut fields = parameters.split(',');
    let attempts = fields.next().ok_or_else(invalid)?;
    let attempts = if parameters.is_empty() {
        "60"
    } else {
        attempts
    };
    let seconds = fields
        .next()
        .unwrap_or("1")
        .parse::<f64>()
        .map_err(|_| invalid())?
        * 60.0;
    // The fixed-window backends accept integer seconds and at most one year.
    if !(1.0..=31_536_000.0).contains(&seconds) {
        return Err(invalid());
    }
    let prefix = fields.next().unwrap_or_default();
    if fields.next().is_some() {
        return Err(invalid());
    }

    let user = match Auth::user(request).await {
        Ok(user) => user,
        Err(crate::auth::AuthError::Configuration("guard" | "guard name")) => None,
        Err(error) => return Err(error.into()),
    };
    let attempts = match attempts.split_once('|') {
        Some((guest, authenticated)) => {
            if user.is_some() {
                authenticated
            } else {
                guest
            }
        }
        None => attempts,
    };
    let max_attempts = attempts.parse::<u64>().map_err(|_| invalid())?;
    let key = match user {
        Some(user) => match user.get_auth_identifier() {
            AuthIdentifier::Integer(id) => format!("{prefix}:user:{id}"),
            AuthIdentifier::UnsignedInteger(id) => format!("{prefix}:user:{id}"),
            AuthIdentifier::String(id) => format!("{prefix}:user:{id}"),
        },
        None => {
            if !request.matched_route {
                return Err(internal(std::io::Error::other(
                    "unable to generate the throttle request signature without a matched route",
                )));
            }
            let domain = request
                .parts()
                .extensions
                .get::<super::MatchedRouteDomain>()
                .map_or("", |domain| domain.0.as_str());
            let ip = request
                .parts()
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map_or_else(|| "unknown".into(), |address| address.0.ip().to_string());
            format!("{prefix}:guest:{domain}:{ip}")
        }
    };
    Ok(Limit::new(key, max_attempts, seconds as u64))
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
