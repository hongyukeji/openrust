//! 共享限流服务契约。

use std::{fmt, future::Future, pin::Pin, sync::Arc, time::Duration};

use crate::{Error, Result};

/// [`RateLimitStore`] 返回的 future。
pub type RateLimitFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 单个 key 在固定时间窗口内的原子计数结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitBucket {
    count: u64,
    reset_after_secs: u64,
}

impl RateLimitBucket {
    /// 为自定义存储实现创建计数结果。
    pub fn new(count: u64, reset_after_secs: u64) -> Self {
        Self {
            count,
            reset_after_secs: reset_after_secs.max(1),
        }
    }

    /// 返回当前时间窗口内已消耗的请求数。
    pub const fn count(self) -> u64 {
        self.count
    }

    /// 返回当前时间窗口重置前的秒数。
    pub const fn reset_after_secs(self) -> u64 {
        self.reset_after_secs
    }
}

/// 在固定时间窗口内原子计数 key 的存储契约。
///
/// 实现不得以明文记录或持久化 `key`。集群部署必须选择原子操作范围覆盖所有消耗同一配额的应用节点的存储。
/// 调用方必须在每个 key 中包含稳定的策略/作用域标识，并在使用该 key 的各处采用相同的 `window`。
/// 对 key 哈希只是假名化，不能防止攻击者猜测电子邮件地址等低熵值。
pub trait RateLimitStore: Send + Sync + 'static {
    /// 按首次命中开始的衰减窗口原子增量；旧consume-only driver必须显式提供此能力。
    fn hit<'a>(
        &'a self,
        _key: &'a str,
        _decay: Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
        Box::pin(async { Err(decay_unsupported()) })
    }
    /// 只读仍未到期的计数；不延长窗口、不消耗额度。
    fn attempts<'a>(
        &'a self,
        _key: &'a str,
    ) -> RateLimitFuture<'a, Result<Option<RateLimitBucket>>> {
        Box::pin(async { Err(decay_unsupported()) })
    }
    /// 删除指定衰减策略key的计数与到期状态。
    fn clear<'a>(&'a self, _key: &'a str) -> RateLimitFuture<'a, Result<()>> {
        Box::pin(async { Err(decay_unsupported()) })
    }
    /// 原子地消耗一次请求额度，并返回更新后的计数和重置延迟。
    fn consume<'a>(
        &'a self,
        key: &'a str,
        window: Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>>;
}

/// 可克隆的服务，通过已注册的存储实施固定窗口请求配额。
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
    /// 使用自定义或框架提供的原子存储创建服务。
    pub fn new(store: impl RateLimitStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    /// 增加首次命中衰减计数；key必须与aligned-window策略使用不同作用域。
    pub async fn hit(&self, key: &str, decay: Duration) -> Result<RateLimitBucket> {
        validate_key(key)?;
        if decay.as_secs() == 0 || decay.subsec_nanos() != 0 || decay.as_secs() > 31_536_000 {
            return Err(invalid_policy());
        }
        self.store.hit(key, decay).await
    }
    /// 返回未到期的原始bucket，不递增计数。
    pub async fn attempts(&self, key: &str) -> Result<Option<RateLimitBucket>> {
        validate_key(key)?;
        self.store.attempts(key).await
    }
    /// 清除同一衰减策略key。
    pub async fn clear(&self, key: &str) -> Result<()> {
        validate_key(key)?;
        self.store.clear(key).await
    }
    /// 判断已发生的命中数是否达到max；不消耗一次请求。
    pub async fn too_many_attempts(&self, key: &str, max: u64) -> Result<bool> {
        validate_key(key)?;
        if max == 0 {
            return Err(invalid_policy());
        }
        Ok(self
            .attempts(key)
            .await?
            .is_some_and(|bucket| bucket.count() >= max))
    }
    /// 返回未到期窗口剩余整秒数，缺失或过期为0。
    pub async fn available_in(&self, key: &str) -> Result<u64> {
        Ok(self
            .attempts(key)
            .await?
            .map_or(0, RateLimitBucket::reset_after_secs))
    }

    /// 消耗一次请求额度，并返回配额状态或稳定的限流错误。
    ///
    /// 每个时间窗口内前 `limit` 次调用成功；之后的调用按 HTTP 429 语义失败，并返回存储给出的 `Retry-After` 延迟。
    /// 存储故障会向上传播，绝不会被视为可以放行请求。
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

/// 请求消耗配额后、仍处于配置额度内时返回的成功状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitStatus {
    limit: u64,
    remaining: u64,
    reset_after_secs: u64,
}

impl RateLimitStatus {
    /// 返回此时间窗口最多接受的请求数。
    pub const fn limit(self) -> u64 {
        self.limit
    }

    /// 返回此时间窗口内剩余的可接受请求数。
    pub const fn remaining(self) -> u64 {
        self.remaining
    }

