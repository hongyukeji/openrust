//! Tag generations provide bounded logical invalidation without scanning keys.

use std::{fmt::Write as _, time::Duration};

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use super::{CacheBackend, CacheStore, Error, Result, normalize_ttl};

/// 可选的原子 tag generation backend 能力。
#[async_trait]
pub trait CacheTagBackend: Send + Sync {
    /// 返回 tag 当前 generation；从未使用的 tag 必须返回 `0`。
    async fn generation(&self, tag_key: &str) -> Result<u64>;
    /// 原子递增 tag generation。
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

/// 绑定 store 与一组 tag 的缓存视图。
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
}

/// 已捕获 generation 的条目句柄，适用于先读、后加载再写的流程。
#[derive(Clone)]
pub struct TaggedCacheEntry {
    store: CacheStore,
    key: String,
    generations: Vec<(String, u64)>,
}

impl TaggedCacheEntry {
    /// 读取捕获 generation 对应的 JSON 值。
    pub async fn get<T: DeserializeOwned>(&self) -> Result<Option<T>> {
        let key = self.physical_key()?;
        self.store
            .backend
            .get(&key)
            .await?
            .map(|bytes| {
                serde_json::from_slice(&bytes).map_err(|_| {
                    Error::custom(
                        "cache_deserialization_failed",
                        "cached value could not be decoded",
                    )
                })
            })
            .transpose()
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
        self.store.backend.remove(&self.physical_key()?).await
    }

    fn physical_key(&self) -> Result<String> {
        let logical = self.store.key(&self.key)?;
        let mut material = format!("{}:{logical}", self.store.store_name);
        for (tag, generation) in &self.generations {
            material.push(':');
            material.push_str(tag);
            material.push(':');
            material.push_str(&generation.to_string());
        }
        Ok(format!("__openrust_cache_tag_v1:{}", digest(&material)))
    }
}

pub(super) async fn invalidate_tags(store: &CacheStore, tags: &[&str]) -> Result<()> {
    let tags = validate_tags(tags)?;
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
    let mut tags = tags.to_vec();
    if tags
        .iter()
        .any(|tag| tag.is_empty() || tag.len() > 128 || tag.chars().any(char::is_control))
    {
        return Err(Error::custom(
            "cache_invalid_tag",
            "cache tags must contain 1 to 128 non-control bytes",
        ));
    }
    tags.sort_unstable();
    tags.dedup();
    Ok(tags.into_iter().map(str::to_owned).collect())
}

fn tag_key(store: &CacheStore, tag: &str) -> String {
    format!(
        "__openrust_cache_tag_meta_v1:{}",
        digest(&format!("{}:{}:{tag}", store.store_name, store.namespace))
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
