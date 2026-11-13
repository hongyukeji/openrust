use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;

use crate::{
    cache::{CacheResult, Repository},
    console::ConsoleFuture,
    contracts::cache::{Lock, LockProvider},
};

use super::{Event, EventMutex};

/// Holds owner-aware overlap locks in the selected cache store.
pub struct CacheEventMutex {
    store: Arc<Repository>,
    locks: Mutex<HashMap<String, Box<dyn Lock>>>,
}

impl CacheEventMutex {
    pub fn new(store: Arc<Repository>) -> Self {
        Self {
            store,
            locks: Mutex::default(),
        }
    }
}

impl CacheEventMutex {
    pub(super) async fn take_owner(&self, event: &Event) -> Option<String> {
        self.locks
            .lock()
            .await
            .remove(&event.mutex_key())
            .map(|lock| lock.owner().to_owned())
    }

    pub(super) async fn release_owner(&self, event: &Event, owner: &str) -> CacheResult<()> {
        self.store
            .restore_lock(&event.mutex_key(), owner)?
            .release()
            .await?;
        Ok(())
    }
}

impl EventMutex for CacheEventMutex {
    fn create<'a>(&'a self, event: &'a Event) -> ConsoleFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let key = event.mutex_key();
            let mut locks = self.locks.lock().await;
            if locks.contains_key(&key) {
                return Ok(false);
            }
            let mut lock = self.store.lock(&key, event.overlap, None)?;
            if !lock.get().await? {
                return Ok(false);
            }
            locks.insert(key, lock);
            Ok(true)
        })
    }

    fn exists<'a>(&'a self, event: &'a Event) -> ConsoleFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let mut lock = self.store.lock(&event.mutex_key(), event.overlap, None)?;
            if !lock.get().await? {
                return Ok(true);
            }
            lock.release().await?;
            Ok(false)
        })
    }

    fn forget<'a>(&'a self, event: &'a Event) -> ConsoleFuture<'a, CacheResult<()>> {
        Box::pin(async move {
            if let Some(mut lock) = self.locks.lock().await.remove(&event.mutex_key()) {
                lock.release().await?;
            }
            Ok(())
        })
    }
}
