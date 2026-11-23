use crate::{ScoutFuture, Searchable};
use openrust::database::eloquent::Collection;
/// Eloquent collection methods expressed as a Rust extension trait.
pub trait SearchableCollection<M: Searchable> {
    fn searchable(&self) -> ScoutFuture<'_, ()>;
    fn unsearchable(&self) -> ScoutFuture<'_, ()>;
    fn searchable_sync(&self) -> ScoutFuture<'_, ()>;
    fn unsearchable_sync(&self) -> ScoutFuture<'_, ()>;
}
impl<M: Searchable> SearchableCollection<M> for Collection<M> {
    fn searchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scout_manager()?.sync(self.all(), false, false).await })
    }
    fn unsearchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scout_manager()?.sync(self.all(), true, false).await })
    }
    fn searchable_sync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scout_manager()?.sync(self.all(), false, true).await })
    }
    fn unsearchable_sync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move { M::scout_manager()?.sync(self.all(), true, true).await })
    }
}
