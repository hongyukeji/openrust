use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::Deserialize;

use crate::{
    contracts::{cache::Factory, config::Repository as Config},
    database::DatabaseManager,
    redis::RedisManager,
};

use super::{
    ArrayStore, CacheError, CacheResult, DatabaseStore, FileStore, RedisStore, Repository, backend,
};

/// Resolves named cache stores and repositories.
pub struct CacheManager {
    config: Arc<dyn Config>,
    database: Arc<DatabaseManager>,
    base: PathBuf,
    redis: Arc<RedisManager>,
    stores: Mutex<HashMap<String, Arc<Repository>>>,
}

#[derive(Deserialize)]
#[serde(tag = "driver", rename_all = "snake_case")]
enum Options {
    Array,
    File {
        path: PathBuf,
        lock_path: Option<PathBuf>,
    },
    Redis {
        connection: Option<String>,
        lock_connection: Option<String>,
    },
    Database {
        connection: Option<String>,
        table: Option<String>,
        lock_connection: Option<String>,
        lock_table: Option<String>,
    },
}

#[allow(non_snake_case, reason = "Matches Laravel's cache manager API")]
impl CacheManager {
    pub fn new(config: Arc<dyn Config>, database: Arc<DatabaseManager>, base: PathBuf) -> Self {
        let redis = Arc::new(RedisManager::new(config.clone()));
        Self {
            redis,
            config,
            database,
            base,
            stores: Mutex::default(),
        }
    }

    pub(super) fn with_redis(mut self, redis: Arc<RedisManager>) -> Self {
        self.redis = redis;
        self
    }

    pub fn getDefaultDriver(&self) -> CacheResult<String> {
        match self.config.get("cache.default") {
            None => Ok("database".into()),
            Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(name),
            _ => Err(CacheError::InvalidConfiguration),
        }
    }

    pub fn store(&self, name: Option<&str>) -> CacheResult<Arc<Repository>> {
        let default;
        let name = match name.filter(|name| !name.is_empty()) {
            Some(name) => name,
            None => {
                default = self.getDefaultDriver()?;
                &default
            }
        };
        let mut stores = self
            .stores
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(store) = stores.get(name) {
            return Ok(Arc::clone(store));
        }
        let config = self
            .config
            .get("cache.stores")
            .and_then(|stores| stores.get(name).cloned())
            .ok_or_else(|| CacheError::UnknownStore {
                name: name.to_owned(),
            })?;
        if config
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|driver| !matches!(driver, "array" | "file" | "database" | "redis"))
        {
            return Err(CacheError::UnsupportedDriver);
        }
        let options =
            serde_json::from_value(config).map_err(|_| CacheError::InvalidConfiguration)?;
        let store = match options {
            Options::Array => Repository::new(Arc::new(ArrayStore::new())),
            Options::File { path, lock_path } => {
                if path.as_os_str().is_empty()
                    || lock_path
                        .as_ref()
                        .is_some_and(|path| path.as_os_str().is_empty())
                {
                    return Err(CacheError::InvalidConfiguration);
                }
                let path = self.base.join(path);
                let lock_path =
                    lock_path.map_or_else(|| path.clone(), |lock_path| self.base.join(lock_path));
                Repository::new(Arc::new(FileStore::new(path, lock_path)))
            }
            Options::Redis {
                connection,
                lock_connection,
            } => {
                let connection = connection.as_deref().unwrap_or("default");
                let data = self.redis.connection(Some(connection)).map_err(backend)?;
                let locks = self
                    .redis
                    .connection(lock_connection.as_deref().or(Some(connection)))
                    .map_err(backend)?;
                Repository::new(Arc::new(RedisStore::new(data, locks, self.prefix()?)))
            }
            Options::Database {
                connection,
                table,
                lock_connection,
                lock_table,
            } => {
                let prefix = self.prefix()?;
                let locks = self
                    .database
                    .connection(lock_connection.as_deref().or(connection.as_deref()))
                    .map_err(backend)?;
                let connection = self
                    .database
                    .connection(connection.as_deref())
                    .map_err(backend)?;
                Repository::new(Arc::new(DatabaseStore::new(
                    connection,
                    table.unwrap_or_else(|| "cache".into()),
                    locks,
                    lock_table.unwrap_or_else(|| "cache_locks".into()),
                    prefix,
                )?))
            }
        };
        let store = Arc::new(store);
        stores.insert(name.to_owned(), Arc::clone(&store));
        Ok(store)
    }

    fn prefix(&self) -> CacheResult<String> {
        match self.config.get("cache.prefix") {
            None | Some(serde_json::Value::Null) => Ok(String::new()),
            Some(serde_json::Value::String(prefix)) => Ok(prefix),
            _ => Err(CacheError::InvalidConfiguration),
        }
    }
}

impl Factory for CacheManager {
    fn store(
        &self,
        name: Option<&str>,
    ) -> CacheResult<Arc<dyn crate::contracts::cache::Repository>> {
        let store = self.store(name)?;
        Ok(store)
    }
}
