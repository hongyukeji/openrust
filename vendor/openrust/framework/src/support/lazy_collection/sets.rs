use std::{cmp::Ordering, iter, mem};

use indexmap::IndexMap;

use super::{ArrayKey, Collection, CollectionResult, LazyCollection, SourceIterator};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn diff(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.diff(&right))
    }

    pub fn diff_using(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&T, &T) -> Ordering + 'source,
    ) -> Self {
        self.set_operation(other, move |left, right| left.diff_using(&right, &compare))
    }

    pub fn diff_assoc(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.diff_assoc(&right))
    }

    pub fn diff_assoc_using(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, move |left, right| {
            left.diff_assoc_using(&right, &compare)
        })
    }

    pub fn diff_keys<U: 'source>(self, other: impl Into<LazyCollection<'source, U>>) -> Self {
        self.set_operation(other, |left, right| left.diff_keys(&right))
    }

    pub fn diff_keys_using<U: 'source>(
        self,
        other: impl Into<LazyCollection<'source, U>>,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self {
        self.set_operation(other, move |left, right| {
            left.diff_keys_using(&right, &compare)
        })
    }

    pub fn intersect(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.intersect(&right))
    }

    pub fn intersect_using(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&T, &T) -> Ordering + 'source,
    ) -> Self {
        self.set_operation(other, move |left, right| {
            left.intersect_using(&right, &compare)
        })
    }

    pub fn intersect_assoc(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.intersect_assoc(&right))
    }

    pub fn intersect_assoc_using(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, move |left, right| {
            left.intersect_assoc_using(&right, &compare)
        })
    }

    pub fn intersect_by_keys<U: 'source>(
        self,
        other: impl Into<LazyCollection<'source, U>>,
    ) -> Self {
        self.set_operation(other, |left, right| left.intersect_by_keys(&right))
    }

    pub fn merge(self, other: impl Into<Self>) -> Self {
        self.set_operation(other, Collection::merge)
    }

    pub fn union(self, other: impl Into<Self>) -> Self {
        self.set_operation(other, Collection::union)
    }

    pub fn replace(self, other: impl Into<Self>) -> Self {
        let other = other.into();
        Self::from_iterators(move || -> SourceIterator<'source, T> {
            let replacements: CollectionResult<IndexMap<_, _>> = other
                .get_iterator()
                .map(|entry| entry.map(|(key, value)| (key, Some(value))))
                .collect();
            let mut replacements = match replacements {
                Ok(items) => items,
                Err(error) => return Box::new(iter::once(Err(error))),
            };
            let mut source = self.get_iterator();
            let mut tail = None;
            Box::new(iter::from_fn(move || {
                if let Some(entry) = source.next() {
                    return Some(entry.map(|(key, value)| {
                        let value = replacements
                            .get_mut(&key)
                            .and_then(Option::take)
                            .unwrap_or(value);
                        (key, value)
                    }));
                }
                tail.get_or_insert_with(|| mem::take(&mut replacements).into_iter())
                    .find_map(|(key, value)| value.map(|value| Ok((key, value))))
            }))
        })
    }

    fn set_operation<U: 'source>(
        self,
        other: impl Into<LazyCollection<'source, U>>,
        operation: impl Fn(Collection<T>, Collection<U>) -> Collection<T> + 'source,
    ) -> Self {
        let other = other.into();
        self.try_passthru(move |left| {
            let right = LazyCollection::collect(&other)?;
            Ok(operation(left, right))
        })
    }
}
