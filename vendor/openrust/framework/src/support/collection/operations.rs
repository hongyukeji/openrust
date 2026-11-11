use std::{cmp::Ordering, collections::HashSet, hash::Hash};

use super::{ArrayKey, Collection, CollectionError, CollectionResult};

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T> Collection<T> {
    pub fn map<U>(self, mut callback: impl FnMut(&T, &ArrayKey) -> U) -> Collection<U> {
        Collection::fromEntries(self.into_iter().map(|(key, value)| {
            let mapped = callback(&value, &key);
            (key, mapped)
        }))
    }

    pub fn tryMap<U, E>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> Result<U, E>,
    ) -> Result<Collection<U>, E> {
        let mut result = Collection::new();
        for (key, value) in self {
            let mapped = callback(&value, &key)?;
            result.put(key, mapped);
        }
        Ok(result)
    }

    pub fn mapWithKeys<K, U, I>(self, mut callback: impl FnMut(&T, &ArrayKey) -> I) -> Collection<U>
    where
        K: Into<ArrayKey>,
        I: IntoIterator<Item = (K, U)>,
    {
        Collection::fromEntries(
            self.into_iter()
                .flat_map(|(key, value)| callback(&value, &key)),
        )
    }

    pub fn filter(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Self {
        Self::fromEntries(
            self.into_iter()
                .filter(|(key, value)| predicate(value, key)),
        )
    }

    pub fn reject(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Self {
        self.filter(|value, key| !predicate(value, key))
    }

    pub fn each(&self, mut callback: impl FnMut(&T, &ArrayKey) -> bool) -> &Self {
        for (key, value) in self {
            if !callback(value, key) {
                break;
            }
        }
        self
    }

    pub fn every(&self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> bool {
        self.iter().all(|(key, value)| predicate(value, key))
    }

    pub fn containsWhere(&self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> bool {
        self.firstWhere(predicate).is_some()
    }

    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.items.values().any(|item| item == value)
    }

    pub fn reduce<U>(&self, initial: U, mut callback: impl FnMut(U, &T, &ArrayKey) -> U) -> U {
        self.iter()
            .fold(initial, |carry, (key, value)| callback(carry, value, key))
    }

    pub fn pluck<U>(&self, mut project: impl FnMut(&T) -> U) -> Collection<U> {
        self.items.values().map(&mut project).collect()
    }

    pub fn pluckWithKeys<K: Into<ArrayKey>, U>(
        &self,
        mut key: impl FnMut(&T) -> K,
        mut project: impl FnMut(&T) -> U,
    ) -> Collection<U> {
        Collection::fromEntries(
            self.items
                .values()
                .map(|value| (key(value), project(value))),
        )
    }

    pub fn keyBy<K: Into<ArrayKey>>(self, mut callback: impl FnMut(&T, &ArrayKey) -> K) -> Self {
        Self::fromEntries(
            self.into_iter()
                .map(|(key, value)| (callback(&value, &key), value)),
        )
    }

    pub fn groupBy<K: Into<ArrayKey>>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> K,
        preserve_keys: bool,
    ) -> Collection<Self> {
        let mut groups = Collection::new();
        for (key, value) in self {
            let group = callback(&value, &key).into().normalized();
            insert_group(&mut groups, group, key, value, preserve_keys);
        }
        groups
    }

    pub fn groupByMany<K, I>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> I,
        preserve_keys: bool,
    ) -> Collection<Self>
    where
        T: Clone,
        K: Into<ArrayKey>,
        I: IntoIterator<Item = K>,
    {
        let mut groups = Collection::new();
        for (key, value) in self {
            for group in callback(&value, &key) {
                insert_group(
                    &mut groups,
                    group.into().normalized(),
                    key.clone(),
                    value.clone(),
                    preserve_keys,
                );
            }
        }
        groups
    }

    pub fn countBy<K: Into<ArrayKey>>(
        &self,
        mut callback: impl FnMut(&T, &ArrayKey) -> K,
    ) -> Collection<usize> {
        let mut counts = Collection::<usize>::new();
        for (key, value) in self {
            let group = callback(value, key).into().normalized();
            if let Some(count) = counts.items.get_mut(&group) {
                *count += 1;
            } else {
                counts.put(group, 1);
            }
        }
        counts
    }

    pub fn partition(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> (Self, Self) {
        let (mut passed, mut failed) = (Self::new(), Self::new());
        for (key, value) in self {
            let target = if predicate(&value, &key) {
                &mut passed
            } else {
                &mut failed
            };
            target.put(key, value);
        }
        (passed, failed)
    }

    pub fn keys(&self) -> Collection<ArrayKey> {
        self.items.keys().cloned().collect()
    }

    pub fn values(self) -> Self {
        self.items.into_values().collect()
    }

    pub fn only<K: Into<ArrayKey>>(self, keys: impl IntoIterator<Item = K>) -> Self {
        let keys: HashSet<_> = keys
            .into_iter()
            .map(|key| key.into().normalized())
            .collect();
        self.filter(|_, key| keys.contains(key))
    }

    pub fn except<K: Into<ArrayKey>>(self, keys: impl IntoIterator<Item = K>) -> Self {
        let keys: HashSet<_> = keys
            .into_iter()
            .map(|key| key.into().normalized())
            .collect();
        self.reject(|_, key| keys.contains(key))
    }

    pub fn slice(self, offset: isize, length: Option<isize>) -> Self {
        let count = self.count();
        let start = if offset >= 0 {
            offset.unsigned_abs().min(count)
        } else {
            count.saturating_sub(offset.unsigned_abs())
        };
        let length = match length {
            Some(length) if length < 0 => count
                .saturating_sub(start)
                .saturating_sub(length.unsigned_abs()),
            Some(length) => length.unsigned_abs(),
            None => count - start,
        };
        Self::fromEntries(self.into_iter().skip(start).take(length))
    }

    pub fn take(self, limit: isize) -> Self {
        if limit < 0 {
            self.slice(limit, None)
        } else {
            self.slice(0, Some(limit))
        }
    }

    pub fn skip(self, count: usize) -> Self {
        Self::fromEntries(self.into_iter().skip(count))
    }

    pub fn takeWhile(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Self {
        Self::fromEntries(
            self.into_iter()
                .take_while(|(key, value)| predicate(value, key)),
        )
    }

    pub fn takeUntil(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Self {
        self.takeWhile(|value, key| !predicate(value, key))
    }

    pub fn skipWhile(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Self {
        Self::fromEntries(
            self.into_iter()
                .skip_while(|(key, value)| predicate(value, key)),
        )
    }

    pub fn skipUntil(self, mut predicate: impl FnMut(&T, &ArrayKey) -> bool) -> Self {
        self.skipWhile(|value, key| !predicate(value, key))
    }

    pub fn reverse(self) -> Self {
        Self::fromEntries(self.into_iter().rev())
    }

    pub fn chunk(self, size: usize) -> Collection<Self> {
        if size == 0 {
            return Collection::new();
        }
        let mut entries = self.into_iter().peekable();
        std::iter::from_fn(|| {
            entries.peek()?;
            Some(Self::fromEntries(entries.by_ref().take(size)))
        })
        .collect()
    }

    pub fn sliding(&self, size: usize, step: usize) -> CollectionResult<Collection<Self>>
    where
        T: Clone,
    {
        if size == 0 || step == 0 {
            return Err(CollectionError::InvalidArgument);
        }
        let Some(end) = self.count().checked_sub(size) else {
            return Ok(Collection::new());
        };
        Ok((0..=end)
            .step_by(step)
            .map(|offset| {
                Self::fromEntries(
                    self.items
                        .get_range(offset..offset + size)
                        .expect("window within collection")
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                )
            })
            .collect())
    }

    pub fn concat(mut self, values: impl IntoIterator<Item = T>) -> CollectionResult<Self> {
        for value in values {
            self.push(value)?;
        }
        Ok(self)
    }

    pub fn merge(self, other: Self) -> Self {
        let mut merged = Self::new();
        for (key, value) in self.into_iter().chain(other) {
            match key {
                ArrayKey::Integer(_) => {
                    merged
                        .push(value)
                        .expect("renumbered keys fit in the integer key space");
                }
                key => {
                    merged.put(key, value);
                }
            }
        }
        merged
    }

    pub fn replace(mut self, other: Self) -> Self {
        for (key, value) in other {
            self.put(key, value);
        }
        self
    }

    pub fn union(mut self, other: Self) -> Self {
        for (key, value) in other {
            if !self.items.contains_key(&key) {
                self.put(key, value);
            }
        }
        self
    }

    pub fn unique(self) -> Self
    where
        T: Eq + Hash + Clone,
    {
        self.uniqueBy(|value, _| value.clone())
    }

    pub fn uniqueBy<K: Eq + Hash>(self, mut callback: impl FnMut(&T, &ArrayKey) -> K) -> Self {
        let mut seen = HashSet::new();
        self.filter(|value, key| seen.insert(callback(value, key)))
    }

    pub fn sort(self) -> Self
    where
        T: Ord,
    {
        self.sortUsing(T::cmp)
    }

    pub fn sortDesc(self) -> Self
    where
        T: Ord,
    {
        self.sortUsing(|left, right| right.cmp(left))
    }

    pub fn sortUsing(mut self, mut compare: impl FnMut(&T, &T) -> Ordering) -> Self {
        self.items.sort_by(|_, left, _, right| compare(left, right));
        self
    }

    pub fn sortBy<K: Ord>(self, callback: impl FnMut(&T, &ArrayKey) -> K) -> Self {
        self.sort_by_key(callback, false)
    }

    pub fn sortByDesc<K: Ord>(self, callback: impl FnMut(&T, &ArrayKey) -> K) -> Self {
        self.sort_by_key(callback, true)
    }

    fn sort_by_key<K: Ord>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> K,
        descending: bool,
    ) -> Self {
        let mut items: Vec<_> = self
            .into_iter()
            .map(|(key, value)| (callback(&value, &key), key, value))
            .collect();
        items.sort_by(|left, right| {
            if descending {
                right.0.cmp(&left.0)
            } else {
                left.0.cmp(&right.0)
            }
        });
        Self::fromEntries(items.into_iter().map(|(_, key, value)| (key, value)))
    }

    pub fn sortKeys(mut self) -> Self {
        self.items.sort_keys();
        self
    }

    pub fn sortKeysDesc(mut self) -> Self {
        self.items.sort_by(|left, _, right, _| right.cmp(left));
        self
    }

    pub fn sortKeysUsing(
        mut self,
        mut compare: impl FnMut(&ArrayKey, &ArrayKey) -> Ordering,
    ) -> Self {
        self.items.sort_by(|left, _, right, _| compare(left, right));
        self
    }
}

fn insert_group<T>(
    groups: &mut Collection<Collection<T>>,
    group: ArrayKey,
    key: ArrayKey,
    value: T,
    preserve_keys: bool,
) {
    if !groups.items.contains_key(&group) {
        groups.put(group.clone(), Collection::new());
    }
    let group = groups.items.get_mut(&group).expect("group exists");
    if preserve_keys {
        group.put(key, value);
    } else {
        group
            .push(value)
            .expect("group keys fit in the integer key space");
    }
}

impl<T> Collection<Collection<T>> {
    pub fn collapse(self) -> Collection<T> {
        let mut result = Collection::new();
        for (_, values) in self {
            for (key, value) in values {
                match key {
                    ArrayKey::Integer(_) => {
                        result
                            .push(value)
                            .expect("renumbered keys fit in the integer key space");
                    }
                    key => {
                        result.put(key, value);
                    }
                }
            }
        }
        result
    }
}
