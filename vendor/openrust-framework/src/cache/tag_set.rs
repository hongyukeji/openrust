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

    pub fn get_names(&self) -> &[String] {
        &self.names
    }

    pub async fn get_namespace(&self) -> CacheResult<String> {
        if let Some(redis) = &self.redis {
            return redis.namespace().await;
        }
        let mut generations = Vec::with_capacity(self.names.len());
        for name in &self.names {
            generations.push(self.tag_id(name).await?);
        }
        Ok(generations.join("|"))
    }

    pub async fn reset(&self) -> CacheResult<()> {
        for name in &self.names {
            self.reset_tag(name).await?;
        }
        Ok(())
    }

    pub async fn flush(&self) -> CacheResult<()> {
        for name in &self.names {
            self.flush_tag(name).await?;
        }
        Ok(())
    }

    pub async fn tag_id(&self, name: &str) -> CacheResult<String> {
        if let Some(redis) = &self.redis {
            return redis.tag_id(name).await;
        }
        let key = self.tag_key(name);
        if let Some(value) = self.store.get(&key).await? {
            return decode_generation(&value);
        }
        let id = generation();
        if !self
            .store
            .add(&key, &Value::from(id.as_str()), None)
            .await?
            && let Some(value) = self.store.get(&key).await?
        {
            return decode_generation(&value);
        }
        // A store such as NullStore can decline to persist the generation.
        Ok(id)
    }

    pub async fn reset_tag(&self, name: &str) -> CacheResult<String> {
        if let Some(redis) = &self.redis {
            return redis.flush(name).await;
        }
        let id = generation();
        self.store
            .forever(&self.tag_key(name), &Value::from(id.as_str()))
            .await?;
        Ok(id)
    }

    pub async fn flush_tag(&self, name: &str) -> CacheResult<()> {
        if let Some(redis) = &self.redis {
            redis.flush(name).await?;
        } else {
            self.store.forget(&self.tag_key(name)).await?;
        }
        Ok(())
    }

    pub fn tag_key(&self, name: &str) -> String {
        self.redis.as_ref().map_or_else(
            || format!("tag:{}:key", key_digest(name)),
            |redis| redis.tag_key(name),
        )
    }
}

fn decode_generation(value: &Value) -> CacheResult<String> {
    value
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .map(str::to_owned)
        .ok_or(CacheError::InvalidValue)
}

pub(super) fn generation() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}
