use std::time::SystemTime;

use http::{HeaderMap, header};
use openrust::{
    cache::{Cache, CacheError, CacheResult, Ttl, Value, limiters::ConcurrencyLease},
    contracts::cache::Repository as _,
    http::client::RetryAfter,
};
use sha2::{Digest, Sha256};

use crate::budget::Budget;

pub(crate) const MAX_SECONDS: i64 = 86_400;
const MILLIS_PER_SECOND: i64 = 1_000;

pub(crate) struct Account {
    pub(crate) cooldown: Option<AccountCooldown>,
    pub(crate) budget: Option<Budget>,
    concurrency: Option<(String, usize)>,
}

pub(crate) enum Admission {
    Allowed(Option<ConcurrencyLease>),
    Busy,
}

impl Account {
    pub(crate) fn new(
        name: &str,
        cooldown: Option<i64>,
        concurrency: Option<usize>,
        budget: Option<Budget>,
    ) -> Self {
        let digest = format!("{:x}", Sha256::digest(name));
        Self {
            cooldown: cooldown.map(|seconds| AccountCooldown::new(&digest, seconds)),
            budget,
            concurrency: concurrency
                .map(|maximum| (format!("ai-gateway:account:concurrency:{digest}:"), maximum)),
        }
    }

    pub(crate) async fn acquire(&self, store: Option<&str>) -> CacheResult<Admission> {
        let Some((name, maximum)) = &self.concurrency else {
            return Ok(Admission::Allowed(None));
        };
        let lease = Cache::store(store)?
            .funnel(name)?
            .limit(*maximum)
            .release_after(120)
            .block(0)
            .acquire()
            .await?;
        Ok(match lease {
            Some(lease) => Admission::Allowed(Some(lease)),
            None => Admission::Busy,
        })
    }
}

pub(crate) struct AccountCooldown {
    key: String,
    lock_key: String,
    seconds: i64,
}

impl AccountCooldown {
    fn new(account_digest: &str, seconds: i64) -> Self {
        let key = format!("ai-gateway:account:cooldown:{account_digest}");
        Self {
            lock_key: format!("{key}:lock"),
            key,
            seconds,
        }
    }

    pub(crate) fn deadline(&self, headers: &HeaderMap) -> Option<i64> {
        let now = timestamp_millis();
        let mut values = headers.get_all(header::RETRY_AFTER).iter();
        let hint = values.next().and_then(|value| {
            if values.next().is_some() {
                return None;
            }
            RetryAfter::from_header(value)
        });
        let delay = |seconds: i64| now.saturating_add(seconds * MILLIS_PER_SECOND);
        let until = match hint {
            Some(RetryAfter::Timestamp(timestamp)) => timestamp.saturating_mul(MILLIS_PER_SECOND),
            Some(RetryAfter::Seconds(seconds)) => delay(seconds.min(MAX_SECONDS as u64) as i64),
            None => delay(self.seconds),
        }
        .min(delay(MAX_SECONDS));
        (until > now).then_some(until)
    }

    pub(crate) async fn remaining(&self, store: Option<&str>) -> CacheResult<Option<u64>> {
        let remaining = Cache::store(store)?
            .get(&self.key)
            .await?
            .as_ref()
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .saturating_sub(timestamp_millis());
        Ok((remaining > 0).then(|| ceil_seconds(remaining) as u64))
    }

    pub(crate) async fn pause(&self, store: Option<&str>, until: i64) -> CacheResult<()> {
        let repository = Cache::store(store)?;
        repository
            .without_overlapping_with(
                &self.lock_key,
                || async {
                    let existing = repository
                        .get(&self.key)
                        .await?
                        .as_ref()
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    let remaining = until.saturating_sub(timestamp_millis());
                    if until > existing
                        && remaining > 0
                        && !repository
                            .put(
                                &self.key,
                                &until.into(),
                                Some(Ttl::Seconds(ceil_seconds(remaining))),
                            )
                            .await?
                    {
                        return Err(CacheError::InvalidValue);
                    }
                    Ok(())
                },
                5,
                1,
                None,
            )
            .await
    }
}

fn timestamp_millis() -> i64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn ceil_seconds(millis: i64) -> i64 {
    millis.saturating_add(MILLIS_PER_SECOND - 1) / MILLIS_PER_SECOND
}
