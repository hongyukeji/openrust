use std::sync::Arc;

use crate::cache::CacheResult;

use super::Repository;

/// Resolves the default or a named cache repository.
pub trait Factory: Send + Sync {
    fn store(&self, name: Option<&str>) -> CacheResult<Arc<dyn Repository>>;
}
