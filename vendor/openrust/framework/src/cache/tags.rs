//! 有序标签的代际隔离与后台刷新。

use std::{fmt::Write as _, future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{CacheBackend, CacheStore, Error, Result, normalize_ttl};

/// 可选的原子 tag generation backend 能力。
#[async_trait]
pub trait CacheTagBackend: Send + Sync {
    /// 返回该后端用于跨连接计算新鲜期的 Unix 毫秒时钟；默认实现读取本机系统时钟。
    async fn now_millis(&self) -> Result<u64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::custom("cache_clock_unavailable", "system clock is unavailable"))?
            .as_millis()
            .try_into()
            .map_err(|_| Error::custom("cache_clock_unavailable", "system clock is out of range"))
    }
    /// 返回 tag 当前 generation，首次访问可初始化一个稳定值。
    async fn generation(&self, tag_key: &str) -> Result<u64>;
    /// 原子更新 tag generation，使旧值失效。可同时清理后端维护的旧条目。
    async fn invalidate(&self, tag_key: &str) -> Result<()>;
    /// 仅当所列 generation 全部仍匹配时写入有限 TTL 的值。
    async fn set_if_generations_match(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
        generations: &[(String, u64)],
    ) -> Result<bool>;
}

/// 绑定 store 与一组有序标签的缓存视图。
#[derive(Clone)]
pub struct TaggedCacheStore {
    store: CacheStore,
    tags: Vec<String>,
}

impl TaggedCacheStore {
    pub(super) fn new(store: CacheStore, tags: &[&str]) -> Result<Self> {
        let tags = validate_tags(tags)?;
        if store.backend.tag_backend().is_none() {
            return Err(unsupported());
        }
        Ok(Self { store, tags })
    }

    /// 捕获当前 generation，供一次读或一段可能较慢的加载使用。
    pub async fn entry(&self, key: &str) -> Result<TaggedCacheEntry> {
        let generations = capture_generations(&self.store, &self.tags).await?;
        Ok(TaggedCacheEntry {
            store: self.store.clone(),
            key: key.to_owned(),
            generations,
            tenant_id: crate::tenant::TenantContext::current()
                .map(|tenant| tenant.as_str().to_owned()),
        })
    }

