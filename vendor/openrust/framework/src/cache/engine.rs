use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{
    CacheError, CacheResult, Value, backend, database_store::DatabaseBackend,
    file_store::FileBackend,
};

pub(super) const FOREVER: i64 = i64::MAX;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Record {
    pub value: Value,
    pub expires: i64,
}

pub(super) enum Change {
    Keep,
    Write(Record),
    Remove,
}

#[derive(Clone)]
pub(super) enum Engine {
    Array(Arc<Mutex<HashMap<String, Record>>>),
    File(Arc<FileBackend>),
    Database(Arc<DatabaseBackend>),
}

impl Engine {
    pub(super) fn array() -> Self {
        Self::Array(Arc::default())
    }

    pub(super) async fn read(&self, key: String) -> CacheResult<Option<Record>> {
        match self {
            Self::Database(db) => db.read(&key).await,
            _ => self.change(key, |entry| Ok((Change::Keep, entry))).await,
        }
    }

    pub(super) async fn change<T: Send + 'static>(
        &self,
        key: String,
        edit: impl Fn(Option<Record>) -> CacheResult<(Change, T)> + Send + Sync + 'static,
    ) -> CacheResult<T> {
        match self {
            Self::Array(items) => {
                let mut items = items
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let now = now()?;
                if items.get(&key).is_some_and(|entry| entry.expires <= now) {
                    items.remove(&key);
                }
                let (change, result) = edit(items.get(&key).cloned())?;
                match change {
                    Change::Keep => {}
                    Change::Write(record) => {
                        items.insert(key, record);
                    }
                    Change::Remove => {
                        items.remove(&key);
                    }
                }
                Ok(result)
            }
            Self::File(store) => {
                let store = Arc::clone(store);
                tokio::task::spawn_blocking(move || store.change(&key, edit))
                    .await
                    .map_err(backend)?
            }
            Self::Database(store) => store.change(&key, edit).await,
        }
    }

    pub(super) async fn flush(&self) -> CacheResult<()> {
        match self {
            Self::Array(items) => {
                items
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clear();
                Ok(())
            }
            Self::File(store) => {
                let store = Arc::clone(store);
                tokio::task::spawn_blocking(move || store.flush())
                    .await
                    .map_err(backend)?
            }
            Self::Database(store) => store.flush().await,
        }
    }
}

pub(super) fn now() -> CacheResult<i64> {
    epoch_millis(SystemTime::now())
}

pub(super) fn epoch_millis(time: SystemTime) -> CacheResult<i64> {
    i64::try_from(
        time.duration_since(UNIX_EPOCH)
            .map_err(|_| CacheError::InvalidValue)?
            .as_millis(),
    )
    .map_err(|_| CacheError::InvalidValue)
}

pub(super) fn expires(lease: Option<Duration>) -> CacheResult<i64> {
    match lease {
        Some(duration) if !duration.is_zero() => {
            let millis =
                i64::try_from(duration.as_millis()).map_err(|_| CacheError::InvalidValue)?;
            now()?
                .checked_add(millis.max(1))
                .ok_or(CacheError::InvalidValue)
        }
        _ => Ok(FOREVER),
    }
}

pub(super) fn key_digest(key: &str) -> String {
    let mut result = String::with_capacity(64);
    for byte in aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, key.as_bytes()).as_ref() {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
