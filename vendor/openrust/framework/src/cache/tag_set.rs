use std::sync::Arc;

use crate::contracts::cache::Store;

use super::{
    CacheError, CacheResult, RedisStore, Value, engine::key_digest, redis_tag_set::RedisTagSet,
};

/// Tracks the generation of each tag in an ordered tag list.
pub struct TagSet {
    pub(super) store: Arc<dyn Store>,
    names: Vec<String>,
    pub(super) redis: Option<RedisTagSet>,
}

#[allow(non_snake_case, reason = "Matches Laravel's TagSet API")]
impl TagSet {
    pub fn new(store: Arc<dyn Store>, names: &[&str]) -> Self {
        Self {
            store,
            names: names.iter().map(|&name| name.to_owned()).collect(),
            redis: None,
        }
    }

    pub(super) fn redis(store: Arc<RedisStore>, names: &[&str]) -> Self {
        let redis = RedisTagSet::new(&store, names);
        let mut tags = Self::new(store, names);
        tags.redis = Some(redis);
        tags
    }

    pub fn getNames(&self) -> &[String] {
        &self.names
    }

    pub async fn getNamespace(&self) -> CacheResult<String> {
        if let Some(redis) = &self.redis {
            return redis.namespace().await;
        }
        let mut generations = Vec::with_capacity(self.names.len());
        for name in &self.names {
            generations.push(self.tagId(name).await?);
        }
        Ok(generations.join("|"))
    }

    pub async fn reset(&self) -> CacheResult<()> {
        for name in &self.names {
            self.resetTag(name).await?;
        }
        Ok(())
    }

    pub async fn flush(&self) -> CacheResult<()> {
        for name in &self.names {
            self.flushTag(name).await?;
        }
        Ok(())
    }

    pub async fn tagId(&self, name: &str) -> CacheResult<String> {
        if let Some(redis) = &self.redis {
            return redis.tag_id(name).await;
        }
        let key = self.tagKey(name);
        loop {
            if let Some(value) = self.store.get(&key).await? {
                return value
                    .as_str()
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .map(str::to_owned)
                    .ok_or(CacheError::InvalidValue);
            }

            let id = generation();
            if self
                .store
                .add(&key, &Value::from(id.as_str()), None)
                .await?
            {
                return Ok(id);
            }
        }
    }

    pub async fn resetTag(&self, name: &str) -> CacheResult<String> {
        if let Some(redis) = &self.redis {
            return redis.flush(name).await;
        }
        let id = generation();
        if !self
            .store
            .forever(&self.tagKey(name), &Value::from(id.as_str()))
            .await?
        {
            return Err(CacheError::InvalidValue);
        }
        Ok(id)
    }

    pub async fn flushTag(&self, name: &str) -> CacheResult<()> {
        if let Some(redis) = &self.redis {
            redis.flush(name).await?;
        } else {
            self.store.forget(&self.tagKey(name)).await?;
        }
        Ok(())
    }

    pub fn tagKey(&self, name: &str) -> String {
        self.redis.as_ref().map_or_else(
            || format!("tag:{}:key", key_digest(name)),
            |redis| redis.tag_key(name),
        )
    }
}

pub(super) fn generation() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}
