use std::sync::Arc;

use crate::support::facades::Facade;

use super::{Cache, CacheError, CacheManager, CacheResult};

/// Resolves the cache manager for the active application.
pub fn cache() -> CacheResult<Arc<CacheManager>> {
    Cache::getFacadeRoot().map_err(CacheError::Unavailable)
}

/// Accesses the active cache manager, reads a value, or writes an entry.
///
/// `cache!()` returns the manager. Read and write forms return futures.
/// With an entry map, the first entry is written, as in Laravel's `cache` helper.
#[macro_export]
macro_rules! cache {
    () => {
        $crate::cache::cache()
    };
    ({ $($key:expr => $value:expr),+ $(,)? }, $ttl:expr $(,)?) => {
        async {
            let entries = [$(($key, $value)),+];
            let (key, value) = &entries[0];
            $crate::cache::Cache::put(key, value, $ttl).await
        }
    };
    ($key:expr, $default:expr $(,)?) => {
        $crate::cache::Cache::getOr($key, $default)
    };
    ($key:expr $(,)?) => {
        $crate::cache::Cache::get($key)
    };
}
