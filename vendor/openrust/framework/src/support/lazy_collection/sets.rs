use std::{cmp::Ordering, iter, mem};

use indexmap::IndexMap;

use super::{ArrayKey, Collection, CollectionResult, LazyCollection, SourceIterator};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn diff(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.diff(&right))
    }

    pub fn diffUsing(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&T, &T) -> Ordering + 'source,
    ) -> Self {
        self.set_operation(other, move |left, right| left.diffUsing(&right, &compare))
    }

    pub fn diffAssoc(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.diffAssoc(&right))
    }

    pub fn diffAssocUsing(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, move |left, right| {
            left.diffAssocUsing(&right, &compare)
        })
    }

    pub fn diffKeys<U: 'source>(self, other: impl Into<LazyCollection<'source, U>>) -> Self {
        self.set_operation(other, |left, right| left.diffKeys(&right))
    }

    pub fn diffKeysUsing<U: 'source>(
        self,
        other: impl Into<LazyCollection<'source, U>>,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self {
        self.set_operation(other, move |left, right| {
            left.diffKeysUsing(&right, &compare)
        })
    }

    pub fn intersect(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.intersect(&right))
    }

    pub fn intersectUsing(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&T, &T) -> Ordering + 'source,
    ) -> Self {
        self.set_operation(other, move |left, right| {
            left.intersectUsing(&right, &compare)
        })
    }

    pub fn intersectAssoc(self, other: impl Into<Self>) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, |left, right| left.intersectAssoc(&right))
    }

    pub fn intersectAssocUsing(
        self,
        other: impl Into<Self>,
        compare: impl Fn(&ArrayKey, &ArrayKey) -> Ordering + 'source,
    ) -> Self
    where
        T: PartialEq,
    {
        self.set_operation(other, move |left, right| {
            left.intersectAssocUsing(&right, &compare)
        })
    }

    pub fn intersectByKeys<U: 'source>(self, other: impl Into<LazyCollection<'source, U>>) -> Self {
        self.set_operation(other, |left, right| left.intersectByKeys(&right))
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
                .getIterator()
                .map(|entry| entry.map(|(key, value)| (key, Some(value))))
                .collect();
            let mut replacements = match replacements {
                Ok(items) => items,
                Err(error) => return Box::new(iter::once(Err(error))),
            };
            let mut source = self.getIterator();
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
