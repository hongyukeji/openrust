use std::{num::NonZeroU64, ops::Deref, sync::Arc};

use crate::contracts::cache::{LockProvider, Store};

use super::{
    CacheError, CacheFuture, CacheResult, RedisStore, Repository, TagSet, Value,
    engine::key_digest, memoized_store::MemoizedStore, redis_tagged_cache::RedisTaggedCache,
};

/// A cache repository scoped to an ordered list of tags.
pub struct TaggedCache {
    pub(super) repository: Repository,
    pub(super) memoized_parent: Option<Arc<MemoizedStore>>,
    pub(super) tags: Arc<TagSet>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl TaggedCache {
    pub fn new(store: Arc<dyn Store>, names: &[&str]) -> Self {
        let tags = Arc::new(TagSet::new(store, names));
        Self {
            repository: Repository::new(Arc::new(TaggedStore(Arc::clone(&tags)))),
            memoized_parent: None,
            tags,
        }
    }

    pub(super) fn redis(store: Arc<RedisStore>, names: &[&str]) -> Self {
        let tags = Arc::new(TagSet::redis(store, names));
        Self {
            repository: Repository::new(Arc::new(RedisTaggedCache::new(Arc::clone(&tags)))),
            memoized_parent: None,
            tags,
        }
    }

    pub fn getTags(&self) -> &TagSet {
        &self.tags
    }

    /// Discards the selected tag group's memoized results without touching the store.
    pub fn flushMemoized(&self) {
        if let Some(parent) = &self.memoized_parent {
            parent.flush_memoized_tags(self.tags.getNames());
        }
    }

    /// Clears the entire store when called through a memoized tagged view.
    pub async fn clear(&self) -> CacheResult<bool> {
        match &self.memoized_parent {
            Some(parent) => parent.flush().await,
            None => {
                self.repository
                    .clear_store(Arc::clone(&self.tags.store))
                    .await
            }
        }
    }

    /// Removes stale references from the selected Redis tags without clearing live values.
    pub async fn flushStale(&self) -> CacheResult<bool> {
        self.tags
            .redis
            .as_ref()
            .ok_or(CacheError::StaleTagPruningUnsupported)?
            .prune_selected()
            .await?;
        Ok(true)
    }

    pub async fn taggedItemKey(&self, key: &str) -> CacheResult<String> {
        if let Some(redis) = &self.tags.redis {
            return redis.item_key(key).await;
        }
        Ok(item_key(&self.tags.getNamespace().await?, key))
    }
}

impl Deref for TaggedCache {
    type Target = Repository;

    fn deref(&self) -> &Repository {
        &self.repository
    }
}

struct TaggedStore(Arc<TagSet>);

fn item_key(namespace: &str, key: &str) -> String {
    format!(
        "tagged:{namespace}:{key}",
        namespace = key_digest(namespace)
    )
}

macro_rules! tagged_operation {
    ($method:ident($($name:ident: $kind:ty),*) -> $output:ty) => {
        fn $method<'a>(
            &'a self,
            key: &'a str,
            $($name: $kind),*
        ) -> CacheFuture<'a, CacheResult<$output>> {
            Box::pin(async move {
                let key = item_key(&self.0.getNamespace().await?, key);
                self.0.store.$method(&key, $($name),*).await
            })
        }
    };
}

impl Store for TaggedStore {
    tagged_operation!(get() -> Option<Value>);
    tagged_operation!(put(value: &'a Value, seconds: NonZeroU64) -> bool);
    tagged_operation!(add(value: &'a Value, seconds: Option<NonZeroU64>) -> bool);
    tagged_operation!(pull() -> Option<Value>);
    tagged_operation!(forever(value: &'a Value) -> bool);
    tagged_operation!(touch(seconds: NonZeroU64) -> bool);
    tagged_operation!(forget() -> bool);
    tagged_operation!(increment(amount: i64) -> i64);
    tagged_operation!(decrement(amount: i64) -> i64);

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(async move {
            let namespace = self.0.getNamespace().await?;
            let keys: Vec<String> = keys.iter().map(|key| item_key(&namespace, key)).collect();
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            self.0.store.many(&keys).await
        })
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let namespace = self.0.getNamespace().await?;
            let values: Vec<_> = values
                .iter()
                .map(|(key, value)| (item_key(&namespace, key), value.clone()))
                .collect();
            self.0.store.put_many(&values, seconds).await
        })
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async move {
            self.0.reset().await?;
            Ok(true)
        })
    }

    fn get_prefix(&self) -> &str {
        self.0.store.get_prefix()
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        self.0.store.lock_provider()
    }
}
