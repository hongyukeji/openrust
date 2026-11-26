use serde_json::Value;

use super::{ArrayKey, CollectionResult, LazyCollection};
use crate::support::data;

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn map_into<U: From<T> + 'source>(self) -> LazyCollection<'source, U> {
        LazyCollection::from_iterators(move || {
            self.get_iterator()
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

impl<'source, T: 'source> LazyCollection<'source, Vec<T>> {
    pub fn map_spread<U: 'source>(
        self,
        callback: impl Fn(&[T], &ArrayKey) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        self.map(move |chunk, key| callback(chunk, key))
    }

    pub fn each_spread(
        &self,
        mut callback: impl FnMut(&[T], &ArrayKey) -> bool,
    ) -> CollectionResult<&Self> {
        self.each(move |chunk, key| callback(chunk, key))
    }
}

impl LazyCollection<'_, ArrayKey> {
    pub fn flip(self) -> Self {
        Self::from_iterators(move || {
            self.get_iterator()
                .map(|entry| entry.map(|(key, value)| (value.normalized(), key)))
        })
    }
}

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

    pub fn merge_recursive(self, other: impl Into<Self>) -> Self {
        let other = other.into();
        self.try_passthru(move |items| items.merge_recursive(Self::collect(&other)?))
    }

    pub fn replace_recursive(self, other: impl Into<Self>) -> Self {
        let other = other.into();
        self.try_passthru(move |items| items.replace_recursive(Self::collect(&other)?))
    }
}
