use super::{engine::Engine, store::StoreCore};

/// Stores values and locks in application memory.
pub struct ArrayStore(StoreCore);

super::store::impl_store!(ArrayStore);

impl ArrayStore {
    pub fn new() -> Self {
        Self(StoreCore::new(
            Engine::array(),
            Engine::array(),
            String::new(),
            false,
        ))
    }
}

impl Default for ArrayStore {
    fn default() -> Self {
        Self::new()
    }
}
