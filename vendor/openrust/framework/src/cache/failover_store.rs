use std::{
    cell::RefCell,
    collections::HashSet,
    num::NonZeroU64,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use crate::{
    container::Container,
    contracts::{
        cache::{Lock, LockProvider, Repository as _, Store},
        container::Container as _,
        events::Dispatcher as _,
    },
    events::{Dispatcher, EventContext},
};

use super::{
    CacheError, CacheFuture, CacheManager, CacheResult, Repository, TaggedCache, Value,
    cache_manager::CacheManagerState, events::CacheFailedOver,
};

tokio::task_local! {
    static ACTIVE_FAILOVERS: RefCell<HashSet<usize>>;
}

/// Tries configured stores in order until one completes the operation.
pub struct FailoverStore {
    manager: Weak<CacheManagerState>,
    owner: Weak<Container>,
    stores: Vec<String>,
    prefix: String,
    failed: Mutex<HashSet<String>>,
}

impl FailoverStore {
    pub(super) fn new(
        manager: Weak<CacheManagerState>,
        owner: Weak<Container>,
        stores: Vec<String>,
        prefix: String,
    ) -> CacheResult<Self> {
        if stores.is_empty() || stores.iter().any(|name| name.is_empty()) {
            return Err(CacheError::InvalidConfiguration);
        }
        Ok(Self {
            manager,
            owner,
            stores,
            prefix,
            failed: Mutex::default(),
        })
    }

    fn manager(&self) -> CacheResult<CacheManager> {
        Ok(CacheManager::from_state(
            self.manager
                .upgrade()
                .ok_or(CacheError::InvalidConfiguration)?,
        ))
    }

    fn identity(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    fn attempt<'a, T: Send + 'a>(
        &'a self,
        operation: impl Fn(Arc<Repository>) -> CacheFuture<'a, CacheResult<T>> + Send + 'a,
    ) -> CacheFuture<'a, CacheResult<T>> {
        Box::pin(async move {
            // Each nested attempt receives the path taken by its caller, including across awaits.
            let mut active = ACTIVE_FAILOVERS
                .try_with(|active| active.borrow().clone())
                .unwrap_or_default();
            if !active.insert(self.identity()) {
                return Err(CacheError::FailoverCycle);
            }
            ACTIVE_FAILOVERS
                .scope(RefCell::new(active), async move {
                    let manager = self.manager()?;
                    let mut failed = Vec::new();
                    let mut outcome = Err(CacheError::InvalidConfiguration);
                    for name in &self.stores {
                        outcome = match manager.store(Some(name)) {
                            Ok(store) => operation(store).await,
                            Err(error) => Err(error),
                        };
                        match &outcome {
                            Ok(_) => break,
                            Err(error) => failed.push(CacheFailedOver {
                                storeName: name.clone(),
                                exception: error.to_string(),
                            }),
                        }
                    }
                    self.notify(failed).await;
                    outcome
                })
                .await
        })
    }

    async fn notify(&self, mut events: Vec<CacheFailedOver>) {
        {
            let mut previous = self
                .failed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let next = events.iter().map(|event| event.storeName.clone()).collect();
            events.retain(|event| !previous.contains(&event.storeName));
            *previous = next;
        }
        if events.is_empty() {
            return;
        }
        if let Some(dispatcher) = self
            .owner
            .upgrade()
            .and_then(|owner| owner.make::<Dispatcher>().ok())
        {
            for event in events {
                let _ = dispatcher
                    .dispatch(Arc::new(event), &mut EventContext::new())
                    .await;
            }
        }
    }

    fn lock_from(
        &self,
        operation: impl Fn(&dyn LockProvider) -> CacheResult<Box<dyn Lock>>,
    ) -> CacheResult<Box<dyn Lock>> {
        let mut active = ACTIVE_FAILOVERS
            .try_with(|active| active.borrow().clone())
            .unwrap_or_default();
        if !active.insert(self.identity()) {
            return Err(CacheError::FailoverCycle);
        }
        ACTIVE_FAILOVERS.sync_scope(RefCell::new(active), || {
            let manager = self.manager()?;
            let mut last = CacheError::LocksUnsupported;
            for name in &self.stores {
                let result = manager.store(Some(name)).and_then(|store| {
                    let locks = store
                        .get_store()
                        .lock_provider()
                        .ok_or(CacheError::LocksUnsupported)?;
                    operation(locks)
                });
                match result {
                    Ok(lock) => return Ok(lock),
                    Err(error) => last = error,
                }
            }
            Err(last)
        })
    }
}

macro_rules! forward_operation {
    ($name:ident($($argument:ident: $ty:ty),*) -> $output:ty) => {
        fn $name<'a>(&'a self, $($argument: $ty),*) -> CacheFuture<'a, CacheResult<$output>> {
            self.attempt(move |store| Box::pin(async move {
                store.get_store().$name($($argument),*).await
            }))
        }
    };
}

impl Store for FailoverStore {
    fn tags(self: Arc<Self>, names: &[&str]) -> CacheResult<TaggedCache> {
        Ok(TaggedCache::new(self, names))
    }

    fn supports_tags(&self) -> bool {
        true
    }

    forward_operation!(get(key: &'a str) -> Option<Value>);
    forward_operation!(many(keys: &'a [&'a str]) -> Vec<Option<Value>>);
    forward_operation!(put(key: &'a str, value: &'a Value, seconds: NonZeroU64) -> bool);
    forward_operation!(put_many(values: &'a [(String, Value)], seconds: NonZeroU64) -> bool);
    forward_operation!(add(key: &'a str, value: &'a Value, seconds: Option<NonZeroU64>) -> bool);
    forward_operation!(pull(key: &'a str) -> Option<Value>);
    forward_operation!(forever(key: &'a str, value: &'a Value) -> bool);
    forward_operation!(touch(key: &'a str, seconds: NonZeroU64) -> bool);
    forward_operation!(forget(key: &'a str) -> bool);
    forward_operation!(increment(key: &'a str, amount: i64) -> i64);
    forward_operation!(decrement(key: &'a str, amount: i64) -> i64);

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        self.attempt(|store| Box::pin(async move { store.get_store().flush().await }))
    }

    fn get_prefix(&self) -> &str {
        &self.prefix
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        Some(self)
    }
}

impl LockProvider for FailoverStore {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        self.lock_from(|locks| locks.lock(name, lease, owner))
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.lock_from(|locks| locks.restore_lock(name, owner))
    }
}
