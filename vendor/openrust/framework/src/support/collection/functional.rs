use std::{fmt::Display, num::NonZeroUsize, ops::RangeInclusive};

use super::{ArrayKey, Collection, CollectionError, CollectionResult};
use crate::support::Arr;

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T> Collection<T> {
    pub fn collect(self) -> Self {
        self
    }

    pub fn wrap(item: Option<T>) -> Self {
        Self::make(item)
    }

    pub fn unwrap(self) -> indexmap::IndexMap<ArrayKey, T> {
        self.items
    }

    pub fn times(count: usize, mut callback: impl FnMut(usize) -> T) -> Self {
        Self::make((1..=count).map(&mut callback))
    }

    pub fn transform(&mut self, mut callback: impl FnMut(&T, &ArrayKey) -> T) -> &mut Self {
        for (key, item) in &mut self.items {
            *item = callback(item, key);
        }
        self
    }

    pub fn mapInto<U: From<T>>(self) -> Collection<U> {
        Collection::fromEntries(self.into_iter().map(|(key, item)| (key, item.into())))
    }

    pub fn flatMap<U>(self, callback: impl FnMut(&T, &ArrayKey) -> Collection<U>) -> Collection<U> {
        self.map(callback).collapse()
    }

    pub fn crossJoin(self, others: &[&[T]]) -> Collection<Vec<T>>
    where
        T: Clone,
    {
        let owned: Vec<_> = self.items.into_values().collect();
        let sets: Vec<_> = std::iter::once(owned.as_slice())
            .chain(others.iter().copied())
            .collect();
        Collection::make(Arr::crossJoin(&sets))
    }

    pub fn mapToGroups<K: Into<ArrayKey>, U>(
        self,
        callback: impl FnMut(&T, &ArrayKey) -> (K, U),
    ) -> Collection<Collection<U>> {
        self.mapToDictionary(callback).mapInto()
    }

    pub fn mapToDictionary<K: Into<ArrayKey>, U>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> (K, U),
    ) -> Collection<Vec<U>> {
        let mut groups = Collection::<Vec<U>>::new();
        for (key, item) in self {
            let (group, value) = callback(&item, &key);
            groups.getOrPut(group, Vec::new).push(value);
        }
        groups
    }

    pub fn getOrPut(&mut self, key: impl Into<ArrayKey>, default: impl FnOnce() -> T) -> &mut T {
        let key = key.into().normalized();
        if !self.items.contains_key(&key) {
            self.put(&key, default());
        }
        self.items.get_mut(&key).expect("existing or inserted key")
    }

    pub fn add(&mut self, item: T) -> CollectionResult<&mut Self> {
        self.push(item)
    }

    pub fn containsOneItem(&self) -> bool {
        self.count() == 1
    }

    pub fn containsManyItems(&self) -> bool {
        self.count() > 1
    }

    pub fn toBase(self) -> Self {
        self
    }

    pub fn reduceWithKeys<U>(&self, initial: U, callback: impl FnMut(U, &T, &ArrayKey) -> U) -> U {
        self.reduce(initial, callback)
    }

    pub fn reduceInto<U>(
        &self,
        mut initial: U,
        mut callback: impl FnMut(&mut U, &T, &ArrayKey),
    ) -> U {
        for (key, item) in self.iter() {
            callback(&mut initial, item, key);
        }
        initial
    }

    pub fn reduceSpread<A, B>(
        &self,
        initial: (A, B),
        mut callback: impl FnMut(A, B, &T, &ArrayKey) -> (A, B),
    ) -> (A, B) {
        self.reduce(initial, |(a, b), item, key| callback(a, b, item, key))
    }

    pub fn pipe<U>(self, callback: impl FnOnce(Self) -> U) -> U {
        callback(self)
    }

    pub fn pipeInto<U: From<Self>>(self) -> U {
        self.into()
    }

    pub fn pipeThrough<F: FnOnce(Self) -> Self>(self, stages: impl IntoIterator<Item = F>) -> Self {
        stages.into_iter().fold(self, |items, stage| stage(items))
    }

    pub fn tap(self, callback: impl FnOnce(&Self)) -> Self {
        callback(&self);
        self
    }

    pub fn tapEach(self, mut callback: impl FnMut(&T, &ArrayKey)) -> Self {
        for (key, value) in self.iter() {
            callback(value, key);
        }
        self
    }

    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }

    pub fn whenElse(
        self,
        condition: bool,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> Self {
        if condition {
            callback(self)
        } else {
            otherwise(self)
        }
    }

    pub fn unless(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, callback)
    }

    pub fn whenEmpty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isEmpty();
        self.when(condition, callback)
    }

    pub fn whenNotEmpty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isNotEmpty();
        self.when(condition, callback)
    }

    pub fn unlessEmpty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        self.whenNotEmpty(callback)
    }

    pub fn unlessNotEmpty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        self.whenEmpty(callback)
    }

    pub fn ensure(self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<Self> {
        self.every(predicate)
            .then_some(self)
            .ok_or(CollectionError::InvalidArgument)
    }

    pub fn r#where<U: PartialEq>(self, mut project: impl FnMut(&T) -> U, expected: &U) -> Self {
        self.filter(|item, _| project(item) == *expected)
    }

    pub fn whereStrict<U: PartialEq>(self, project: impl FnMut(&T) -> U, expected: &U) -> Self {
        self.r#where(project, expected)
    }

    pub fn whereIn<U: PartialEq>(self, mut project: impl FnMut(&T) -> U, expected: &[U]) -> Self {
        self.filter(|item, _| expected.contains(&project(item)))
    }

    pub fn whereNotIn<U: PartialEq>(
        self,
        mut project: impl FnMut(&T) -> U,
        expected: &[U],
    ) -> Self {
        self.reject(|item, _| expected.contains(&project(item)))
    }

    pub fn whereInStrict<U: PartialEq>(self, project: impl FnMut(&T) -> U, expected: &[U]) -> Self {
        self.whereIn(project, expected)
    }

    pub fn whereNotInStrict<U: PartialEq>(
        self,
        project: impl FnMut(&T) -> U,
        expected: &[U],
    ) -> Self {
        self.whereNotIn(project, expected)
    }

    pub fn whereBetween<U: PartialOrd>(
        self,
        mut project: impl FnMut(&T) -> U,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.filter(|item, _| bounds.contains(&project(item)))
    }

    pub fn whereNotBetween<U: PartialOrd>(
        self,
        mut project: impl FnMut(&T) -> U,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.reject(|item, _| bounds.contains(&project(item)))
    }

    pub fn randItem(&self) -> CollectionResult<&T> {
        if self.isEmpty() {
            return Err(CollectionError::ItemNotFound);
        }
        let index = sample_index(self.count())?;
        Ok(self
            .items
            .get_index(index)
            .expect("random index in bounds")
            .1)
    }

    pub fn random(self, count: usize, preserve_keys: bool) -> CollectionResult<Self> {
        if count > self.count() {
            return Err(CollectionError::InvalidArgument);
        }
        let mut selected: Vec<_> = (0..self.count()).collect();
        for index in 0..count {
            let swap = index + sample_index(selected.len() - index)?;
            selected.swap(index, swap);
        }
        selected.truncate(count);
        selected.sort_unstable();
        let mut selected = selected.into_iter().peekable();
        let items = self
            .into_iter()
            .enumerate()
            .filter_map(|(index, entry)| selected.next_if_eq(&index).map(|_| entry));
        Ok(if preserve_keys {
            Self::fromEntries(items)
        } else {
            Self::make(items.map(|(_, value)| value))
        })
    }

    pub fn shuffle(self) -> CollectionResult<Self> {
        let mut items: Vec<_> = self.items.into_values().collect();
        for index in (1..items.len()).rev() {
            items.swap(index, sample_index(index + 1)?);
        }
        Ok(Self::make(items))
    }

    pub fn join(&self, glue: &str, final_glue: &str) -> String
    where
        T: Display,
    {
        let parts: Vec<_> = self.items.values().map(ToString::to_string).collect();
        Arr::join(&parts, glue, final_glue)
    }

    /// Projects each item and joins the displayed values in collection order.
    pub fn implodeBy<U: Display>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
        glue: &str,
    ) -> String {
        let parts: Vec<_> = self
            .iter()
            .map(|(key, value)| project(value, key).to_string())
            .collect();
        parts.join(glue)
    }

    pub fn implode(&self, glue: &str) -> String
    where
        T: Display,
    {
        self.join(glue, "")
    }
}

