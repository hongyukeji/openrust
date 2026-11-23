use std::sync::Arc;

use crate::{container::ServiceKey, contracts::hashing::Hasher, support::facades::Facade};

use super::{HashInfo, HashManager, HashOptions, HashResult};

/// Resolves the application hash manager.
pub struct Hash;

impl Facade for Hash {
    type Service = HashManager;

    fn getFacadeAccessor() -> ServiceKey {
        "hash".into()
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Hash {
    pub fn extend(
        name: impl Into<String>,
        factory: impl Fn(&HashManager) -> HashResult<Arc<dyn Hasher>> + Send + Sync + 'static,
    ) -> HashResult<()> {
        Self::getFacadeRoot()?.extend(name, factory);
        Ok(())
    }

    pub fn forgetDrivers() -> HashResult<()> {
        Self::getFacadeRoot()?.forgetDrivers();
        Ok(())
    }

    pub fn driver(name: Option<&str>) -> HashResult<Arc<dyn Hasher>> {
        Self::getFacadeRoot()?.driver(name)
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
        Self::getFacadeRoot()?.isHashed(value)
    }

    pub fn verifyConfiguration(hashed: &str) -> HashResult<bool> {
        Self::driver(None)?.verifyConfiguration(hashed)
    }
}
