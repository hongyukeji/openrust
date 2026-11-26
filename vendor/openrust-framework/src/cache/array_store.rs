use std::sync::Arc;

use super::{CacheResult, TaggedCache, engine::Engine, store::StoreCore};

/// Stores values and locks in application memory.
pub struct ArrayStore(StoreCore);

super::store::impl_store!(
    ArrayStore,
    fn tags(self: Arc<Self>, names: &[&str]) -> CacheResult<TaggedCache> {
        Ok(TaggedCache::new(self, names))
    },
    fn supports_tags(&self) -> bool {
        true
    }
);

impl ArrayStore {
    pub fn new() -> Self {
        Self(StoreCore::new(
            Engine::array(),
            Engine::array(),
            String::new(),
            false,
            true,
        ))
    }
}

impl Default for ArrayStore {
    fn default() -> Self {
        Self::new()
    }
}