    /// 返回此时间窗口重置前的秒数。
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

fn decay_unsupported() -> Error {
    Error::custom_with_status(
        crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "rate_limit_decay_unsupported",
        "限流存储未实现首次命中衰减计数能力",
    )
}
fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 512 || key.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(Error::custom(
            "invalid_rate_limit_key",
            "rate-limit key must be non-empty and at most 512 bytes",
        ));
    }
    Ok(())
}
fn invalid_policy() -> Error {
    Error::custom(
        "invalid_rate_limit_policy",
        "rate-limit limit and whole-second window must be positive and bounded",
    )
}

#[cfg(test)]
mod decay_tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct DecayStore {
        bucket: Mutex<Option<RateLimitBucket>>,
        fail: bool,
    }
    impl RateLimitStore for DecayStore {
        fn consume<'a>(
            &'a self,
            _: &'a str,
            _: Duration,
        ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
            Box::pin(async { Err(decay_unsupported()) })
        }
        fn hit<'a>(
            &'a self,
            _: &'a str,
            _: Duration,
        ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
            Box::pin(async move {
                if self.fail {
                    return Err(Error::custom("test_store_failed", "存储不可用"));
                }
                let mut bucket = self
                    .bucket
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let new = RateLimitBucket::new(bucket.map_or(1, |b| b.count() + 1), 19);
                *bucket = Some(new);
                Ok(new)
            })
        }
        fn attempts<'a>(
            &'a self,
            _: &'a str,
        ) -> RateLimitFuture<'a, Result<Option<RateLimitBucket>>> {
            Box::pin(async move {
                if self.fail {
                    return Err(Error::custom("test_store_failed", "存储不可用"));
                }
                Ok(*self
                    .bucket
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner))
            })
        }
        fn clear<'a>(&'a self, _: &'a str) -> RateLimitFuture<'a, Result<()>> {
            Box::pin(async move {
                if self.fail {
                    return Err(Error::custom("test_store_failed", "存储不可用"));
                }
                *self
                    .bucket
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn decay_reads_do_not_consume_and_clear_resets_threshold() -> Result<()> {
        let limiter = RateLimiter::new(DecayStore::default());
        assert_eq!(limiter.attempts("decay:login").await?, None);
        assert_eq!(limiter.available_in("decay:login").await?, 0);
        limiter.hit("decay:login", Duration::from_secs(60)).await?;
        assert!(!limiter.too_many_attempts("decay:login", 2).await?);
        assert_eq!(
            limiter
                .attempts("decay:login")
                .await?
                .map(RateLimitBucket::count),
            Some(1)
        );
        limiter.hit("decay:login", Duration::from_secs(60)).await?;
        assert!(limiter.too_many_attempts("decay:login", 2).await?);
        assert_eq!(limiter.available_in("decay:login").await?, 19);
        limiter.clear("decay:login").await?;
        assert_eq!(limiter.attempts("decay:login").await?, None);
        Ok(())
    }
    #[tokio::test]
    async fn decay_rejects_bad_inputs_and_propagates_every_store_failure() -> Result<()> {
        let limiter = RateLimiter::new(DecayStore {
            fail: true,
            ..DecayStore::default()
        });
        assert!(limiter.hit("key", Duration::ZERO).await.is_err());
        assert!(
            limiter
                .hit("key", Duration::from_millis(1500))
                .await
                .is_err()
        );
        assert!(
            limiter
                .hit("key", Duration::from_secs(31_536_001))
                .await
                .is_err()
        );
        assert!(limiter.hit("key", Duration::from_secs(60)).await.is_err());
        assert!(limiter.attempts("key").await.is_err());
        assert!(limiter.clear("key").await.is_err());
        assert!(limiter.too_many_attempts("key", 2).await.is_err());
        assert!(limiter.available_in("key").await.is_err());
        for key in ["", "secret\nvalue"] {
            assert!(limiter.attempts(key).await.is_err());
            assert!(limiter.clear(key).await.is_err());
        }
        assert!(limiter.too_many_attempts("key", 0).await.is_err());
        Ok(())
    }
    #[tokio::test]
    async fn consume_only_store_keeps_compiling_but_explicitly_refuses_new_capability() {
        struct Legacy;
        impl RateLimitStore for Legacy {
            fn consume<'a>(
                &'a self,
                _: &'a str,
                _: Duration,
            ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
                Box::pin(async { Ok(RateLimitBucket::new(1, 1)) })
            }
        }
        let limiter = RateLimiter::new(Legacy);
        assert!(
            limiter
                .check("aligned:key", 1, Duration::from_secs(60))
                .await
                .is_ok()
        );
        for error in [
            limiter
                .hit("decay:key", Duration::from_secs(60))
                .await
                .err(),
            limiter.attempts("decay:key").await.err(),
            limiter.clear("decay:key").await.err(),
        ] {
            assert_eq!(
                error.as_ref().map(Error::status),
                Some(crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            );
        }
    }
}
