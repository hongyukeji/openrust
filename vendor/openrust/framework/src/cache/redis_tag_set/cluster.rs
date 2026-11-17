use ::redis::{Cmd, FromRedisValue, ToRedisArgs};

use super::{BATCH_SIZE, RedisTagSet};
use crate::cache::{CacheError, CacheResult, backend, engine::key_digest, tag_set::generation};

const GENERATIONS: &str = include_str!("../scripts/redis_cluster_tag_generations.lua");
const ITEM: &str = concat!(
    include_str!("../scripts/redis_counter.lua"),
    include_str!("../scripts/redis_tagged_item_operations.lua"),
    include_str!("../scripts/redis_cluster_tag_item.lua"),
);
const RETIRE: &str = include_str!("../scripts/redis_cluster_tag_retire.lua");
const CLEANUP: &str = include_str!("../scripts/redis_cluster_tag_cleanup.lua");
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

    fn registry(&self) -> String {
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
            let retired: Option<Vec<String>> = self
                .query(
                    cluster_script(RETIRE, 3)
                        .arg(&metadata)
                        .arg(&pending)
                        .arg(&index)
                        .arg(previous.unwrap_or_default())
                        .arg(&next),
                )
                .await?;
            let Some(retired) = retired else { continue };
            for id in retired {
                if !valid_digest(&id, 32) {
                    return Err(CacheError::InvalidValue);
                }
                let index = format!("{index_prefix}{id}");
                loop {
                    let items: Vec<String> = self
                        .query(
                            ::redis::cmd("ZRANGE")
                                .arg(&index)
                                .arg(0)
                                .arg(BATCH_SIZE - 1),
                        )
                        .await?;
                    if !self
                        .cleanup(&registry, &index, &pending, &id, &items)
                        .await?
                    {
                        break;
                    }
                }
            }
            return Ok(format!("{tag}:{next}"));
        }
        Err(CacheError::InvalidValue)
    }

    pub(super) async fn cluster_prune(&self) -> CacheResult<()> {
        let registry = self.registry();
        let index_prefix = format!("{}index:", self.prefix);
        let mut cursor = 0_u64;
        loop {
            let (next, indexes): (u64, Vec<String>) = self
                .query(
                    ::redis::cmd("SSCAN")
                        .arg(&registry)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(BATCH_SIZE),
                )
                .await?;
            for index in indexes {
                let Some((tag, id)) = index
                    .strip_prefix(&index_prefix)
                    .and_then(|part| part.split_once(':'))
                else {
                    return Err(CacheError::InvalidValue);
                };
                if !valid_digest(tag, 64) || !valid_digest(id, 32) {
                    return Err(CacheError::InvalidValue);
                }
                self.prune_cluster_index(&registry, &index).await?;
            }
            if next == 0 {
                return Ok(());
            }
            cursor = next;
        }
    }

    async fn prune_cluster_index(&self, registry: &str, index: &str) -> CacheResult<()> {
        let mut cursor = 0_u64;
        loop {
            let (next, entries): (u64, Vec<(String, f64)>) = self
                .query(
                    ::redis::cmd("ZSCAN")
                        .arg(index)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(BATCH_SIZE),
                )
                .await?;
            let items: Vec<String> = entries.into_iter().map(|(key, _)| key).collect();
            let mut exists = true;
            for batch in items.chunks(BATCH_SIZE) {
                exists = self.cleanup(registry, index, registry, "", batch).await?;
            }
            if items.is_empty() {
                exists = self.cleanup(registry, index, registry, "", &[]).await?;
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
        items: &[String],
    ) -> CacheResult<bool> {
        let prefix = format!("{}value:", self.prefix);
        if !items.iter().all(|item| {
            item.strip_prefix(&prefix)
                .is_some_and(|id| valid_digest(id, 64))
        }) {
            return Err(CacheError::InvalidValue);
        }
        self.query(
            cluster_script(CLEANUP, 3 + items.len())
                .arg(registry)
                .arg(index)
                .arg(pending)
                .arg(items)
                .arg(retirement),
        )
        .await
    }

    async fn query<T: FromRedisValue>(&self, command: &Cmd) -> CacheResult<T> {
        self.connection.query_slot(command).await.map_err(backend)
    }
}

fn cluster_script(script: &str, key_count: usize) -> Cmd {
    let mut command = ::redis::cmd("EVAL");
    command.arg(script).arg(key_count);
    command
}

fn valid_digest(id: &str, length: usize) -> bool {
    id.len() == length
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
