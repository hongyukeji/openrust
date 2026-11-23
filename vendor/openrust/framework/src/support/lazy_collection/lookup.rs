use std::collections::HashSet;

use super::{ArrayKey, CollectionResult, LazyCollection};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn get(&self, key: impl Into<ArrayKey>) -> CollectionResult<Option<T>> {
        let key = key.into().normalized();
        self.first_where(|_, candidate| candidate == &key)
    }

    /// Evaluates the fallback only when the requested key is absent.
    pub fn get_or_else(
        &self,
        key: impl Into<ArrayKey>,
        fallback: impl FnOnce() -> T,
    ) -> CollectionResult<T> {
        Ok(self.get(key)?.unwrap_or_else(fallback))
    }

    pub fn has(&self, key: impl Into<ArrayKey>) -> CollectionResult<bool> {
        Ok(self.get(key)?.is_some())
    }

    pub fn has_all<K: Into<ArrayKey>>(
        &self,
        keys: impl IntoIterator<Item = K>,
    ) -> CollectionResult<bool> {
        let mut remaining: HashSet<_> = keys
            .into_iter()
            .map(|key| key.into().normalized())
            .collect();
        for entry in self {
            let (key, _) = entry?;
            remaining.remove(&key);
            if remaining.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn has_any<K: Into<ArrayKey>>(
        &self,
        keys: impl IntoIterator<Item = K>,
    ) -> CollectionResult<bool> {
        let keys: HashSet<_> = keys
            .into_iter()
            .map(|key| key.into().normalized())
            .collect();
        self.contains_where(|_, key| keys.contains(key))
    }

    pub fn last(&self) -> CollectionResult<Option<T>> {
        self.last_where(|_, _| true)
    }

    /// Evaluates the fallback only when the source is empty.
    pub fn last_or_else(&self, fallback: impl FnOnce() -> T) -> CollectionResult<T> {
        Ok(self.last()?.unwrap_or_else(fallback))
    }

    pub fn last_where(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<Option<T>> {
        let mut result = None;
        for entry in self {
            let (key, value) = entry?;
            if predicate(&value, &key) {
                result = Some(value);
            }
        }
        Ok(result)
    }

    /// Evaluates the fallback only when no entry matches.
    pub fn last_where_or_else(
        &self,
        predicate: impl FnMut(&T, &ArrayKey) -> bool,
        fallback: impl FnOnce() -> T,
    ) -> CollectionResult<T> {
        Ok(self.last_where(predicate)?.unwrap_or_else(fallback))
    }

    pub fn search(&self, value: &T) -> CollectionResult<Option<ArrayKey>>
    where
        T: PartialEq,
    {
        self.search_where(|item, _| item == value)
    }

    pub fn search_where(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<Option<ArrayKey>> {
        for entry in self {
            let (key, value) = entry?;
            if predicate(&value, &key) {
                return Ok(Some(key));
            }
        }
        Ok(None)
    }

    pub fn before(&self, value: &T) -> CollectionResult<Option<T>>
    where
        T: PartialEq,
    {
        self.before_where(|item, _| item == value)
    }

    pub fn before_where(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<Option<T>> {
        let mut previous = None;
        for entry in self {
            let (key, value) = entry?;
            if predicate(&value, &key) {
                return Ok(previous);
            }
            previous = Some(value);
        }
        Ok(None)
    }

    pub fn after(&self, value: &T) -> CollectionResult<Option<T>>
    where
        T: PartialEq,
    {
        self.after_where(|item, _| item == value)
    }

    pub fn after_where(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<Option<T>> {
        let mut source = self.get_iterator();
        while let Some(entry) = source.next() {
            let (key, value) = entry?;
            if predicate(&value, &key) {
                return Ok(source.next().transpose()?.map(|(_, next)| next));
            }
        }
        Ok(None)
    }
}
