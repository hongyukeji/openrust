use crate::{ScoutFuture, Searchable};
use openrust::database::eloquent::Collection;
/// Eloquent collection methods expressed as a Rust extension trait.
pub trait SearchableCollection<M: Searchable> {
    fn searchable(&self) -> ScoutFuture<'_, ()>;
    fn unsearchable(&self) -> ScoutFuture<'_, ()>;
    fn searchableSync(&self) -> ScoutFuture<'_, ()>;
    fn unsearchableSync(&self) -> ScoutFuture<'_, ()>;
}
impl<M: Searchable> SearchableCollection<M> for Collection<M> {
    fn searchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scoutManager()?.sync(self.all(), false, false).await })
    }
    fn unsearchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scoutManager()?.sync(self.all(), true, false).await })
    }
    fn searchableSync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scoutManager()?.sync(self.all(), false, true).await })
    }
    fn unsearchableSync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scoutManager()?.sync(self.all(), true, true).await })
    }
}
