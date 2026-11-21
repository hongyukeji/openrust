use crate::{
    EntryResult, EntryType, EntryUpdate, IncomingEntry, Result, storage::EntryQueryOptions,
};
use std::{future::Future, pin::Pin};
pub type TelescopeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub trait ClearableRepository: Send + Sync {
    fn clear(&self) -> TelescopeFuture<'_, Result<()>>;
}
pub trait PrunableRepository: Send + Sync {
    fn prune(
        &self,
        before: jiff::Timestamp,
        keepExceptions: bool,
    ) -> TelescopeFuture<'_, Result<u64>>;
}
/// A scoped storage backend. Monitoring snapshots are loaded before recording starts.
pub trait EntriesRepository: ClearableRepository + PrunableRepository {
    fn find<'a>(&'a self, id: &'a str) -> TelescopeFuture<'a, Result<EntryResult>>;
    fn get(
        &self,
        kind: Option<EntryType>,
        options: EntryQueryOptions,
    ) -> TelescopeFuture<'_, Result<Vec<EntryResult>>>;
    fn store<'a>(&'a self, entries: &'a [IncomingEntry]) -> TelescopeFuture<'a, Result<()>>;
    fn update<'a>(
        &'a self,
        updates: &'a [EntryUpdate],
    ) -> TelescopeFuture<'a, Result<Vec<EntryUpdate>>>;
    fn monitoring(&self) -> TelescopeFuture<'_, Result<Vec<String>>>;
    fn loadMonitoredTags(&self) -> TelescopeFuture<'_, ()>;
    fn hasMonitoredTag(&self, tags: &[String]) -> bool;
    fn isMonitoring<'a>(&'a self, tags: &'a [String]) -> TelescopeFuture<'a, bool>;
    fn monitor<'a>(&'a self, tags: &'a [String]) -> TelescopeFuture<'a, Result<()>>;
    fn stopMonitoring<'a>(&'a self, tags: &'a [String]) -> TelescopeFuture<'a, Result<()>>;
    fn terminate(&self);
}
