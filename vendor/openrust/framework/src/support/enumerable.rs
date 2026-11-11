use super::{ArrayKey, Collection, CollectionResult};

/// Shared operations for materialized and deferred keyed collections.
pub trait Enumerable<'source>: Sized {
    type Item: 'source;
    type Rebind<T: 'source>: Enumerable<'source, Item = T>;

    fn all(self) -> CollectionResult<Vec<(ArrayKey, Self::Item)>>;
    fn collect(self) -> CollectionResult<Collection<Self::Item>>;
    fn map<T: 'source>(
        self,
        callback: impl Fn(&Self::Item, &ArrayKey) -> T + 'source,
    ) -> Self::Rebind<T>;
    fn filter(self, predicate: impl Fn(&Self::Item, &ArrayKey) -> bool + 'source) -> Self;
    fn values(self) -> Self;
    fn take(self, limit: isize) -> Self;
}
