use ::redis::{Cmd, FromRedisValue, ToRedisArgs};

use super::{BATCH_SIZE, RedisTagSet, valid_digest};
use crate::cache::{CacheError, CacheResult, backend, engine::key_digest, tag_set::generation};

const GENERATIONS: &str = include_str!("../scripts/redis_cluster_tag_generations.lua");
const ITEM: &str = concat!(
    include_str!("../scripts/redis_counter.lua"),
    include_str!("../scripts/redis_tagged_item_operations.lua"),
    include_str!("../scripts/redis_cluster_tag_item.lua"),
);
const RETIRE: &str = include_str!("../scripts/redis_cluster_tag_retire.lua");
const CLEANUP: &str = include_str!("../scripts/redis_cluster_tag_cleanup.lua");
const PRUNE_RETIREMENTS: &str = include_str!("../scripts/redis_cluster_tag_prune_retirements.lua");
const GENERATION_ATTEMPTS: usize = 16;

impl RedisTagSet {
    async fn generations(&self, metadata: &[String]) -> CacheResult<Vec<String>> {
        if metadata.is_empty() {
            return Ok(Vec::new());
        }
        self.query(
            cluster_script(GENERATIONS, metadata.len())
                .arg(metadata)
                .arg(generation()),
        )
        .await
    }

    fn metadata(&self) -> Vec<String> {
        self.tags
            .iter()
            .map(|tag| format!("{}generation:{tag}", self.prefix))
            .collect()
    }

    fn namespace_for(&self, generations: &[String]) -> String {
        self.tags
            .iter()
            .zip(generations)
            .map(|(tag, id)| format!("{tag}:{id}"))
            .collect::<Vec<_>>()
            .join("|")
    }

    fn value_key(&self, namespace: &str, key: &str) -> String {
        let digest = key_digest(&format!("{}:{namespace}{key}", namespace.len()));
        format!("{}value:{digest}", self.prefix)
    }

    pub(super) fn registry(&self) -> String {
        format!("{}indexes", self.prefix)
    }

    pub(super) async fn cluster_namespace(&self) -> CacheResult<String> {
        let generations = self.generations(&self.metadata()).await?;
        Ok(self.namespace_for(&generations))
    }

    pub(super) async fn cluster_item_key(&self, key: &str) -> CacheResult<String> {
        Ok(self.value_key(&self.cluster_namespace().await?, key))
    }

    pub(super) async fn cluster_item<T: FromRedisValue>(
        &self,
        operation: &str,
        key: &str,
        input: impl ToRedisArgs + Send,
        option: impl ToRedisArgs + Send,
    ) -> CacheResult<T> {
        let metadata = self.metadata();
        let input = input.to_redis_args();
        let option = option.to_redis_args();
        let registry = self.registry();
        for _ in 0..GENERATION_ATTEMPTS {
            let generations = self.generations(&metadata).await?;
            let item = self.value_key(&self.namespace_for(&generations), key);
            let mut command = cluster_script(ITEM, 2 + metadata.len() * 2);
            command.arg(item).arg(&registry).arg(&metadata);
            for (tag, id) in self.tags.iter().zip(&generations) {
                command.arg(format!("{}index:{tag}:{id}", self.prefix));
            }
            command
                .arg(operation)
                .arg(&input)
                .arg(&option)
                .arg(&generations);
            if let Some((result,)) = self.query::<Option<(T,)>>(&command).await? {
                return Ok(result);
            }
        }
        Err(CacheError::InvalidValue)
    }

    pub(super) async fn cluster_flush(&self, name: &str) -> CacheResult<String> {
        let metadata = self.tag_key(name);
        let tag = key_digest(name);
        let index_prefix = format!("{}index:{tag}:", self.prefix);
        let pending = format!("{}pending:{tag}", self.prefix);
        let next = generation();
        let registry = self.registry();
        for _ in 0..GENERATION_ATTEMPTS {
            let previous: Option<String> = self.query(::redis::cmd("GET").arg(&metadata)).await?;
            let index = match &previous {
                Some(id) if valid_digest(id, 32) => format!("{index_prefix}{id}"),
                Some(_) => return Err(CacheError::InvalidValue),
                // Redis checks declared keys before running the script, even if absent.
                None => format!("{index_prefix}{next}"),
            };
            let rotated: Option<u64> = self
                .query(
                    cluster_script(RETIRE, 3)
                        .arg(&metadata)
                        .arg(&pending)
                        .arg(&index)
                        .arg(previous.unwrap_or_default())
                        .arg(&next),
                )
                .await?;
            if rotated.is_none() {
                continue;
            }
            self.cleanup_pending_generations(&registry, &index_prefix, &pending, true)
                .await?;
            return Ok(format!("{tag}:{next}"));
        }
        Err(CacheError::InvalidValue)
    }