impl Collection<i64> {
    pub fn range(start: i64, end: i64, step: NonZeroUsize) -> Self {
        if start <= end {
            Self::make((start..=end).step_by(step.get()))
        } else {
            Self::make((end..=start).rev().step_by(step.get()))
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T> Collection<Vec<T>> {
    pub fn mapSpread<U>(self, mut callback: impl FnMut(&[T], &ArrayKey) -> U) -> Collection<U> {
        self.map(|chunk, key| callback(chunk, key))
    }

    pub fn eachSpread(&self, mut callback: impl FnMut(&[T], &ArrayKey) -> bool) -> &Self {
        self.each(|chunk, key| callback(chunk, key))
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T> Collection<Option<T>> {
    pub fn whereNull(self) -> Self {
        self.filter(|value, _| value.is_none())
    }

    pub fn whereNotNull(self) -> Self {
        self.filter(|value, _| value.is_some())
    }
}

fn sample_index(len: usize) -> CollectionResult<usize> {
    crate::support::arr::random_index(len).map_err(|error| CollectionError::Source(Box::new(error)))
}

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl Collection<Box<dyn std::any::Any>> {
    pub fn whereInstanceOf<T: 'static>(self) -> Collection<Box<T>> {
        Collection::fromEntries(
            self.into_iter()
                .filter_map(|(key, item)| item.downcast::<T>().ok().map(|item| (key, item))),
        )
    }
}
