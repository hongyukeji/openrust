use std::{cmp::Ordering, collections::BTreeMap};

use super::{ArrayKey, Collection, CollectionError, CollectionResult, percentage_from_counts};

impl<T> Collection<T> {
    pub fn contains_strict(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.contains(value)
    }

    pub fn doesnt_contain(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        !self.contains(value)
    }

    pub fn doesnt_contain_strict(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.doesnt_contain(value)
    }

    pub fn some(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> bool {
        self.contains_where(predicate)
    }

    pub fn has_many<K: Into<ArrayKey>>(&self, keys: impl IntoIterator<Item = K>) -> bool {
        keys.into_iter().all(|key| self.has(key))
    }

    pub fn has_sole(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> bool {
        self.sole_where(predicate).is_ok()
    }

    pub fn search(&self, value: &T) -> Option<&ArrayKey>
    where
        T: PartialEq,
    {
        self.search_where(|item, _| item == value)
    }

    pub fn search_where(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> Option<&ArrayKey> {
        self.iter()
            .find_map(|(key, item)| predicate(item, key).then_some(key))
    }

    pub fn before(&self, value: &T) -> Option<&T>
    where
        T: PartialEq,
    {
        let index = self
            .items
            .get_index_of(self.search(value)?)?
            .checked_sub(1)?;
        self.items.get_index(index).map(|(_, item)| item)
    }

    pub fn after(&self, value: &T) -> Option<&T>
    where
        T: PartialEq,
    {
        let index = self.items.get_index_of(self.search(value)?)? + 1;
        self.items.get_index(index).map(|(_, item)| item)
    }

    pub fn diff(self, other: &Self) -> Self
    where
        T: PartialEq,
    {
        self.reject(|item, _| other.contains(item))
    }

    pub fn diff_using(self, other: &Self, mut compare: impl FnMut(&T, &T) -> Ordering) -> Self {
        self.reject(|item, _| {
            other
                .items
                .values()
                .any(|candidate| compare(item, candidate).is_eq())
        })
    }

    pub fn intersect(self, other: &Self) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, _| other.contains(item))
    }

    pub fn intersect_using(
        self,
        other: &Self,
        mut compare: impl FnMut(&T, &T) -> Ordering,
    ) -> Self {
        self.filter(|item, _| {
            other
                .items
                .values()
                .any(|candidate| compare(item, candidate).is_eq())
        })
    }

    pub fn diff_assoc(self, other: &Self) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, key| other.get(key) != Some(item))
    }

    pub fn intersect_assoc(self, other: &Self) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, key| other.get(key) == Some(item))
    }

    pub fn diff_assoc_using(
        self,
        other: &Self,
        mut keys: impl FnMut(&ArrayKey, &ArrayKey) -> Ordering,
    ) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, key| {
            !other
                .iter()
                .any(|(candidate, value)| item == value && keys(key, candidate).is_eq())
        })
    }

    pub fn intersect_assoc_using(
        self,
        other: &Self,
        mut keys: impl FnMut(&ArrayKey, &ArrayKey) -> Ordering,
    ) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, key| {
            other
                .iter()
                .any(|(candidate, value)| item == value && keys(key, candidate).is_eq())
        })
    }

    pub fn diff_keys<U>(self, other: &Collection<U>) -> Self {
        self.filter(|_, key| !other.has(key))
    }

    pub fn diff_keys_using<U>(
        self,
        other: &Collection<U>,
        mut keys: impl FnMut(&ArrayKey, &ArrayKey) -> Ordering,
    ) -> Self {
        self.filter(|_, key| {
            !other
                .items
                .keys()
                .any(|candidate| keys(key, candidate).is_eq())
        })
    }

    pub fn intersect_by_keys<U>(self, other: &Collection<U>) -> Self {
        self.filter(|_, key| other.has(key))
    }

    pub fn duplicates(self) -> Self
    where
        T: PartialEq,
    {
        let mut seen = Vec::new();
        let mut repeated = Self::new();
        for (key, value) in self {
            if seen.contains(&value) {
                repeated.put(key, value);
            } else {
                seen.push(value);
            }
        }
        repeated
    }

    pub fn duplicates_strict(self) -> Self
    where
        T: PartialEq,
    {
        self.duplicates()
    }

    pub fn unique_strict(self) -> Self
    where
        T: Eq + std::hash::Hash + Clone,
    {
        self.unique()
    }

    pub fn mode(&self) -> Vec<&T>
    where
        T: Ord,
    {
        let mut counts = BTreeMap::new();
        for value in self.items.values() {
            *counts.entry(value).or_insert(0_usize) += 1;
        }
        let highest = counts.values().copied().max().unwrap_or(0);
        counts
            .into_iter()
            .filter_map(|(value, count)| (count == highest).then_some(value))
            .collect()
    }

    pub fn percentage(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
        precision: u8,
    ) -> Option<f64> {
        let matches = self
            .iter()
            .filter(|(key, item)| predicate(item, key))
            .count();
        percentage_from_counts(matches, self.count(), precision)
    }

    pub fn combine<U>(self, values: impl IntoIterator<Item = U>) -> CollectionResult<Collection<U>>
    where
        T: Into<ArrayKey>,
    {
        let mut values = values.into_iter();
        let mut result = Collection::new();
        for key in self.items.into_values() {
            result.put(key, values.next().ok_or(CollectionError::InvalidArgument)?);
        }
        if values.next().is_some() {
            return Err(CollectionError::InvalidArgument);
        }
        Ok(result)
    }

    pub fn collapse_with_keys<U>(self) -> Collection<U>
    where
        T: IntoIterator<Item = (ArrayKey, U)>,
    {
        Collection::from_entries(self.items.into_values().flatten())
    }
}
