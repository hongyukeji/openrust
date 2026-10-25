//! Shared rate-limit service contract.

use std::{fmt, future::Future, pin::Pin, sync::Arc, time::Duration};

use crate::{Error, Result};

/// A future returned by a [`RateLimitStore`].
pub type RateLimitFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Atomic counter result for one key and fixed window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitBucket {
    count: u64,
    reset_after_secs: u64,
}

impl RateLimitBucket {
    /// Creates a bucket result for a custom store implementation.
    pub fn new(count: u64, reset_after_secs: u64) -> Self {
        Self {
            count,
            reset_after_secs: reset_after_secs.max(1),
        }
    }

    /// Returns the number of requests consumed in the active window.
    pub const fn count(self) -> u64 {
        self.count
    }

    /// Returns the number of seconds until the current window resets.
    pub const fn reset_after_secs(self) -> u64 {
        self.reset_after_secs
    }
}

/// Storage contract for atomically counting a key within a fixed time window.
///
/// Implementations must not log or persist `key` in plaintext. Shared deployments must choose a
/// store whose atomicity scope includes every application node that consumes the same quota.
/// Callers must include a stable policy/scope identity in each key and use the same `window`
/// wherever that key is consumed. Hashing a key is pseudonymization, not protection against
/// guessing low-entropy values such as email addresses.
pub trait RateLimitStore: Send + Sync + 'static {
    /// Atomically consumes one hit and returns the updated count and reset delay.
    fn consume<'a>(
        &'a self,
        key: &'a str,
        window: Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>>;
}

/// Cloneable service that enforces a fixed-window request quota through a registered store.
#[derive(Clone)]
pub struct RateLimiter {
    store: Arc<dyn RateLimitStore>,
}

impl fmt::Debug for RateLimiter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RateLimiter")
    }
}

impl RateLimiter {
    /// Creates the service from a custom or framework-provided atomic store.
    pub fn new(store: impl RateLimitStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    /// Consumes one request and returns its quota status, or a stable rate-limit error.
    ///
    /// The first `limit` calls in each window succeed. Later calls fail with HTTP 429 semantics
    /// and a `Retry-After` delay from the store. Store failures are propagated and never treated
    /// as permission to proceed.
    pub async fn check(&self, key: &str, limit: u64, window: Duration) -> Result<RateLimitStatus> {
        if key.is_empty() || key.len() > 512 || key.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(Error::custom(
                "invalid_rate_limit_key",
                "rate-limit key must be non-empty and at most 512 bytes",
            ));
        }
        if limit == 0
            || window.as_secs() == 0
            || window.subsec_nanos() != 0
            || window.as_secs() > 31_536_000
        {
            return Err(Error::custom(
                "invalid_rate_limit_policy",
                "rate-limit limit and whole-second window must be positive and bounded",
            ));
        }
        let bucket = self.store.consume(key, window).await?;
        if bucket.count > limit {
            return Err(Error::rate_limited(bucket.reset_after_secs));
        }
        Ok(RateLimitStatus {
            limit,
            remaining: limit.saturating_sub(bucket.count),
            reset_after_secs: bucket.reset_after_secs,
        })
    }
}

/// Successful status for a consumed request within its configured quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitStatus {
    limit: u64,
    remaining: u64,
    reset_after_secs: u64,
}

impl RateLimitStatus {
    /// Returns the maximum number of requests accepted during the window.
    pub const fn limit(self) -> u64 {
        self.limit
    }

    /// Returns the number of accepted requests remaining in this window.
    pub const fn remaining(self) -> u64 {
        self.remaining
    }

    /// Returns the seconds until this window resets.
    pub const fn reset_after_secs(self) -> u64 {
        self.reset_after_secs
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{RateLimitBucket, RateLimitFuture, RateLimitStore, RateLimiter};
    use crate::{Error, Result};

    #[derive(Default)]
    struct FakeStore {
        count: Mutex<u64>,
        fail: bool,
    }

    impl RateLimitStore for FakeStore {
        fn consume<'a>(
            &'a self,
            _key: &'a str,
            _window: std::time::Duration,
        ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
            Box::pin(async move {
                if self.fail {
                    return Err(Error::internal(std::io::Error::other("store failed")));
                }
                let mut count = self
                    .count
                    .lock()
                    .map_err(|_| Error::internal(std::io::Error::other("fake lock poisoned")))?;
                *count += 1;
                Ok(RateLimitBucket::new(*count, 17))
            })
        }
    }

    #[tokio::test]
    async fn permits_the_limit_then_returns_retry_after() -> Result<()> {
        let limiter = RateLimiter::new(FakeStore::default());
        let status = limiter
            .check("login:client", 2, std::time::Duration::from_secs(60))
            .await?;
        assert_eq!(status.remaining(), 1);
        limiter
            .check("login:client", 2, std::time::Duration::from_secs(60))
            .await?;
        assert!(matches!(
            limiter
                .check("login:client", 2, std::time::Duration::from_secs(60))
                .await,
            Err(Error::RateLimitedWithRetryAfter(17))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_policy_and_fails_closed_on_store_error() -> Result<()> {
        let limiter = RateLimiter::new(FakeStore {
            count: Mutex::new(0),
            fail: true,
        });
        assert!(
            limiter
                .check("key", 0, std::time::Duration::from_secs(1))
                .await
                .is_err()
        );
        assert!(
            limiter
                .check("key", 1, std::time::Duration::from_secs(1))
                .await
                .is_err()
        );
        Ok(())
    }
}
