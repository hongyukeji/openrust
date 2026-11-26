use std::sync::Arc;

use crate::{container::ServiceKey, contracts::hashing::Hasher, support::facades::Facade};

use super::{HashInfo, HashManager, HashOptions, HashResult};

/// Resolves the application hash manager.
pub struct Hash;

impl Facade for Hash {
    type Service = HashManager;

    fn get_facade_accessor() -> ServiceKey {
        "hash".into()
    }
}

impl Hash {
    pub fn extend(
        name: impl Into<String>,
        factory: impl Fn(&HashManager) -> HashResult<Arc<dyn Hasher>> + Send + Sync + 'static,
    ) -> HashResult<()> {
        Self::get_facade_root()?.extend(name, factory);
        Ok(())
    }

    pub fn forget_drivers() -> HashResult<()> {
        Self::get_facade_root()?.forget_drivers();
        Ok(())
    }

    pub fn driver(name: Option<&str>) -> HashResult<Arc<dyn Hasher>> {
        Self::get_facade_root()?.driver(name)
    }

    pub async fn make(value: impl AsRef<[u8]>, options: &HashOptions) -> HashResult<String> {
        Self::driver(None)?.make(value.as_ref(), options).await
    }

    pub async fn check(
        value: impl AsRef<[u8]>,
        hashed: &str,
        options: &HashOptions,
    ) -> HashResult<bool> {
        Self::driver(None)?
            .check(value.as_ref(), hashed, options)
            .await
    }

    pub fn info(hashed: &str) -> HashResult<HashInfo> {
        Self::driver(None)?.info(hashed)
    }

    pub fn needs_rehash(hashed: &str, options: &HashOptions) -> HashResult<bool> {
        Self::driver(None)?.needs_rehash(hashed, options)
    }

    pub fn is_hashed(value: &str) -> HashResult<bool> {
        Self::get_facade_root()?.is_hashed(value)
    }

    pub fn verify_configuration(hashed: &str) -> HashResult<bool> {
        Self::driver(None)?.verify_configuration(hashed)
    }
}
