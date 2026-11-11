use std::{
    collections::{BTreeSet, HashMap},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    cache::{CacheError, CacheResult, Ttl, Value},
    contracts::cache::{Lock, Repository},
};

#[derive(Clone)]
pub(crate) struct Quota {
    pub key: String,
    pub max: u64,
    pub decay: u64,
    pub charge: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Bucket {
    pub count: u64,
    pub remaining_seconds: u64,
}

pub(crate) enum Admission {
    Allowed(Vec<Bucket>),
    Denied { index: usize, bucket: Bucket },
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Counter {
    count: u64,
    expires: u64,
}

impl Counter {
    fn snapshot(self, now: u64) -> Bucket {
        Bucket {
            count: self.count,
            remaining_seconds: self.expires.saturating_sub(now).div_ceil(1000),
        }
    }
}

pub(crate) fn clock() -> CacheResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| CacheError::InvalidValue)
}

fn millis() -> CacheResult<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CacheError::InvalidValue)?;
    u64::try_from(duration.as_millis()).map_err(|_| CacheError::InvalidValue)
}

pub(crate) fn key(namespace: &str, name: &str, key: &str) -> String {
    use std::fmt::Write as _;
    let input = format!("{namespace}:{}:{name}{key}", name.len());
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, input.as_bytes());
    let mut result = String::from("rate-limiter:");
    for byte in digest.as_ref() {
        write!(result, "{byte:02x}").expect("formatting a string is infallible");
    }
    result
}

pub(crate) fn validate(quotas: &[Quota]) -> CacheResult<()> {
    if quotas.len() > 128
        || quotas
            .iter()
            .any(|quota| !(1..=31_536_000).contains(&quota.decay) || quota.max > i64::MAX as u64)
    {
        return Err(CacheError::InvalidConfiguration);
    }
    Ok(())
}

async fn read(repository: &dyn Repository, key: &str, now: u64) -> CacheResult<Option<Counter>> {
    match repository.get(key).await? {
        None => Ok(None),
        Some(Value::Json(value)) => {
            let counter: Counter =
                serde_json::from_value(value).map_err(|_| CacheError::InvalidValue)?;
            Ok((counter.expires > now).then_some(counter))
        }
        Some(Value::Bytes(_)) => Err(CacheError::InvalidValue),
    }
}

pub(crate) async fn snapshot(repository: &dyn Repository, key: &str) -> CacheResult<Bucket> {
    let now = millis()?;
    Ok(read(repository, key, now)
        .await?
        .map_or_else(Bucket::default, |counter| counter.snapshot(now)))
}

pub(crate) async fn clear(repository: &dyn Repository, key: &str) -> CacheResult<()> {
    let locks = acquire(repository, [key]).await?;
    let result = repository.forget(key).await.map(|_| ());
    finish(result, locks).await
}

pub(crate) async fn admit(repository: &dyn Repository, quotas: &[Quota]) -> CacheResult<Admission> {
    validate(quotas)?;
    let keys: BTreeSet<_> = quotas.iter().map(|quota| quota.key.as_str()).collect();
    let locks = acquire(repository, keys).await?;
    let result = apply(repository, quotas).await;
    finish(result, locks).await
}

async fn acquire<'a>(
    repository: &dyn Repository,
    keys: impl IntoIterator<Item = &'a str>,
) -> CacheResult<Vec<Box<dyn Lock>>> {
    let provider = repository
        .get_store()
        .lock_provider()
        .ok_or(CacheError::LocksUnsupported)?;
    let mut locks = Vec::new();
    for key in keys {
        let result = async {
            let mut lock =
                provider.lock(&format!("{key}:lock"), Some(Duration::from_secs(15)), None)?;
            lock.block(Duration::from_secs(5)).await?;
            Ok(lock)
        }
        .await;
        match result {
            Ok(lock) => locks.push(lock),
            Err(error) => return finish(Err(error), locks).await,
        }
    }
    Ok(locks)
}

async fn finish<T>(mut result: CacheResult<T>, mut locks: Vec<Box<dyn Lock>>) -> CacheResult<T> {
    while let Some(mut lock) = locks.pop() {
        match lock.release().await {
            Ok(true) => {}
            Ok(false) if result.is_ok() => result = Err(CacheError::LockTimeout),
            Err(error) if result.is_ok() => result = Err(error),
            _ => {}
        }
    }
    result
}

async fn apply(repository: &dyn Repository, quotas: &[Quota]) -> CacheResult<Admission> {
    let now = millis()?;
    let mut counters = HashMap::new();
    for (index, quota) in quotas.iter().enumerate() {
        let counter = read(repository, &quota.key, now).await?;
        let bucket = counter.map_or_else(Bucket::default, |counter| counter.snapshot(now));
        if bucket.count >= quota.max {
            return Ok(Admission::Denied { index, bucket });
        }
        counters.insert(quota.key.as_str(), counter);
    }
    for quota in quotas.iter().filter(|quota| quota.charge) {
        let counter = counters
            .get_mut(quota.key.as_str())
            .expect("counter was loaded");
        let counter = counter.get_or_insert(Counter {
            count: 0,
            expires: now.saturating_add(quota.decay * 1000),
        });
        counter.count = counter
            .count
            .checked_add(1)
            .ok_or(CacheError::InvalidValue)?;
        let value =
            Value::Json(serde_json::to_value(*counter).map_err(|_| CacheError::InvalidValue)?);
        let until = UNIX_EPOCH
            .checked_add(Duration::from_millis(counter.expires))
            .ok_or(CacheError::InvalidValue)?;
        if !repository
            .put(&quota.key, &value, Some(Ttl::Until(until)))
            .await?
        {
            return Err(CacheError::InvalidValue);
        }
    }
    Ok(Admission::Allowed(
        quotas
            .iter()
            .map(|quota| {
                counters
                    .get(quota.key.as_str())
                    .and_then(|counter| *counter)
                    .map_or_else(Bucket::default, |counter| counter.snapshot(now))
            })
            .collect(),
    ))
}
