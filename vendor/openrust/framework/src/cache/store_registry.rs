use std::sync::Arc;

use super::{CacheError, CacheResult, Repository};

#[derive(Default)]
pub(super) struct StoreRegistry(crate::support::driver_registry::DriverRegistry<Repository>);

impl StoreRegistry {
    pub(super) fn get_or_resolve(
        &self,
        name: &str,
        resolve: impl FnOnce() -> CacheResult<Repository>,
    ) -> CacheResult<Arc<Repository>> {
        self.0.get_or_resolve(
            name,
            || resolve().map(Arc::new),
            CacheError::StoreResolutionCycle,
        )
    }

    pub(super) fn forget(&self, name: &str) {
        self.0.forget(name);
    }
    pub(super) fn resolved(&self) -> Vec<Arc<Repository>> {
        self.0.resolved()
    }
}
