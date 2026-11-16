use std::cmp::Ordering;

use super::{ArrayKey, Collection, LazyCollection};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn sort(self) -> Self
    where
        T: Ord,
    {
        self.passthru(Collection::sort)
    }

    pub fn sortDesc(self) -> Self
    where
        T: Ord,
    {
        self.passthru(Collection::sortDesc)
    }

    pub fn sortUsing(self, compare: impl Fn(&T, &T) -> Ordering + 'source) -> Self {
        self.passthru(move |items| items.sortUsing(&compare))
    }

    pub fn sortBy<K: Ord>(self, project: impl Fn(&T, &ArrayKey) -> K + 'source) -> Self {
        self.passthru(move |items| items.sortBy(&project))
    }

    pub fn sortByDesc<K: Ord>(self, project: impl Fn(&T, &ArrayKey) -> K + 'source) -> Self {
        self.passthru(move |items| items.sortByDesc(&project))
    }

    pub fn sortKeys(self) -> Self {
        self.passthru(Collection::sortKeys)
    }

    pub fn sortKeysDesc(self) -> Self {
        self.passthru(Collection::sortKeysDesc)
    }

    pub fn sortKeysUsing(
        self,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self {
        self.passthru(move |items| items.sortKeysUsing(&compare))
    }

    pub fn reverse(self) -> Self {
        self.passthru(Collection::reverse)
    }

    pub fn duplicates(self) -> Self
    where
        T: PartialEq,
    {
        self.passthru(Collection::duplicates)
    }

    pub fn duplicatesStrict(self) -> Self
    where
        T: PartialEq,
    {
        self.duplicates()
    }

    pub fn duplicatesBy<U: PartialEq + 'source>(
        self,
        project: impl Fn(&T, &ArrayKey) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        self.passthru(move |items| items.map(&project).duplicates())
    }
}
