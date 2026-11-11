use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::contracts::{config::Repository, hashing::Hasher};

use super::{
    Argon2IdHasher, ArgonHasher, BcryptHasher, HashError, HashFuture, HashInfo, HashOptions,
    HashResult, driver::OperationPool,
};

/// Resolves named hash drivers and delegates to the configured default driver.
pub struct HashManager {
    config: Arc<dyn Repository>,
    pool: OperationPool,
    drivers: Mutex<HashMap<String, Arc<dyn Hasher>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's hash manager API")]
impl HashManager {
    pub fn new(config: Arc<dyn Repository>) -> HashResult<Self> {
        let capacity = read(config.as_ref(), "hashing.max_concurrent_operations")?.unwrap_or(2);
        let limits = read(config.as_ref(), "hashing.execution_limits")?.unwrap_or_default();
        Ok(Self {
            config,
            pool: OperationPool::new(capacity, limits)?,
            drivers: Mutex::default(),
        })
    }

    pub fn getDefaultDriver(&self) -> HashResult<String> {
        Ok(read(self.config.as_ref(), "hashing.driver")?.unwrap_or_else(|| "bcrypt".into()))
    }

    pub fn driver(&self, name: Option<&str>) -> HashResult<Arc<dyn Hasher>> {
        let default;
        let name = match name {
            Some(name) => name,
            None => {
                default = self.getDefaultDriver()?;
                &default
            }
        };
        let mut drivers = self
            .drivers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(driver) = drivers.get(name) {
            return Ok(Arc::clone(driver));
        }
        let driver: Arc<dyn Hasher> = match name {
            "bcrypt" => Arc::new(BcryptHasher::with_pool(
                read(self.config.as_ref(), "hashing.bcrypt")?.unwrap_or_default(),
                self.pool.clone(),
            )),
            "argon" => Arc::new(ArgonHasher::with_pool(
                read(self.config.as_ref(), "hashing.argon")?.unwrap_or_default(),
                self.pool.clone(),
            )),
            "argon2id" => Arc::new(Argon2IdHasher::with_pool(
                read(self.config.as_ref(), "hashing.argon")?.unwrap_or_default(),
                self.pool.clone(),
            )),
            _ => return Err(HashError::UnknownDriver),
        };
        drivers.insert(name.to_owned(), Arc::clone(&driver));
        Ok(driver)
    }

    pub fn isHashed(&self, value: &str) -> HashResult<bool> {
        Ok(self.info(value)?.algo.is_some())
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's hash manager API")]
impl Hasher for HashManager {
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
