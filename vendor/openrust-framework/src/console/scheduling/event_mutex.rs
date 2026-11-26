use crate::{cache::CacheResult, console::ConsoleFuture};

use super::Event;

/// Controls overlap between runs of an event.
pub trait EventMutex: Send + Sync {
    fn create<'a>(&'a self, event: &'a Event) -> ConsoleFuture<'a, CacheResult<bool>>;
    fn exists<'a>(&'a self, event: &'a Event) -> ConsoleFuture<'a, CacheResult<bool>>;
    fn forget<'a>(&'a self, event: &'a Event) -> ConsoleFuture<'a, CacheResult<()>>;
}