    /// 读取当前 generation 的 JSON 值。
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.entry(key).await?.get().await
    }

    /// 写入当前 generation；tagged 条目必须有有限 TTL。
    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) -> Result<()> {
        self.entry(key).await?.set(value, ttl).await
    }

    /// 使包含视图中任一标签的条目失效。
    ///
    /// Redis 同时分批删除已索引的值。后端故障会返回错误，已完成的失效不会回滚。
    pub async fn flush(&self) -> Result<()> {
        let tags: Vec<&str> = self.tags.iter().map(String::as_str).collect();
        self.store.invalidate_tags(&tags).await
    }

    /// 在新鲜期返回当前值，在有限 stale 窗口内立即返回旧值并安排后台刷新；
    /// 超过窗口后等待加载，且 tag generation 变化会使旧值与旧刷新失效。
    pub async fn get_stale_while_revalidate<T, F, Fut>(
        &self,
        key: &str,
        load_id: &str,
        fresh_ttl: Duration,
        stale_ttl: Duration,
        loader: F,
    ) -> Result<T>
    where
        T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        super::validate_load_id(load_id)?;
        let fresh_ttl = normalize_ttl(fresh_ttl)?;
        let stale_ttl = normalize_ttl(stale_ttl)?;
        let total_ttl = fresh_ttl.checked_add(stale_ttl).ok_or_else(|| {
            Error::custom(
                "cache_invalid_ttl",
                "SWR cache TTL exceeds supported limits",
            )
        })?;
        let total_ttl = normalize_ttl(total_ttl)?;
        let fresh_ms = u64::try_from(fresh_ttl.as_millis()).map_err(|_| {
            Error::custom(
                "cache_invalid_ttl",
                "SWR fresh TTL exceeds supported limits",
            )
        })?;
        let store = &self.store;
        loop {
            let entry = self.entry(key).await?;
            let flight_key = store.flight_key::<T>(&entry.physical_key()?, load_id);
            if let Some(cached) = entry.get_backend::<SwrValue<T>>().await?
                && entry.is_current().await?
            {
                if entry.now_millis().await? < cached.fresh_until_ms {
                    return Ok(cached.value);
                }
                self.schedule_refresh(entry, &flight_key, total_ttl, fresh_ms, loader);
                return Ok(cached.value);
            }

            match store.singleflight.begin(flight_key.clone())? {
                super::FlightRole::Leader(guard) => {
                    return load_swr_leader(&entry, guard, store, fresh_ms, total_ttl, &loader)
                        .await;
                }
                super::FlightRole::Waiter(waiter) => {
                    if let Some(bytes) = waiter.wait(store.singleflight.loader_timeout).await?
                        && entry.is_current().await?
                    {
                        return serde_json::from_slice::<SwrValue<T>>(&bytes)
                            .map(|cached| cached.value)
                            .map_err(|_| {
                                Error::custom(
                                    "cache_deserialization_failed",
                                    "cached value could not be decoded",
                                )
                            });
                    }
                }
            }
        }
    }

    fn schedule_refresh<T, F, Fut>(
        &self,
        entry: TaggedCacheEntry,
        flight_key: &str,
        total_ttl: Duration,
        fresh_ms: u64,
        loader: F,
    ) where
        T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        let store = self.store.clone();
        let Ok(Some(guard)) = store.singleflight.try_begin_refresh(flight_key.to_owned()) else {
            return;
        };
        let Some(permit) = store.refresh_limiter.acquire() else {
            drop(guard);
            return;
        };
        let shared_locks = store.shared_locks.clone();
        let lease_ttl = store.singleflight.loader_timeout + Duration::from_secs(5);
        let lock_key = format!("cache:swr:{}", digest(flight_key));
        let timeout = store.singleflight.loader_timeout;
        let spawn = store
            .background_tasks
            .spawn(move |cancellation| async move {
                let _permit = permit;
                let Some(current) = run_refresh(
                    &entry,
                    &loader,
                    fresh_ms,
                    total_ttl,
                    timeout,
                    &cancellation,
                    shared_locks.as_ref(),
                    &lock_key,
                    lease_ttl,
                )
                .await
                else {
                    guard.finish(super::FlightState::Failed);
                    return;
                };
                let Ok(bytes) = serde_json::to_vec(&current) else {
                    guard.finish(super::FlightState::Failed);
                    return;
                };
                guard.finish(super::FlightState::Ready(Arc::from(bytes)));
            });
        if let Err(error) = spawn {
            tracing::debug!(
                code = error.code(),
                "background cache refresh was not scheduled"
            );
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct SwrValue<T> {
    value: T,
    fresh_until_ms: u64,
}

async fn load_swr_leader<T, F, Fut>(
    entry: &TaggedCacheEntry,
    guard: super::FlightGuard,
    store: &CacheStore,
    fresh_ms: u64,
    total_ttl: Duration,
    loader: &F,
) -> Result<T>
where
    T: Serialize + DeserializeOwned + Clone,
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    if let Some(cached) = entry.get_backend::<SwrValue<T>>().await?
        && entry.is_current().await?
    {
        let bytes = encode_swr(&cached)?;
        guard.finish(super::FlightState::Ready(Arc::from(bytes)));
        return Ok(cached.value);
    }
    let value = match tokio::time::timeout(store.singleflight.loader_timeout, loader()).await {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            guard.finish(super::FlightState::Failed);
            return Err(error);
        }
        Err(_) => {
            guard.finish(super::FlightState::Failed);
            return Err(Error::custom(
                "cache_loader_timeout",
                "cache loader exceeded its configured timeout",
            ));
        }
    };
    let cached = SwrValue {
        value,
        fresh_until_ms: entry.now_millis().await?.saturating_add(fresh_ms),
    };
    let bytes = encode_swr(&cached)?;
    if let Err(error) = entry.set(&cached, total_ttl).await {
        guard.finish(super::FlightState::Failed);
        return Err(error);
    }
    guard.finish(super::FlightState::Ready(Arc::from(bytes)));
    Ok(cached.value)
}

