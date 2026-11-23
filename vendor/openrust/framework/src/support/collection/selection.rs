use std::{cmp::Ordering, collections::BTreeMap};

use super::{ArrayKey, Collection, CollectionError, CollectionResult, percentage_from_counts};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<T> Collection<T> {
    pub fn containsStrict(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.contains(value)
    }

    pub fn doesntContain(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        !self.contains(value)
    }

    pub fn doesntContainStrict(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.doesntContain(value)
    }

    pub fn some(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> bool {
        self.containsWhere(predicate)
    }

    pub fn hasMany<K: Into<ArrayKey>>(&self, keys: impl IntoIterator<Item = K>) -> bool {
        keys.into_iter().all(|key| self.has(key))
    }

    pub fn hasSole(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> bool {
        self.soleWhere(predicate).is_ok()
    }

    pub fn search(&self, value: &T) -> Option<&ArrayKey>
    where
        T: PartialEq,
    {
        self.searchWhere(|item, _| item == value)
    }

    pub fn searchWhere(
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

    pub fn diffUsing(self, other: &Self, mut compare: impl FnMut(&T, &T) -> Ordering) -> Self {
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

    pub fn intersectUsing(self, other: &Self, mut compare: impl FnMut(&T, &T) -> Ordering) -> Self {
        self.filter(|item, _| {
            other
                .items
                .values()
                .any(|candidate| compare(item, candidate).is_eq())
        })
    }

    pub fn diffAssoc(self, other: &Self) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, key| other.get(key) != Some(item))
    }

    pub fn intersectAssoc(self, other: &Self) -> Self
    where
        T: PartialEq,
    {
        self.filter(|item, key| other.get(key) == Some(item))
    }

    pub fn diffAssocUsing(
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

    pub fn intersectAssocUsing(
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

    pub fn diffKeys<U>(self, other: &Collection<U>) -> Self {
        self.filter(|_, key| !other.has(key))
    }

    pub fn diffKeysUsing<U>(
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

    pub fn intersectByKeys<U>(self, other: &Collection<U>) -> Self {
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

    pub fn duplicatesStrict(self) -> Self
    where
        T: PartialEq,
    {
        self.duplicates()
    }

    pub fn uniqueStrict(self) -> Self
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

    pub fn collapseWithKeys<U>(self) -> Collection<U>
    where
        T: IntoIterator<Item = (ArrayKey, U)>,
    {
        Collection::fromEntries(self.items.into_values().flatten())
    }
}
