use serde_json::Value;

use super::{ArrayKey, CollectionResult, LazyCollection};
use crate::support::data;

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn mapInto<U: From<T> + 'source>(self) -> LazyCollection<'source, U> {
        LazyCollection::from_iterators(move || {
            self.getIterator()
                .map(|entry| entry.map(|(key, item)| (key, item.into())))
        })
    }

    pub fn multiply(self, count: usize) -> Self
    where
        T: Clone,
    {
        self.passthru(move |items| items.multiply(count))
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, Vec<T>> {
    pub fn mapSpread<U: 'source>(
        self,
        callback: impl Fn(&[T], &ArrayKey) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        self.map(move |chunk, key| callback(chunk, key))
    }

    pub fn eachSpread(
        &self,
        mut callback: impl FnMut(&[T], &ArrayKey) -> bool,
    ) -> CollectionResult<&Self> {
        self.each(move |chunk, key| callback(chunk, key))
    }
}

impl LazyCollection<'_, ArrayKey> {
    pub fn flip(self) -> Self {
        Self::from_iterators(move || {
            self.getIterator()
                .map(|entry| entry.map(|(key, value)| (value.normalized(), key)))
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source> LazyCollection<'source, Value> {
    pub fn select(self, keys: &'source [&'source str]) -> Self {
        self.map(move |item, _| {
            data::packed(
                keys.iter()
                    .filter_map(|&key| {
                        data::direct(item, key).map(|value| (key.to_owned(), value.clone()))
                    })
                    .collect(),
            )
        })
        .values()
    }

    pub fn mergeRecursive(self, other: impl Into<Self>) -> Self {
        let other = other.into();
        self.try_passthru(move |items| items.mergeRecursive(Self::collect(&other)?))
    }

    pub fn replaceRecursive(self, other: impl Into<Self>) -> Self {
        let other = other.into();
        self.try_passthru(move |items| items.replaceRecursive(Self::collect(&other)?))
    }
}