fn encode_swr<T: Serialize>(value: &SwrValue<T>) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| {
        Error::custom(
            "cache_serialization_failed",
            "cache value could not be encoded",
        )
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "刷新仅依赖本次调用捕获的缓存与租约上下文"
)]
async fn run_refresh<T, F, Fut>(
    entry: &TaggedCacheEntry,
    loader: &F,
    fresh_ms: u64,
    total_ttl: Duration,
    timeout: Duration,
    cancellation: &crate::core::Cancellation,
    shared_locks: Option<&crate::database::SharedLocks>,
    lock_key: &str,
    lease_ttl: Duration,
) -> Option<SwrValue<T>>
where
    T: Serialize + DeserializeOwned + Clone,
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let lease = if let Some(locks) = shared_locks {
        match locks.acquire(lock_key, lease_ttl).await {
            Ok(crate::database::LockAcquire::Acquired(lease)) => Some((locks, lease)),
            Ok(crate::database::LockAcquire::Busy { .. }) => {
                return wait_for_shared_refresh(entry, cancellation, timeout).await;
            }
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "shared cache refresh lock unavailable; proceeding locally"
                );
                None
            }
        }
    } else {
        None
    };
    let current = match entry.is_current().await {
        Ok(current) => current,
        Err(error) => {
            tracing::debug!(code = error.code(), "cache refresh tag check failed");
            false
        }
    };
    if !current {
        if let Some((locks, lease)) = lease
            && let Err(error) = locks.release(&lease).await
        {
            tracing::debug!(
                code = error.code(),
                "shared cache refresh lock release failed"
            );
        }
        return None;
    }
    let refreshed = tokio::select! {
        () = cancellation.cancelled() => None,
        result = tokio::time::timeout(timeout, loader()) => match result {
            Ok(Ok(value)) => Some(value),
            Ok(Err(error)) => {
                tracing::debug!(code = error.code(), "background cache refresh failed");
                None
            }
            Err(_) => {
                tracing::debug!("background cache refresh timed out");
                None
            }
        }
    };
    let result = if let Some(value) = refreshed {
        let fresh_until_ms = match entry.now_millis().await {
            Ok(now) => Some(now.saturating_add(fresh_ms)),
            Err(error) => {
                tracing::debug!(code = error.code(), "cache refresh clock unavailable");
                None
            }
        };
        let Some(fresh_until_ms) = fresh_until_ms else {
            if let Some((locks, lease)) = lease
                && let Err(error) = locks.release(&lease).await
            {
                tracing::debug!(
                    code = error.code(),
                    "shared cache refresh lock release failed"
                );
            }
            return None;
        };
        let cached = SwrValue {
            value,
            fresh_until_ms,
        };
        match entry.set(&cached, total_ttl).await {
            Ok(()) => Some(cached),
            Err(error) => {
                tracing::debug!(code = error.code(), "background cache refresh write failed");
                None
            }
        }
    } else {
        None
    };
    if let Some((locks, lease)) = lease
        && let Err(error) = locks.release(&lease).await
    {
        tracing::debug!(
            code = error.code(),
            "shared cache refresh lock release failed"
        );
    }
    result
}

async fn wait_for_shared_refresh<T>(
    entry: &TaggedCacheEntry,
    cancellation: &crate::core::Cancellation,
    timeout: Duration,
) -> Option<SwrValue<T>>
where
    T: DeserializeOwned,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(value) = entry.get_backend::<SwrValue<T>>().await.ok().flatten()
            && entry.is_current().await.ok() == Some(true)
            && entry
                .now_millis()
                .await
                .is_ok_and(|now| now < value.fresh_until_ms)
        {
            return Some(value);
        }
        tokio::select! {
            () = cancellation.cancelled() => return None,
            () = tokio::time::sleep_until(deadline) => return None,
            () = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
    }
}

/// 已捕获 generation 的条目句柄，适用于先读、后加载再写的流程。
#[derive(Clone)]
pub struct TaggedCacheEntry {
    store: CacheStore,
    key: String,
    generations: Vec<(String, u64)>,
    tenant_id: Option<String>,
}

impl TaggedCacheEntry {
    /// 读取捕获 generation 对应的 JSON 值。
    pub async fn get<T: DeserializeOwned>(&self) -> Result<Option<T>> {
        self.store.get_at(&self.physical_key()?).await
    }

    // SWR 依据后端时钟判断新鲜度，写入仍走同一条 memo 失效路径。
    async fn get_backend<T: DeserializeOwned>(&self) -> Result<Option<T>> {
        CacheStore::decode(self.store.backend.get(&self.physical_key()?).await?)
    }