    pub(super) async fn cluster_prune(&self) -> CacheResult<()> {
        let registry = self.registry();
        let index_prefix = format!("{}index:", self.prefix);
        self.recover_cluster_indexes(&registry, &index_prefix)
            .await?;
        let mut cursor = 0_u64;
        loop {
            let (next, indexes): (u64, Vec<Vec<u8>>) = self
                .query(
                    ::redis::cmd("SSCAN")
                        .arg(&registry)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(BATCH_SIZE),
                )
                .await?;
            let mut invalid = Vec::new();
            for member in &indexes {
                let Some((index, tag, id)) = cluster_index(member, &index_prefix) else {
                    invalid.push(member.as_slice());
                    continue;
                };
                let pending = format!("{}pending:{tag}", self.prefix);
                self.prune_cluster_index(&registry, index, &pending, id)
                    .await?;
            }
            for batch in invalid.chunks(BATCH_SIZE) {
                self.query::<u64>(::redis::cmd("SREM").arg(&registry).arg(batch))
                    .await?;
            }
            if next == 0 {
                return self.prune_empty_retirements(&registry, &index_prefix).await;
            }
            cursor = next;
        }
    }

    pub(super) async fn cluster_prune_selected(&self) -> CacheResult<()> {
        let registry = self.registry();
        let index_prefix = format!("{}index:", self.prefix);
        for tag in &self.tags {
            let pattern = self.key_pattern(&format!("index:{tag}:*"));
            let pending = format!("{}pending:{tag}", self.prefix);
            let mut cursor = 0;
            loop {
                let (next, keys) = self
                    .connection
                    .scan_slot_keys(&registry, &pattern, cursor, BATCH_SIZE)
                    .await
                    .map_err(backend)?;
                for key in &keys {
                    if let Some((index, _, id)) = cluster_index(key, &index_prefix) {
                        self.prune_cluster_index(&registry, index, &pending, id)
                            .await?;
                    }
                }
                if next == 0 {
                    break;
                }
                cursor = next;
            }
        }
        Ok(())
    }

    async fn recover_cluster_indexes(&self, registry: &str, index_prefix: &str) -> CacheResult<()> {
        let pattern = self.key_pattern("index:*");
        let mut cursor = 0;
        loop {
            let (next, keys) = self
                .connection
                .scan_slot_keys(registry, &pattern, cursor, BATCH_SIZE)
                .await
                .map_err(backend)?;
            let indexes: Vec<_> = keys
                .iter()
                .filter_map(|key| cluster_index(key, index_prefix).map(|(index, _, _)| index))
                .collect();
            for batch in indexes.chunks(BATCH_SIZE) {
                self.query::<u64>(::redis::cmd("SADD").arg(registry).arg(batch))
                    .await?;
            }
            if next == 0 {
                return Ok(());
            }
            cursor = next;
        }
    }

    async fn prune_empty_retirements(&self, registry: &str, index_prefix: &str) -> CacheResult<()> {
        let pending_prefix = format!("{}pending:", self.prefix);
        let pattern = self.key_pattern("pending:*");
        let mut cursor = 0;
        loop {
            let (next, keys) = self
                .connection
                .scan_slot_keys(registry, &pattern, cursor, BATCH_SIZE)
                .await
                .map_err(backend)?;
            for key in &keys {
                let Ok(pending) = std::str::from_utf8(key) else {
                    continue;
                };
                if let Some(tag) = pending
                    .strip_prefix(&pending_prefix)
                    .filter(|tag| valid_digest(tag, 64))
                {
                    let prefix = format!("{index_prefix}{tag}:");
                    self.cleanup_pending_generations(registry, &prefix, pending, false)
                        .await?;
                }
            }
            if next == 0 {
                return Ok(());
            }
            cursor = next;
        }
    }

