use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock, Weak},
};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    container::Container,
    contracts::{
        cache::{Factory, Store},
        config::Repository as Config,
        container::Container as _,
        foundation::Application as _,
    },
    database::DatabaseManager,
    filesystem::FilesystemManager,
    redis::RedisManager,
};

use super::{
    ArrayStore, CacheError, CacheResult, DatabaseStore, DynamoDbStore, FailoverStore, FileStore,
    MemcachedStore, NullStore, RedisStore, Repository, SessionStore, StorageStore, backend,
    dynamo_db_store, memcached_connector as memcached,
    memoized_store::{MemoizedRepositories, MemoizedStore},
    store_registry::StoreRegistry,
};

type DriverFactory = dyn Fn(&CacheManager, &Value) -> CacheResult<Repository> + Send + Sync;

/// Resolves named cache stores and repositories.
pub struct CacheManager {
    inner: Arc<CacheManagerState>,
    selection: Option<Option<CacheApplication>>,
}

pub(super) struct CacheManagerState {
    config: Arc<dyn Config>,
    database: Arc<DatabaseManager>,
    base: PathBuf,
    redis: Arc<RedisManager>,
    stores: StoreRegistry,
    filesystem: Arc<FilesystemManager>,
    memcached: memcached::Connector,
    factories: Mutex<HashMap<String, Arc<DriverFactory>>>,
    owner: Weak<Container>,
    application: RwLock<Option<CacheApplication>>,
    macros: Arc<super::macro_support::CacheMacros>,
}

#[derive(Clone)]
struct CacheApplication {
    owner: Weak<Container>,
    base: PathBuf,
}

#[derive(Deserialize)]
#[serde(tag = "driver", rename_all = "snake_case")]
enum Options {
    Array,
    Null,
    Failover {
        stores: Vec<String>,
    },
    Session {
        key: Option<String>,
    },
    Memcached {
        persistent_id: Option<String>,
        sasl: Option<memcached::Credentials>,
        options: Option<memcached::Options>,
        servers: Vec<memcached::Server>,
    },
    Dynamodb(dynamo_db_store::Options),
    Storage {
        disk: Option<String>,
        path: Option<String>,
    },
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

impl CacheManager {
    pub fn new(config: Arc<dyn Config>, database: Arc<DatabaseManager>, base: PathBuf) -> Self {
        let redis = Arc::new(RedisManager::new(config.clone()));
        let filesystem = Arc::new(FilesystemManager::new(config.clone(), base.clone()));
        Self {
            inner: Arc::new(CacheManagerState {
                redis,
                filesystem,
                config,
                database,
                base,
                stores: StoreRegistry::default(),
                factories: Mutex::default(),
                memcached: memcached::Connector::default(),
                owner: Weak::new(),
                application: RwLock::new(None),
                macros: Arc::default(),
            }),
            selection: None,
        }
    }

    pub(super) fn with_redis(mut self, redis: Arc<RedisManager>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("unshared cache manager")
            .redis = redis;
        self
    }

    pub(super) fn with_filesystem(mut self, filesystem: Arc<FilesystemManager>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("unshared cache manager")
            .filesystem = filesystem;
        self
    }

    pub(super) fn with_owner(mut self, owner: Weak<Container>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("unshared cache manager")
            .owner = owner;
        self
    }

    pub(super) fn from_state(inner: Arc<CacheManagerState>) -> Self {
        Self {
            inner,
            selection: None,
        }
    }

    pub(super) fn with_macros(mut self, macros: Arc<super::macro_support::CacheMacros>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("unshared cache manager")
            .macros = macros;
        self
    }

