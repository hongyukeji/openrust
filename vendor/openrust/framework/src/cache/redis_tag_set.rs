mod cluster;

use std::sync::Arc;

use ::redis::{FromRedisValue, ToRedisArgs};

use crate::redis::connections::Connection;

use super::{
    CacheError, CacheResult, RedisStore, backend, engine::key_digest, tag_set::generation,
};

const ITEM: &str = concat!(
    include_str!("scripts/redis_counter.lua"),
    include_str!("scripts/redis_tagged_item_operations.lua"),
    include_str!("scripts/redis_tagged_item.lua"),
);
const RETIRE: &str = include_str!("scripts/redis_retire_tag.lua");
const PRUNE: &str = include_str!("scripts/redis_prune_tag.lua");
const DRAIN: &str = include_str!("scripts/redis_drain_tag.lua");
const BATCH_SIZE: usize = 128;

pub(super) struct RedisTagSet {
    connection: Arc<Connection>,
    prefix: String,
    tags: Vec<String>,
}

impl RedisTagSet {
    pub(super) fn new(store: &RedisStore, names: &[&str]) -> Self {
        let prefix = store.data.key(&store.key("tagged:v1:"));
        let prefix = if store.data.is_cluster() {
            format!("{{{}}}:{prefix}", key_digest(&prefix))
        } else {
            prefix
        };
        Self {
            connection: Arc::clone(&store.data),
            prefix,
            tags: names.iter().map(|name| key_digest(name)).collect(),
        }
    }

    pub(super) async fn item<T: FromRedisValue>(
        &self,
        operation: &str,
        key: &str,
        input: impl ToRedisArgs + Send,
        option: impl ToRedisArgs + Send,
    ) -> CacheResult<T> {
        if self.connection.is_cluster() {
            return self.cluster_item(operation, key, input, option).await;
        }
        self.connection
            .query(
                ::redis::cmd("EVAL")
                    .arg(ITEM)
                    .arg(0)
                    .arg(&self.prefix)
                    .arg(generation())
                    .arg(operation)
                    .arg(key)
                    .arg(input)
                    .arg(option)
                    .arg(&self.tags),
            )
            .await
            .map_err(backend)
    }

    pub(super) async fn namespace(&self) -> CacheResult<String> {
        if self.connection.is_cluster() {
            return self.cluster_namespace().await;
        }
        self.item("namespace", "", "", "").await
    }

    pub(super) async fn item_key(&self, key: &str) -> CacheResult<String> {
        if self.connection.is_cluster() {
            return self.cluster_item_key(key).await;
        }
        self.item("key", key, "", "").await
    }

    pub(super) fn tag_key(&self, name: &str) -> String {
        format!("{}generation:{}", self.prefix, key_digest(name))
    }

    pub(super) async fn tag_id(&self, name: &str) -> CacheResult<String> {
        let digest = key_digest(name);
        let tags = Self {
            connection: Arc::clone(&self.connection),
            prefix: self.prefix.clone(),
            tags: vec![digest],
        };
        tags.namespace().await
    }

    pub(super) async fn prune(&self) -> CacheResult<()> {
        if self.connection.is_cluster() {
            return self.cluster_prune().await;
        }
        let pattern = self.index_pattern();
        let value_prefix = format!("{}value:", self.prefix);
        let mut cursor = 0;
        loop {
            let (next, indexes): (u64, Vec<String>) = self
                .connection
                .query(::redis::cmd("SCAN").arg(cursor).arg("MATCH").arg(&pattern))
                .await
                .map_err(backend)?;
            for index in indexes {
                self.prune_index(&index, &value_prefix).await?;
            }
            if next == 0 {
                return Ok(());
            }
            cursor = next;
        }
    }

    fn index_pattern(&self) -> String {
        let mut pattern = String::new();
        for character in self.prefix.chars() {
            if matches!(character, '*' | '?' | '[' | ']' | '\\') {
                pattern.push('\\');
            }
            pattern.push(character);
        }
        pattern.push_str("index:*");
        pattern
    }

    pub(super) async fn prune_selected(&self) -> CacheResult<()> {
        let value_prefix = format!("{}value:", self.prefix);
        for tag in &self.tags {
            let metadata = format!("{}generation:{tag}", self.prefix);
            let id: Option<String> = self
                .connection
                .query_slot(::redis::cmd("GET").arg(&metadata))
                .await
                .map_err(backend)?;
            let Some(id) = id else { continue };
            if !valid_digest(&id, 32) {
                return Err(CacheError::InvalidValue);
            }
            let index = format!("{}index:{tag}:{id}", self.prefix);
            if self.connection.is_cluster() {
                let pending = format!("{}pending:{tag}", self.prefix);
                self.prune_cluster_index(&self.registry(), &index, &pending, &id)
                    .await?;
            } else {
                self.prune_index(&index, &value_prefix).await?;
            }
        }
        Ok(())
    }

    async fn prune_index(&self, index: &str, value_prefix: &str) -> CacheResult<()> {
        let mut cursor = 0;
        loop {
            cursor = self
                .connection
                .query(
                    ::redis::cmd("EVAL")
                        .arg(PRUNE)
                        .arg(1)
                        .arg(index)
                        .arg(cursor)
                        .arg(value_prefix)
                        .arg(BATCH_SIZE),
                )
                .await
                .map_err(backend)?;
            if cursor == 0 {
                return Ok(());
            }
        }
    }

    pub(super) async fn flush(&self, name: &str) -> CacheResult<String> {
        if self.connection.is_cluster() {
            return self.cluster_flush(name).await;
        }
        let tag = key_digest(name);
        let pending = format!("{}pending:{tag}", self.prefix);
        let index = format!("{}index:{tag}:", self.prefix);
        let next = generation();
        let retired: Vec<String> = self
            .connection
            .query(
                ::redis::cmd("EVAL")
                    .arg(RETIRE)
                    .arg(2)
                    .arg(self.tag_key(name))
                    .arg(&pending)
                    .arg(&index)
                    .arg(&next),
            )
            .await
            .map_err(backend)?;
        let value_prefix = format!("{}value:", self.prefix);
        for generation in retired {
            if uuid::Uuid::parse_str(&generation).is_err() {
                return Err(CacheError::InvalidValue);
            }
            let mut command = ::redis::cmd("EVAL");
            command
                .arg(DRAIN)
                .arg(2)
                .arg(&pending)
                .arg(format!("{index}{generation}"))
                .arg(&generation)
                .arg(&value_prefix)
                .arg(BATCH_SIZE);
            while self
                .connection
                .query::<bool>(&command)
                .await
                .map_err(backend)?
            {}
        }
        Ok(format!("{tag}:{next}"))
    }
}

fn valid_digest(id: &str, length: usize) -> bool {
    id.len() == length
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
