use std::sync::Arc;

use crate::{container::ServiceKey, contracts::hashing::Hasher, support::facades::Facade};

use super::{HashError, HashInfo, HashManager, HashOptions, HashResult};

/// Resolves the application hash manager.
pub struct Hash;

impl Facade for Hash {
    type Service = HashManager;

    fn getFacadeAccessor() -> ServiceKey {
        "hash".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Hash facade API")]
impl Hash {
    pub fn driver(name: Option<&str>) -> HashResult<Arc<dyn Hasher>> {
        Self::getFacadeRoot()
            .map_err(HashError::Unavailable)?
            .driver(name)
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

    pub fn needsRehash(hashed: &str, options: &HashOptions) -> HashResult<bool> {
        Self::driver(None)?.needsRehash(hashed, options)
    }

    pub fn isHashed(value: &str) -> HashResult<bool> {
        Self::getFacadeRoot()
            .map_err(HashError::Unavailable)?
            .isHashed(value)
    }

    pub fn verifyConfiguration(hashed: &str) -> HashResult<bool> {
        Self::driver(None)?.verifyConfiguration(hashed)
    }
}
