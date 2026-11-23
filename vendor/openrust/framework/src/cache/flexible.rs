use std::{future::Future, time::Duration};

use uuid::Uuid;

use crate::{
    concurrency::enqueue_named,
    contracts::cache::{LockProvider as _, Repository as _},
};

use super::{CacheError, CacheResult, Repository, Ttl, Value, engine, repository::seconds};

const CREATED_KEY: &str = "illuminate:cache:flexible:created:";
const REFRESH_LOCK: &str = "illuminate:cache:flexible:lock:";

/// Options for the lock and deferred refresh used by `flexible_with`.
#[derive(Debug, Default)]
pub struct FlexibleOptions {
    pub lock_for: Duration,
    pub owner: Option<String>,
    pub always_defer: bool,
}

impl Repository {
    /// Returns fresh or stale cache data. The durations are in seconds from the last write.
    pub async fn flexible<F>(
        &self,
        key: &str,
        ttl: [i64; 2],
        resolve: impl FnOnce() -> F + Send + 'static,
    ) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>> + Send + 'static,
    {
        self.flexible_with(
            key,
            ttl.map(Ttl::Seconds),
            resolve,
            FlexibleOptions::default(),
        )
        .await
    }

    /// Schedules a stale refresh during application termination, optionally after failure.
    pub async fn flexible_with<F>(
        &self,
        key: &str,
        [fresh_until, expires_at]: [Ttl; 2],
        resolve: impl FnOnce() -> F + Send + 'static,
        options: FlexibleOptions,
    ) -> CacheResult<Value>
    where
        F: Future<Output = CacheResult<Value>> + Send + 'static,
    {
        let created_key = format!("{CREATED_KEY}{key}");
        let mut entries = self.many(&[key, &created_key]).await?.into_iter();
        let value = entries.next().flatten().filter(|value| !value.is_null());
        let revision = entries.next().flatten();
        let Some((value, (revision, created))) = value.zip(revision.and_then(revision_time)) else {
            let value = resolve().await?;
            self.put_flexible(key.to_owned(), created_key, value.clone(), expires_at)
                .await?;
            return Ok(value);
        };
        let now = engine::now()?;
        if seconds(fresh_until).is_some_and(|ttl| {
            (now.saturating_sub(created).max(0) as u64) < ttl.get().saturating_mul(1000)
        }) {
            return Ok(value);
        }

        let repository = self.clone();
        let name = format!("illuminate:cache:flexible:{key}");
        let key = key.to_owned();
        let FlexibleOptions {
            lock_for,
            owner,
            always_defer,
        } = options;
        let queued = enqueue_named(
            name,
            Box::pin(async move {
                let lock = repository.lock(
                    &format!("{REFRESH_LOCK}{key}"),
                    Some(lock_for),
                    owner.as_deref(),
                )?;
                lock.get_with(|| async {
                    if repository.get(&created_key).await?.as_ref() == Some(&revision) {
                        let value = resolve().await?;
                        repository
                            .put_flexible(key, created_key, value, expires_at)
                            .await?;
                    }
                    Ok::<_, CacheError>(())
                })
                .await?;
                Ok::<_, CacheError>(())
            }),
        );
        // Repository calls outside a framework lifecycle still return stale data.
        if let Ok(callback) = queued
            && always_defer
        {
            let _ = callback.always();
        }
        Ok(value)
    }

    async fn put_flexible(
        &self,
        key: String,
        created_key: String,
        value: Value,
        ttl: Ttl,
    ) -> CacheResult<()> {
        let revision = Value::from(Uuid::now_v7().to_string().as_str());
        self.put_many(&[(key, value), (created_key, revision)], Some(ttl))
            .await?;
        Ok(())
    }
}

fn revision_time(revision: Value) -> Option<(Value, i64)> {
    let timestamp = Uuid::parse_str(revision.as_str()?).ok()?.get_timestamp()?;
    let (seconds, nanos) = timestamp.to_unix();
    let millis = seconds
        .checked_mul(1000)?
        .checked_add(u64::from(nanos / 1_000_000))?;
    Some((revision, millis.try_into().ok()?))
}
