use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    container::Container,
    contracts::{config::Repository, hashing::Hasher},
};

use super::{
    Argon2IdHasher, ArgonHasher, BcryptHasher, HashError, HashFuture, HashInfo, HashOptions,
    HashResult, driver::OperationPool,
};

type DriverFactory = dyn Fn(&HashManager) -> HashResult<Arc<dyn Hasher>> + Send + Sync;

#[derive(Default)]
struct DriverState {
    factories: HashMap<String, Arc<DriverFactory>>,
    drivers: HashMap<String, Arc<dyn Hasher>>,
    creating: HashSet<String>,
    cache_generation: Arc<()>,
}

struct DriverCreation<'a> {
    manager: &'a HashManager,
    name: &'a str,
}

impl Drop for DriverCreation<'_> {
    fn drop(&mut self) {
        self.manager.driver_state().creating.remove(self.name);
    }
}

/// Resolves named hash drivers and delegates to the configured default driver.
pub struct HashManager {
    config: Arc<dyn Repository>,
    pool: OperationPool,
    container: Weak<Container>,
    state: Mutex<DriverState>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl HashManager {
    pub fn new(config: Arc<dyn Repository>) -> HashResult<Self> {
        Self::for_container(config, Weak::new())
    }

    pub(crate) fn for_container(
        config: Arc<dyn Repository>,
        container: Weak<Container>,
    ) -> HashResult<Self> {
        let capacity = read(config.as_ref(), "hashing.max_concurrent_operations")?.unwrap_or(2);
        let limits = read(config.as_ref(), "hashing.execution_limits")?.unwrap_or_default();
        Ok(Self {
            config,
            pool: OperationPool::new(capacity, limits)?,
            container,
            state: Mutex::default(),
        })
    }

    pub fn get_default_driver(&self) -> HashResult<String> {
        Ok(read(self.config.as_ref(), "hashing.driver")?.unwrap_or_else(|| "bcrypt".into()))
    }

    /// Registers a creator for future resolutions of this driver name.
    pub fn extend(
        &self,
        name: impl Into<String>,
        factory: impl Fn(&HashManager) -> HashResult<Arc<dyn Hasher>> + Send + Sync + 'static,
    ) -> &Self {
        let name = name.into();
        let previous = self
            .driver_state()
            .factories
            .insert(name, Arc::new(factory));
        drop(previous);
        self
    }

    /// Returns the application container, when this manager belongs to an application.
    pub fn getContainer(&self) -> Option<Arc<Container>> {
        self.container.upgrade()
    }

    pub fn getDrivers(&self) -> HashMap<String, Arc<dyn Hasher>> {
        self.driver_state().drivers.clone()
    }

    pub fn forgetDrivers(&self) -> &Self {
        let previous = {
            let mut state = self.driver_state();
            state.cache_generation = Arc::new(());
            std::mem::take(&mut state.drivers)
        };
        drop(previous);
        self
    }

    pub fn driver(&self, name: Option<&str>) -> HashResult<Arc<dyn Hasher>> {
        let default;
        let name = match name.filter(|name| !matches!(*name, "" | "0")) {
            Some(name) => name,
            None => {
                default = self.get_default_driver()?;
                &default
            }
        };
        let (factory, generation) = {
            let mut state = self.driver_state();
            if let Some(driver) = state.drivers.get(name) {
                return Ok(Arc::clone(driver));
            }
            let Some(factory) = state.factories.get(name).cloned() else {
                let driver: Arc<dyn Hasher> = match name {
                    "bcrypt" => Arc::new(self.createBcryptDriver()?),
                    "argon" => Arc::new(self.createArgonDriver()?),
                    "argon2id" => Arc::new(self.createArgon2idDriver()?),
                    _ => return Err(HashError::UnknownDriver),
                };
                state.drivers.insert(name.to_owned(), Arc::clone(&driver));
                return Ok(driver);
            };
            if !state.creating.insert(name.to_owned()) {
                return Err(HashError::DriverBusy);
            }
            (factory, Arc::clone(&state.cache_generation))
        };
        let _creation = DriverCreation {
            manager: self,
            name,
        };
        let candidate = factory(self)?;
        let driver = match candidate.as_hash_manager() {
            Some(manager) if std::ptr::eq(manager, self) => {
                return Err(HashError::DriverSelfReference);
            }
            Some(manager) => manager.driver(None)?,
            None => candidate,
        };
        let mut state = self.driver_state();
        if Arc::ptr_eq(&state.cache_generation, &generation) {
            state.drivers.insert(name.to_owned(), Arc::clone(&driver));
        }
        Ok(driver)
    }

    pub fn createBcryptDriver(&self) -> HashResult<BcryptHasher> {
        Ok(BcryptHasher::with_pool(
            read(self.config.as_ref(), "hashing.bcrypt")?.unwrap_or_default(),
            self.pool.clone(),
        ))
    }

    pub fn createArgonDriver(&self) -> HashResult<ArgonHasher> {
        Ok(ArgonHasher::with_pool(
            read(self.config.as_ref(), "hashing.argon")?.unwrap_or_default(),
            self.pool.clone(),
        ))
    }

    pub fn createArgon2idDriver(&self) -> HashResult<Argon2IdHasher> {
        Ok(Argon2IdHasher::with_pool(
            read(self.config.as_ref(), "hashing.argon")?.unwrap_or_default(),
            self.pool.clone(),
        ))
    }

    fn driver_state(&self) -> MutexGuard<'_, DriverState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn isHashed(&self, value: &str) -> HashResult<bool> {
        Ok(self.info(value)?.algo.is_some())
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Hasher for HashManager {
    fn as_hash_manager(&self) -> Option<&HashManager> {
        Some(self)
    }

    fn info(&self, hashed: &str) -> HashResult<HashInfo> {
        self.driver(None)?.info(hashed)
    }

    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<String>> {
        Box::pin(async move { self.driver(None)?.make(value, options).await })
    }

    fn check<'a>(
        &'a self,
        value: &'a [u8],
        hashed: &'a str,
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<bool>> {
        Box::pin(async move { self.driver(None)?.check(value, hashed, options).await })
    }

    fn needsRehash(&self, hashed: &str, options: &HashOptions) -> HashResult<bool> {
        self.driver(None)?.needsRehash(hashed, options)
    }

    fn verifyConfiguration(&self, hashed: &str) -> HashResult<bool> {
        self.driver(None)?.verifyConfiguration(hashed)
    }
}

fn read<T: DeserializeOwned>(config: &dyn Repository, key: &str) -> HashResult<Option<T>> {
    match config.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(|_| HashError::InvalidOptions),
    }
}