    async fn cleanup_pending_generations(
        &self,
        registry: &str,
        index_prefix: &str,
        pending: &str,
        retire: bool,
    ) -> CacheResult<()> {
        let mut cursor = 0_u64;
        loop {
            let (next, generations): (u64, Vec<Vec<u8>>) = self
                .query(
                    ::redis::cmd("SSCAN")
                        .arg(pending)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(BATCH_SIZE),
                )
                .await?;
            let mut invalid = Vec::new();
            let mut valid = Vec::new();
            for generation in &generations {
                let Some(id) = std::str::from_utf8(generation)
                    .ok()
                    .filter(|id| valid_digest(id, 32))
                else {
                    invalid.push(generation.as_slice());
                    continue;
                };
                if retire {
                    let index = format!("{index_prefix}{id}");
                    self.drain_cluster_index(registry, &index, pending, id)
                        .await?;
                } else {
                    valid.push(id);
                }
            }
            for batch in valid.chunks(BATCH_SIZE) {
                let mut command = cluster_script(PRUNE_RETIREMENTS, 2 + batch.len());
                command.arg(registry).arg(pending);
                for id in batch {
                    command.arg(format!("{index_prefix}{id}"));
                }
                self.query::<()>(command.arg(batch)).await?;
            }
            for batch in invalid.chunks(BATCH_SIZE) {
                self.query::<u64>(::redis::cmd("SREM").arg(pending).arg(batch))
                    .await?;
            }
            if next == 0 {
                return Ok(());
            }
            cursor = next;
        }
    }

    async fn drain_cluster_index(
        &self,
        registry: &str,
        index: &str,
        pending: &str,
        id: &str,
    ) -> CacheResult<()> {
        loop {
            let items: Vec<Vec<u8>> = self
                .query(::redis::cmd("ZRANGE").arg(index).arg(0).arg(BATCH_SIZE - 1))
                .await?;
            if !self
                .cleanup(registry, index, pending, id, &items, true)
                .await?
            {
                return Ok(());
            }
        }
    }

    async fn prune_cluster_index(
        &self,
        registry: &str,
        index: &str,
        pending: &str,
        id: &str,
    ) -> CacheResult<()> {
        let mut cursor = 0_u64;
        loop {
            let (next, entries): (u64, Vec<(Vec<u8>, f64)>) = self
                .query(
                    ::redis::cmd("ZSCAN")
                        .arg(index)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(BATCH_SIZE),
                )
                .await?;
            let items: Vec<_> = entries.into_iter().map(|(key, _)| key).collect();
            let mut exists = true;
            for batch in items.chunks(BATCH_SIZE) {
                exists = self
                    .cleanup(registry, index, pending, id, batch, false)
                    .await?;
            }
            if items.is_empty() {
                exists = self
                    .cleanup(registry, index, pending, id, &[], false)
                    .await?;
            }
            if next == 0 || !exists {
                return Ok(());
            }
            cursor = next;
        }
    }

    async fn cleanup(
        &self,
        registry: &str,
        index: &str,
        pending: &str,
        retirement: &str,
        items: &[Vec<u8>],
        retire: bool,
    ) -> CacheResult<bool> {
        let prefix = format!("{}value:", self.prefix);
        let (values, invalid): (Vec<_>, Vec<_>) =
            items.iter().map(Vec::as_slice).partition(|item| {
                item.strip_prefix(prefix.as_bytes())
                    .and_then(|id| std::str::from_utf8(id).ok())
                    .is_some_and(|id| valid_digest(id, 64))
            });
        self.query(
            cluster_script(CLEANUP, 3 + values.len())
                .arg(registry)
                .arg(index)
                .arg(pending)
                .arg(values)
                .arg(retirement)
                .arg(retire)
                .arg(invalid),
        )
        .await
    }

    async fn query<T: FromRedisValue>(&self, command: &Cmd) -> CacheResult<T> {
        self.connection.query_slot(command).await.map_err(backend)
    }
}

fn cluster_index<'a>(member: &'a [u8], prefix: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let index = std::str::from_utf8(member).ok()?;
    let (tag, id) = index.strip_prefix(prefix)?.split_once(':')?;
    (valid_digest(tag, 64) && valid_digest(id, 32)).then_some((index, tag, id))
}

fn cluster_script(script: &str, key_count: usize) -> Cmd {
    let mut command = ::redis::cmd("EVAL");
    command.arg(script).arg(key_count);
    command
}
