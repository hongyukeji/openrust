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
        keep_exceptions: bool,
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
    /// 查询公开列表时，将原始批次和游标文本交给仓储。
    /// 内置数据库列表沿用上游，只通过原生 `get` 启用 UUID 集合筛选。
    fn get_with_filter_inputs(
        &self,
        kind: Option<EntryType>,
        options: EntryQueryOptions,
        batch: Option<String>,
        before: Option<String>,
    ) -> TelescopeFuture<'_, Result<Vec<EntryResult>>> {
        Box::pin(async move {
            self.get(kind, options.with_filter_inputs(batch, before)?)
                .await
        })
    }
    fn store<'a>(&'a self, entries: &'a [IncomingEntry]) -> TelescopeFuture<'a, Result<()>>;
    fn update<'a>(
        &'a self,
        updates: &'a [EntryUpdate],
    ) -> TelescopeFuture<'a, Result<Vec<EntryUpdate>>>;
    fn monitoring(&self) -> TelescopeFuture<'_, Result<Vec<String>>>;
    fn load_monitored_tags(&self) -> TelescopeFuture<'_, ()>;
    fn has_monitored_tag(&self, tags: &[String]) -> bool;
    fn is_monitoring<'a>(&'a self, tags: &'a [String]) -> TelescopeFuture<'a, bool>;
    fn monitor<'a>(&'a self, tags: &'a [String]) -> TelescopeFuture<'a, Result<()>>;
    fn stop_monitoring<'a>(&'a self, tags: &'a [String]) -> TelescopeFuture<'a, Result<()>>;
    fn terminate(&self);
}