    fn application(&self) -> Option<CacheApplication> {
        self.selection.clone().unwrap_or_else(|| {
            self.inner
                .application
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    fn snapshot(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            selection: Some(self.application()),
        }
    }

    fn owner(&self) -> Weak<Container> {
        self.application()
            .map_or_else(|| self.inner.owner.clone(), |application| application.owner)
    }

    fn base_path(&self) -> PathBuf {
        self.application()
            .map_or_else(|| self.inner.base.clone(), |application| application.base)
    }

    fn config(&self) -> CacheResult<Arc<dyn Config>> {
        match self.application() {
            None => Ok(self.inner.config.clone()),
            Some(application) => Ok(application
                .owner
                .upgrade()
                .ok_or(CacheError::Unavailable(
                    crate::contracts::container::BindingResolutionError::FacadeNotSet,
                ))?
                .get::<dyn Config>("config")?),
        }
    }

    fn database(&self) -> CacheResult<Arc<DatabaseManager>> {
        match self.application() {
            None => Ok(self.inner.database.clone()),
            Some(application) => Ok(application
                .owner
                .upgrade()
                .ok_or(CacheError::Unavailable(
                    crate::contracts::container::BindingResolutionError::FacadeNotSet,
                ))?
                .get::<DatabaseManager>("db")?),
        }
    }

    fn redis(&self) -> CacheResult<Arc<RedisManager>> {
        match self.application() {
            None => Ok(self.inner.redis.clone()),
            Some(application) => Ok(application
                .owner
                .upgrade()
                .ok_or(CacheError::Unavailable(
                    crate::contracts::container::BindingResolutionError::FacadeNotSet,
                ))?
                .get::<RedisManager>("redis")?),
        }
    }

    fn filesystem(&self) -> CacheResult<Arc<FilesystemManager>> {
        match self.application() {
            None => Ok(self.inner.filesystem.clone()),
            Some(application) => Ok(application
                .owner
                .upgrade()
                .ok_or(CacheError::Unavailable(
                    crate::contracts::container::BindingResolutionError::FacadeNotSet,
                ))?
                .get::<FilesystemManager>("filesystem")?),
        }
    }

    fn macro_registry(&self) -> CacheResult<Arc<super::macro_support::CacheMacros>> {
        match self.application() {
            None => Ok(self.inner.macros.clone()),
            Some(application) => Ok(application
                .owner
                .upgrade()
                .ok_or(CacheError::Unavailable(
                    crate::contracts::container::BindingResolutionError::FacadeNotSet,
                ))?
                .make::<super::macro_support::CacheMacros>()?),
        }
    }

    /// 只更换后续解析使用的应用，不提前访问服务或删除已解析仓储。
    pub fn set_application(&self, application: &crate::foundation::Application) -> &Self {
        let next = CacheApplication {
            owner: Arc::downgrade(&application.container_handle()),
            base: application.base_path().to_path_buf(),
        };
        let retired = self
            .inner
            .application
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(next);
        drop(retired);
        self
    }

    /// Memoizes reads of the selected store within the current application lifecycle.
    pub fn memo(&self, name: Option<&str>) -> CacheResult<Arc<Repository>> {
        self.snapshot().memo_selected(name)
    }

    fn memo_selected(&self, name: Option<&str>) -> CacheResult<Arc<Repository>> {
        let repository = self.store(name)?;
        if let Some(owner) = self
            .get_container()
            .filter(|owner| owner.has_active_scope())
        {
            let repositories = owner.make::<MemoizedRepositories>()?;
            return Ok(repositories.repository(repository));
        }
        Ok(MemoizedStore::wrap(repository, None))
    }

    pub fn get_default_driver(&self) -> CacheResult<String> {
        match self.config()?.get("cache.default") {
            None | Some(Value::Null) => Ok("null".into()),
            Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(name),
            _ => Err(CacheError::InvalidConfiguration),
        }
    }

    /// Registers or replaces a driver creator for future store resolutions.
    pub fn extend(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(&CacheManager, &Value) -> CacheResult<Repository> + Send + Sync + 'static,
    ) -> CacheResult<&Self> {
        let driver = driver.into();
        if driver.is_empty() {
            return Err(CacheError::InvalidConfiguration);
        }
        let previous = self
            .inner
            .factories
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(driver, Arc::new(factory));
        drop(previous);
        Ok(self)
    }

    /// Returns the application container when the manager belongs to an application.
    pub fn get_container(&self) -> Option<Arc<Container>> {
        self.owner().upgrade()
    }

    pub fn set_default_driver(&self, name: &str) -> CacheResult<()> {
        if name.is_empty() {
            return Err(CacheError::InvalidConfiguration);
        }
        self.config()?
            .set("cache.default", name.into())
            .map_err(backend)
    }

    pub fn store(&self, name: Option<&str>) -> CacheResult<Arc<Repository>> {
        let manager = self.snapshot();
        let name = manager.store_name(name)?;
        self.inner
            .stores
            .get_or_resolve(&name, || manager.resolve(&name))
    }

    pub fn driver(&self, name: Option<&str>) -> CacheResult<Arc<Repository>> {
        self.store(name)
    }

    /// Removes a cached repository so the next access resolves the store again.
    pub fn forget_driver(&self, name: Option<&str>) -> CacheResult<&Self> {
        self.inner.stores.forget(&self.store_name(name)?);
        Ok(self)
    }

    pub fn purge(&self, name: Option<&str>) -> CacheResult<()> {
        self.forget_driver(name)?;
        Ok(())
    }

    fn store_name(&self, name: Option<&str>) -> CacheResult<String> {
        match name.filter(|name| !name.is_empty()) {
            Some(name) => Ok(name.to_owned()),
            None => self.get_default_driver(),
        }
    }

    /// Creates a fresh repository from a named store's configuration.
    pub fn resolve(&self, name: &str) -> CacheResult<Repository> {
        let manager = self.snapshot();
        manager.build(manager.store_config(name)?)
    }

    fn store_config(&self, name: &str) -> CacheResult<Value> {
        if name == "null" {
            return Ok(serde_json::json!({ "driver": "null", "store": "null" }));
        }
        let mut config = self
            .config()?
            .get("cache.stores")
            .and_then(|stores| stores.get(name).cloned())
            .ok_or_else(|| CacheError::UnknownStore {
                name: name.to_owned(),
            })?;
        let options = config
            .as_object_mut()
            .ok_or(CacheError::InvalidConfiguration)?;
        options.entry("store").or_insert_with(|| name.into());
        Ok(config)
    }

    pub(crate) fn redis_session_store(
        &self,
        name: &str,
        connection: Option<&str>,
        prefix: Option<&str>,
    ) -> CacheResult<Repository> {
        let manager = self.snapshot();
        let mut config = manager.store_config(name)?;
        if config.get("driver").and_then(Value::as_str) != Some("redis") {
            return Err(CacheError::InvalidConfiguration);
        }
        config["connection"] = connection.into();
        if let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) {
            config["prefix"] = prefix.into();
        }
        manager.build(config)
    }

    /// Builds an on-demand repository without registering it as a named store.
    pub fn build(&self, config: Value) -> CacheResult<Repository> {
        self.snapshot().build_selected(config)
    }

    fn build_selected(&self, mut config: Value) -> CacheResult<Repository> {
        let params = config
            .as_object_mut()
            .ok_or(CacheError::InvalidConfiguration)?;
        if !params.contains_key("store") {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("ondemand");
            params.insert("store".into(), name.to_owned().into());
        }
        let driver = config
            .get("driver")
            .and_then(Value::as_str)
            .ok_or(CacheError::InvalidConfiguration)?;
        let factory = self
            .inner
            .factories
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(driver)
            .cloned();
        if let Some(factory) = factory {
            let repository = factory(self, &config)?;
            return Ok(repository.with_macros(self.macro_registry()?));
        }
        if !matches!(
            driver,
            "array"
                | "file"
                | "database"
                | "redis"
                | "failover"
                | "null"
                | "storage"
                | "session"
                | "memcached"
                | "dynamodb"
        ) {
            return Err(CacheError::UnsupportedDriver);
        }
        let options =
            Options::deserialize(&config).map_err(|_| CacheError::InvalidConfiguration)?;
        let store: Arc<dyn Store> = match options {
            Options::Null => return self.repository(Arc::new(NullStore), &Value::Null),
            Options::Array => Arc::new(ArrayStore::new()),
            Options::Failover { stores } => Arc::new(FailoverStore::new(
                Arc::downgrade(&self.inner),
                self.owner(),
                stores,
                self.prefix(&config)?,
            )?),
            Options::Session { key } => Arc::new(SessionStore::new(
                self.owner(),
                key.unwrap_or_else(|| "_cache".into()),
            )?),
            Options::Memcached {
                persistent_id,
                sasl,
                options,
                servers,
            } => {
                let settings =
                    memcached::Settings::new(servers, sasl, options.unwrap_or_default())?;
                let client = self.inner.memcached.connect(persistent_id, settings)?;
                Arc::new(MemcachedStore::new(client, self.prefix(&config)?))
            }
            Options::Dynamodb(options) => {
                Arc::new(DynamoDbStore::configure(options, self.prefix(&config)?)?)
            }
            Options::Storage { disk, path } => {
                let disk = self.filesystem()?.disk(disk.as_deref()).map_err(backend)?;
                Arc::new(StorageStore::new(
                    disk,
                    path.as_deref().unwrap_or_default(),
                    self.prefix(&config)?,
                )?)
            }
            Options::File { path, lock_path } => {
                if path.as_os_str().is_empty()
                    || lock_path
                        .as_ref()
                        .is_some_and(|path| path.as_os_str().is_empty())
                {
                    return Err(CacheError::InvalidConfiguration);
                }
                let path = self.base_path().join(path);
                let lock_path = lock_path.map_or_else(
                    || path.clone(),
                    |lock_path| self.base_path().join(lock_path),
                );
                Arc::new(FileStore::new(path, lock_path))
            }
            Options::Redis {
                connection,
                lock_connection,
            } => {
                let connection = connection.as_deref().unwrap_or("default");
                let data = self
                    .redis()?
                    .connection(Some(connection))
                    .map_err(backend)?;
                let locks = self
                    .redis()?
                    .connection(lock_connection.as_deref().or(Some(connection)))
                    .map_err(backend)?;
                Arc::new(RedisStore::new(data, locks, self.prefix(&config)?))
            }
            Options::Database {
                connection,
                table,
                lock_connection,
                lock_table,
            } => {
                let locks = self
                    .database()?
                    .connection(lock_connection.as_deref().or(connection.as_deref()))
                    .map_err(backend)?;
                let connection = self
                    .database()?
                    .connection(connection.as_deref())
                    .map_err(backend)?;
                Arc::new(DatabaseStore::new(
                    connection,
                    table.unwrap_or_else(|| "cache".into()),
                    locks,
                    lock_table.unwrap_or_else(|| "cache_locks".into()),
                    self.prefix(&config)?,
                )?)
            }
        };
        self.repository(store, &config)
    }

    /// Wraps a store implementation using the application's repository event settings.
    pub fn repository(&self, store: Arc<dyn Store>, config: &Value) -> CacheResult<Repository> {
        self.snapshot().repository_selected(store, config)
    }

    fn repository_selected(
        &self,
        store: Arc<dyn Store>,
        config: &Value,
    ) -> CacheResult<Repository> {
        let events = match config.get("events") {
            None => config.get("driver").and_then(Value::as_str) != Some("failover"),
            Some(Value::Null) => true,
            Some(value) => value.as_bool().ok_or(CacheError::InvalidConfiguration)?,
        };
        let name = config.get("store").and_then(Value::as_str);
        let repository = Repository::new(store)
            .with_name(name)
            .with_macros(self.macro_registry()?);
        if events && let Some(dispatcher) = self.event_dispatcher()? {
            repository.set_event_dispatcher(dispatcher);
        }
        Ok(repository)
    }

    fn event_dispatcher(
        &self,
    ) -> CacheResult<Option<Arc<dyn crate::contracts::events::Dispatcher>>> {
        let Some(owner) = self.get_container() else {
            return Ok(None);
        };
        if owner.bound::<dyn crate::contracts::events::Dispatcher>() {
            return Ok(Some(
                owner.make::<dyn crate::contracts::events::Dispatcher>()?,
            ));
        }
        if owner.bound::<crate::events::Dispatcher>() {
            return Ok(Some(owner.make::<crate::events::Dispatcher>()?));
        }
        Ok(None)
    }

    pub fn refresh_event_dispatcher(&self) -> CacheResult<()> {
        if let Some(dispatcher) = self.event_dispatcher()? {
            for repository in self.inner.stores.resolved() {
                repository.set_event_dispatcher(dispatcher.clone());
            }
        }
        Ok(())
    }

    fn prefix(&self, config: &Value) -> CacheResult<String> {
        if let Some(prefix) = config.get("prefix").filter(|prefix| !prefix.is_null()) {
            return prefix
                .as_str()
                .map(str::to_owned)
                .ok_or(CacheError::InvalidConfiguration);
        }
        match self.config()?.get("cache.prefix") {
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
