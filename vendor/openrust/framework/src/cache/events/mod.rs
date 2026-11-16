//! Events dispatched during cache operations.

mod cache_failed_over;

pub use cache_failed_over::CacheFailedOver;

mod operations;

pub use operations::*;
