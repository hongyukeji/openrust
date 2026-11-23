use crate::contracts::events::Event;

/// Reports a backing store that newly failed during a cache operation.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub struct CacheFailedOver {
    pub storeName: String,
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
