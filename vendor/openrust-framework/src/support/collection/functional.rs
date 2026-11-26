use std::{fmt::Display, num::NonZeroUsize, ops::RangeInclusive};

use super::{ArrayKey, Collection, CollectionError, CollectionResult};
use crate::support::Arr;

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

    pub fn map_into<U: From<T>>(self) -> Collection<U> {
        Collection::from_entries(self.into_iter().map(|(key, item)| (key, item.into())))
    }

    pub fn flat_map<U>(
        self,
        callback: impl FnMut(&T, &ArrayKey) -> Collection<U>,
    ) -> Collection<U> {
        self.map(callback).collapse()
    }

    pub fn cross_join(self, others: &[&[T]]) -> Collection<Vec<T>>
    where
        T: Clone,
    {
        let owned: Vec<_> = self.items.into_values().collect();
        let sets: Vec<_> = std::iter::once(owned.as_slice())
            .chain(others.iter().copied())
            .collect();
        Collection::make(Arr::cross_join(&sets))
    }

    pub fn map_to_groups<K: Into<ArrayKey>, U>(
        self,
        callback: impl FnMut(&T, &ArrayKey) -> (K, U),
    ) -> Collection<Collection<U>> {
        self.map_to_dictionary(callback).map_into()
    }

    pub fn map_to_dictionary<K: Into<ArrayKey>, U>(
        self,
        mut callback: impl FnMut(&T, &ArrayKey) -> (K, U),
    ) -> Collection<Vec<U>> {
        let mut groups = Collection::<Vec<U>>::new();
        for (key, item) in self {
            let (group, value) = callback(&item, &key);
            groups.get_or_put(group, Vec::new).push(value);
        }
        groups
    }

    pub fn get_or_put(&mut self, key: impl Into<ArrayKey>, default: impl FnOnce() -> T) -> &mut T {
        let key = key.into().normalized();
        if !self.items.contains_key(&key) {
            self.put(&key, default());
        }
        self.items.get_mut(&key).expect("existing or inserted key")
    }

    pub fn add(&mut self, item: T) -> CollectionResult<&mut Self> {
        self.push(item)
    }

    pub fn contains_one_item(&self) -> bool {
        self.count() == 1
    }

    pub fn contains_many_items(&self) -> bool {
        self.count() > 1
    }

    pub fn to_base(self) -> Self {
        self
    }

    pub fn reduce_with_keys<U>(
        &self,
        initial: U,
        callback: impl FnMut(U, &T, &ArrayKey) -> U,
    ) -> U {
        self.reduce(initial, callback)
    }

    pub fn reduce_into<U>(
        &self,
        mut initial: U,
        mut callback: impl FnMut(&mut U, &T, &ArrayKey),
    ) -> U {
        for (key, item) in self.iter() {
            callback(&mut initial, item, key);
        }
        initial
    }

    pub fn reduce_spread<A, B>(
        &self,
        initial: (A, B),
        mut callback: impl FnMut(A, B, &T, &ArrayKey) -> (A, B),
    ) -> (A, B) {
        self.reduce(initial, |(a, b), item, key| callback(a, b, item, key))
    }

    pub fn pipe<U>(self, callback: impl FnOnce(Self) -> U) -> U {
        callback(self)
    }

    pub fn pipe_into<U: From<Self>>(self) -> U {
        self.into()
    }

    pub fn pipe_through<F: FnOnce(Self) -> Self>(
        self,
        stages: impl IntoIterator<Item = F>,
    ) -> Self {
        stages.into_iter().fold(self, |items, stage| stage(items))
    }

    pub fn tap(self, callback: impl FnOnce(&Self)) -> Self {
        callback(&self);
        self
    }

    pub fn tap_each(self, mut callback: impl FnMut(&T, &ArrayKey)) -> Self {
        for (key, value) in self.iter() {
            callback(value, key);
        }
        self
    }

    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }

    pub fn when_else(
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

    pub fn when_empty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_empty();
        self.when(condition, callback)
    }

    pub fn when_not_empty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_not_empty();
        self.when(condition, callback)
    }

    pub fn unless_empty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when_not_empty(callback)
    }

    pub fn unless_not_empty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when_empty(callback)
    }

    pub fn ensure(self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<Self> {
        self.every(predicate)
            .then_some(self)
            .ok_or(CollectionError::InvalidArgument)
    }

    pub fn r#where<U: PartialEq>(self, mut project: impl FnMut(&T) -> U, expected: &U) -> Self {
        self.filter(|item, _| project(item) == *expected)
    }

    pub fn where_strict<U: PartialEq>(self, project: impl FnMut(&T) -> U, expected: &U) -> Self {
        self.r#where(project, expected)
    }

    pub fn where_in<U: PartialEq>(self, mut project: impl FnMut(&T) -> U, expected: &[U]) -> Self {
        self.filter(|item, _| expected.contains(&project(item)))
    }

    pub fn where_not_in<U: PartialEq>(
        self,
        mut project: impl FnMut(&T) -> U,
        expected: &[U],
    ) -> Self {
        self.reject(|item, _| expected.contains(&project(item)))
    }

    pub fn where_in_strict<U: PartialEq>(
        self,
        project: impl FnMut(&T) -> U,
        expected: &[U],
    ) -> Self {
        self.where_in(project, expected)
    }

    pub fn where_not_in_strict<U: PartialEq>(
        self,
        project: impl FnMut(&T) -> U,
        expected: &[U],
    ) -> Self {
        self.where_not_in(project, expected)
    }

    pub fn where_between<U: PartialOrd>(
        self,
        mut project: impl FnMut(&T) -> U,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.filter(|item, _| bounds.contains(&project(item)))
    }

    pub fn where_not_between<U: PartialOrd>(
        self,
        mut project: impl FnMut(&T) -> U,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.reject(|item, _| bounds.contains(&project(item)))
    }

    pub fn rand_item(&self) -> CollectionResult<&T> {
        if self.is_empty() {
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
            Self::from_entries(items)
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
    pub fn implode_by<U: Display>(
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

impl<T> Collection<Vec<T>> {
    pub fn map_spread<U>(self, mut callback: impl FnMut(&[T], &ArrayKey) -> U) -> Collection<U> {
        self.map(|chunk, key| callback(chunk, key))
    }

    pub fn each_spread(&self, mut callback: impl FnMut(&[T], &ArrayKey) -> bool) -> &Self {
        self.each(|chunk, key| callback(chunk, key))
    }
}

impl<T> Collection<Option<T>> {
    pub fn where_null(self) -> Self {
        self.filter(|value, _| value.is_none())
    }

    pub fn where_not_null(self) -> Self {
        self.filter(|value, _| value.is_some())
    }
}

fn sample_index(len: usize) -> CollectionResult<usize> {
    crate::support::arr::random_index(len).map_err(Into::into)
}

impl Collection<Box<dyn std::any::Any>> {
    pub fn where_instance_of<T: 'static>(self) -> Collection<Box<T>> {
        Collection::from_entries(
            self.into_iter()
                .filter_map(|(key, item)| item.downcast::<T>().ok().map(|item| (key, item))),
        )
    }
}
