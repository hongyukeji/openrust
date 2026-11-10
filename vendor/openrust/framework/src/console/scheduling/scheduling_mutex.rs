use std::time::SystemTime;

use crate::{cache::CacheResult, console::ConsoleFuture};

use super::Event;

/// Elects one runner for a scheduled event and time slot.
pub trait SchedulingMutex: Send + Sync {
    fn create<'a>(
        &'a self,
        event: &'a Event,
        time: SystemTime,
    ) -> ConsoleFuture<'a, CacheResult<bool>>;
    fn exists<'a>(
        &'a self,
        event: &'a Event,
        time: SystemTime,
    ) -> ConsoleFuture<'a, CacheResult<bool>>;
}
