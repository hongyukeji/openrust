use std::cmp::Ordering;

use super::{ArrayKey, Collection, LazyCollection};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn sort(self) -> Self
    where
        T: Ord,
    {
        self.passthru(Collection::sort)
    }

    pub fn sort_desc(self) -> Self
    where
        T: Ord,
    {
        self.passthru(Collection::sort_desc)
    }

    pub fn sort_using(self, compare: impl Fn(&T, &T) -> Ordering + 'source) -> Self {
        self.passthru(move |items| items.sort_using(&compare))
    }

    pub fn sort_by<K: Ord>(self, project: impl Fn(&T, &ArrayKey) -> K + 'source) -> Self {
        self.passthru(move |items| items.sort_by(&project))
    }

    pub fn sort_by_desc<K: Ord>(self, project: impl Fn(&T, &ArrayKey) -> K + 'source) -> Self {
        self.passthru(move |items| items.sort_by_desc(&project))
    }

    pub fn sort_keys(self) -> Self {
        self.passthru(Collection::sort_keys)
    }

    pub fn sort_keys_desc(self) -> Self {
        self.passthru(Collection::sort_keys_desc)
    }

    pub fn sort_keys_using(
        self,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self {
        self.passthru(move |items| items.sort_keys_using(&compare))
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

    pub fn duplicates_strict(self) -> Self
    where
        T: PartialEq,
    {
        self.duplicates()
    }

    pub fn duplicates_by<U: PartialEq + 'source>(
        self,
        project: impl Fn(&T, &ArrayKey) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        self.passthru(move |items| items.map(&project).duplicates())
    }
}
