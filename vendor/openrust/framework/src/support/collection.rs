use std::fmt;

use indexmap::IndexMap;

mod finite;
mod numbers;
mod operations;
mod serialization;

pub use numbers::CollectionNumber;

use super::Enumerable;

/// An integer or string key in an ordered collection.
#[derive(
    Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(untagged)]
pub enum ArrayKey {
    Integer(i64),
    String(String),
}

impl ArrayKey {
    fn normalized(self) -> Self {
        match self {
            Self::String(value) => Self::from(value),
            key => key,
        }
    }
}

impl From<i64> for ArrayKey {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<bool> for ArrayKey {
    fn from(value: bool) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<String> for ArrayKey {
    fn from(value: String) -> Self {
        match value.parse::<i64>() {
            Ok(integer) if integer.to_string() == value => Self::Integer(integer),
            _ => Self::String(value),
        }
    }
}

impl From<&str> for ArrayKey {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl From<&ArrayKey> for ArrayKey {
    fn from(value: &ArrayKey) -> Self {
        value.clone()
    }
}

impl fmt::Display for ArrayKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(formatter),
            Self::String(value) => value.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NextIndex {
    Unset,
    Key(i64),
    Exhausted,
}

impl NextIndex {
    fn after(index: i64) -> Self {
        index.checked_add(1).map_or(Self::Exhausted, Self::Key)
    }
}

/// A materialized collection that preserves key and iteration order.
#[derive(Clone)]
pub struct Collection<T = serde_json::Value> {
    items: IndexMap<ArrayKey, T>,
    next_index: NextIndex,
}

impl<T> Default for Collection<T> {
    fn default() -> Self {
        Self {
            items: IndexMap::new(),
            next_index: NextIndex::Unset,
        }
    }
}

impl<T> fmt::Debug for Collection<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Collection")
            .field("len", &self.items.len())
            .finish_non_exhaustive()
    }
}

impl<T: PartialEq> PartialEq for Collection<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items.iter().eq(other.items.iter())
    }
}