    /// 仅在捕获的 tag generation 未变化时写入，TTL 必须大于零且有限。
    pub async fn set<T: Serialize>(&self, value: &T, ttl: Duration) -> Result<()> {
        let ttl = normalize_ttl(ttl)?;
        let bytes = serde_json::to_vec(value).map_err(|_| {
            Error::custom(
                "cache_serialization_failed",
                "cache value could not be encoded",
            )
        })?;
        let key = self.physical_key()?;
        let backend = tag_backend(self.store.backend.as_ref())?;
        let _write = self.store.memo.as_ref().map(|memo| memo.begin_write(&key));
        if backend
            .set_if_generations_match(&key, bytes, ttl, &self.generations)
            .await?
        {
            Ok(())
        } else {
            Err(Error::custom(
                "cache_tag_invalidated",
                "cache tag changed while the value was being prepared",
            ))
        }
    }

    /// 删除该 generation 下的值。
    pub async fn remove(&self) -> Result<()> {
        self.store.remove_at(&self.physical_key()?).await
    }

    async fn is_current(&self) -> Result<bool> {
        let backend = tag_backend(self.store.backend.as_ref())?;
        for (key, generation) in &self.generations {
            if backend.generation(key).await? != *generation {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn now_millis(&self) -> Result<u64> {
        tag_backend(self.store.backend.as_ref())?.now_millis().await
    }

    fn physical_key(&self) -> Result<String> {
        let logical = self
            .store
            .key_for_tenant(&self.key, self.tenant_id.as_deref())?;
        // 长度界定逻辑键，防止键尾伪装成标签段。
        let mut material = format!("{}:{}:{logical}", self.store.store_name, logical.len());
        for (tag, generation) in &self.generations {
            material.push(':');
            material.push_str(tag);
            material.push(':');
            material.push_str(&generation.to_string());
        }
        Ok(format!("__cache_tag_v2:{}", digest(&material)))
    }
}

pub(super) async fn invalidate_tags(store: &CacheStore, tags: &[&str]) -> Result<()> {
    let mut tags = validate_tags(tags)?;
    tags.sort_unstable();
    tags.dedup();
    let backend = tag_backend(store.backend.as_ref())?;
    for tag in tags {
        backend.invalidate(&tag_key(store, &tag)).await?;
    }
    Ok(())
}

async fn capture_generations(store: &CacheStore, tags: &[String]) -> Result<Vec<(String, u64)>> {
    let backend = tag_backend(store.backend.as_ref())?;
    let mut generations = Vec::with_capacity(tags.len());
    for tag in tags {
        let key = tag_key(store, tag);
        generations.push((key.clone(), backend.generation(&key).await?));
    }
    Ok(generations)
}

fn tag_backend(backend: &dyn CacheBackend) -> Result<&dyn CacheTagBackend> {
    backend.tag_backend().ok_or_else(unsupported)
}

fn validate_tags(tags: &[&str]) -> Result<Vec<String>> {
    if tags.is_empty() || tags.len() > 16 {
        return Err(Error::custom(
            "cache_invalid_tags",
            "cache tag sets must contain between 1 and 16 tags",
        ));
    }
    if tags
        .iter()
        .any(|tag| tag.is_empty() || tag.len() > 128 || tag.chars().any(char::is_control))
    {
        return Err(Error::custom(
            "cache_invalid_tag",
            "cache tags must contain 1 to 128 non-control bytes",
        ));
    }
    Ok(tags.iter().copied().map(str::to_owned).collect())
}

fn tag_key(store: &CacheStore, tag: &str) -> String {
    let tenant = crate::tenant::TenantContext::current()
        .map(|tenant| tenant.as_str().to_owned())
        .unwrap_or_default();
    format!(
        "__cache_tag_meta_v2:{}",
        digest(&format!(
            "{}:{}:{}:{}:{}:{tag}",
            store.store_name,
            store.namespace.len(),
            store.namespace,
            tenant.len(),
            tenant,
        ))
    )
}

fn digest(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let bytes = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn unsupported() -> Error {
    Error::custom(
        "cache_tag_unsupported",
        "the configured cache driver does not support tag invalidation",
    )
}

#[cfg(test)]
#[path = "../../tests/unit/cache/tag_identity.rs"]
mod identity_tests;
