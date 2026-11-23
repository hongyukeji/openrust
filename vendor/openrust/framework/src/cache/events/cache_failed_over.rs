use crate::contracts::events::Event;

/// Reports a backing store that newly failed during a cache operation.
pub struct CacheFailedOver {
    pub store_name: String,
    pub exception: String,
}

impl CacheFailedOver {
    pub const NAME: &str = "CacheFailedOver";
}

impl Event for CacheFailedOver {
    fn name(&self) -> &str {
        Self::NAME
    }
}