impl<T: Eq> Eq for Collection<T> {}

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T> Collection<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn make(items: impl IntoIterator<Item = T>) -> Self {
        items.into_iter().collect()
    }

    pub fn fromEntries<K: Into<ArrayKey>>(items: impl IntoIterator<Item = (K, T)>) -> Self {
        let mut collection = Self::new();
        for (key, value) in items {
            collection.put(key, value);
        }
        collection
    }

    pub fn all(&self) -> &IndexMap<ArrayKey, T> {
        &self.items
    }

    pub fn iter(&self) -> indexmap::map::Iter<'_, ArrayKey, T> {
        self.items.iter()
    }

    pub fn get(&self, key: impl Into<ArrayKey>) -> Option<&T> {
        self.items.get(&key.into().normalized())
    }

    pub fn getMut(&mut self, key: impl Into<ArrayKey>) -> Option<&mut T> {
        self.items.get_mut(&key.into().normalized())
    }

    pub fn has(&self, key: impl Into<ArrayKey>) -> bool {
        self.get(key).is_some()
    }

    pub fn hasAny<K: Into<ArrayKey>>(&self, keys: impl IntoIterator<Item = K>) -> bool {
        keys.into_iter().any(|key| self.has(key))
    }

    pub fn put(&mut self, key: impl Into<ArrayKey>, value: T) -> &mut Self {
        let key = key.into().normalized();
        if let ArrayKey::Integer(index) = key {
            match self.next_index {
                NextIndex::Unset => self.next_index = NextIndex::after(index),
                NextIndex::Key(next) if index >= next => self.next_index = NextIndex::after(index),
                _ => {}
            }
        }
        self.items.insert(key, value);
        self
    }

    pub fn push(&mut self, value: T) -> CollectionResult<&mut Self> {
        let next = match self.next_index {
            NextIndex::Unset => 0,
            NextIndex::Key(next) => next,
            NextIndex::Exhausted => return Err(CollectionError::IndexOverflow),
        };
        Ok(self.put(next, value))
    }

    pub fn forget(&mut self, key: impl Into<ArrayKey>) -> &mut Self {
        self.pull(key);
        self
    }

    pub fn pull(&mut self, key: impl Into<ArrayKey>) -> Option<T> {
        self.items.shift_remove(&key.into().normalized())
    }

    pub fn pop(&mut self) -> Option<T> {
        let (key, value) = self.items.pop()?;
        // Rewind only when the last numeric append slot is popped.
        if let ArrayKey::Integer(index) = key
            && self.next_index == NextIndex::after(index)
        {
            self.next_index = NextIndex::Key(index);
        }
        Some(value)
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn isEmpty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn isNotEmpty(&self) -> bool {
        !self.isEmpty()
    }

    pub fn first(&self) -> Option<&T> {
        self.items.first().map(|(_, value)| value)
    }

    pub fn firstWhere(&self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Option<&T> {
        self.items
            .iter()
            .find_map(|(key, value)| predicate(value, key).then_some(value))
    }

    pub fn firstOrFail(&self) -> CollectionResult<&T> {
        self.first().ok_or(CollectionError::ItemNotFound)
    }

    pub fn last(&self) -> Option<&T> {
        self.items.last().map(|(_, value)| value)
    }

    pub fn sole(&self) -> CollectionResult<&T> {
        self.soleWhere(|_, _| true)
    }

    pub fn soleWhere(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<&T> {
        let mut matches = self
            .items
            .iter()
            .filter_map(|(key, value)| predicate(value, key).then_some(value));
        let item = matches.next().ok_or(CollectionError::ItemNotFound)?;
        if matches.next().is_some() {
            return Err(CollectionError::MultipleItemsFound);
        }
        Ok(item)
    }
}

impl<T> FromIterator<T> for Collection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::fromEntries(iter.into_iter().enumerate().map(|(index, value)| {
            (
                i64::try_from(index).expect("a collection fits within the integer key space"),
                value,
            )
        }))
    }
}

impl<T> IntoIterator for Collection<T> {
    type Item = (ArrayKey, T);
    type IntoIter = indexmap::map::IntoIter<ArrayKey, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Collection<T> {
    type Item = (&'a ArrayKey, &'a T);
    type IntoIter = indexmap::map::Iter<'a, ArrayKey, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'source, T: 'source> Enumerable<'source> for Collection<T> {
    type Item = T;
    type Rebind<U: 'source> = Collection<U>;

    fn all(self) -> CollectionResult<Vec<(ArrayKey, T)>> {
        Ok(self.into_iter().collect())
    }

    fn collect(self) -> CollectionResult<Self> {
        Ok(self)
    }

    fn map<U: 'source>(self, callback: impl Fn(&T, &ArrayKey) -> U + 'source) -> Collection<U> {
        self.map(callback)
    }

    fn filter(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        self.filter(predicate)
    }

    fn values(self) -> Self {
        self.values()
    }

    fn take(self, limit: isize) -> Self {
        self.take(limit)
    }
}

pub type CollectionResult<T> = Result<T, CollectionError>;

/// A collection source or operation could not produce its result.
#[derive(Debug)]
#[non_exhaustive]
pub enum CollectionError {
    InvalidArgument,
    IndexOverflow,
    ItemNotFound,
    MultipleItemsFound,
    Source(Box<dyn std::error::Error + Send + Sync>),
    Serialization(serde_json::Error),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidArgument => "invalid collection argument",
            Self::IndexOverflow => "collection integer key overflow",
            Self::ItemNotFound => "collection item not found",
            Self::MultipleItemsFound => "multiple collection items found",
            Self::Source(_) => "collection source failed",
            Self::Serialization(_) => "collection serialization failed",
        })
    }
}

impl std::error::Error for CollectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error.as_ref()),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}
