use std::{hash::Hash, iter::Sum};

use indexmap::IndexMap;

use super::{ArrayKey, CollectionResult, LazyCollection};
use crate::support::collection::{CollectionNumber, percentage_from_counts};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn reduce<U>(
        &self,
        initial: U,
        mut callback: impl FnMut(U, &T, &ArrayKey) -> U,
    ) -> CollectionResult<U> {
        self.get_iterator().try_fold(initial, |carry, entry| {
            let (key, value) = entry?;
            Ok(callback(carry, &value, &key))
        })
    }

    pub fn reduce_with_keys<U>(
        &self,
        initial: U,
        callback: impl FnMut(U, &T, &ArrayKey) -> U,
    ) -> CollectionResult<U> {
        self.reduce(initial, callback)
    }

    pub fn reduce_into<U>(
        &self,
        initial: U,
        mut callback: impl FnMut(&mut U, &T, &ArrayKey),
    ) -> CollectionResult<U> {
        self.reduce(initial, |mut carry, value, key| {
            callback(&mut carry, value, key);
            carry
        })
    }

    pub fn reduce_spread<A, B>(
        &self,
        initial: (A, B),
        mut callback: impl FnMut(A, B, &T, &ArrayKey) -> (A, B),
    ) -> CollectionResult<(A, B)> {
        self.reduce(initial, |(a, b), value, key| callback(a, b, value, key))
    }

    pub fn every(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<bool> {
        for entry in self {
            let (key, value) = entry?;
            if !predicate(&value, &key) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Returns the percentage of entries that match, in one traversal.
    pub fn percentage(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
        precision: u8,
    ) -> CollectionResult<Option<f64>> {
        let (matches, total) = self.reduce((0, 0), |(matches, total), value, key| {
            (matches + usize::from(predicate(value, key)), total + 1)
        })?;
        Ok(percentage_from_counts(matches, total, precision))
    }

    pub fn sum(&self) -> CollectionResult<T>
    where
        T: Sum<T>,
    {
        self.get_iterator()
            .map(|entry| entry.map(|(_, value)| value))
            .sum()
    }

    pub fn sum_by<U: Sum<U>>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> CollectionResult<U> {
        self.get_iterator()
            .map(|entry| entry.map(|(key, value)| project(&value, &key)))
            .sum()
    }

    pub fn avg(&self) -> CollectionResult<Option<f64>>
    where
        T: CollectionNumber,
    {
        self.avg_by(|value, _| value.as_f64())
    }

    pub fn average(&self) -> CollectionResult<Option<f64>>
    where
        T: CollectionNumber,
    {
        self.avg()
    }

    pub fn avg_by<U: Into<Option<f64>>>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> CollectionResult<Option<f64>> {
        let (sum, count) = self.reduce((0.0, 0usize), |(sum, count), value, key| {
            project(value, key)
                .into()
                .map_or((sum, count), |number| (sum + number, count + 1))
        })?;
        Ok((count > 0).then(|| sum / count as f64))
    }

    pub fn min(&self) -> CollectionResult<Option<T>>
    where
        T: Ord,
    {
        self.get_iterator().try_fold(None, |minimum, entry| {
            let (_, value) = entry?;
            Ok(Some(match minimum {
                Some(old) => std::cmp::min(old, value),
                None => value,
            }))
        })
    }

    pub fn max(&self) -> CollectionResult<Option<T>>
    where
        T: Ord,
    {
        self.get_iterator().try_fold(None, |maximum, entry| {
            let (_, value) = entry?;
            Ok(Some(match maximum {
                Some(old) => std::cmp::max(value, old),
                None => value,
            }))
        })
    }

    pub fn min_by<U: Ord>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> CollectionResult<Option<U>> {
        self.reduce(None, |minimum, value, key| {
            let value = project(value, key);
            Some(match minimum {
                Some(old) => std::cmp::min(old, value),
                None => value,
            })
        })
    }

    pub fn max_by<U: Ord>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> CollectionResult<Option<U>> {
        self.reduce(None, |maximum, value, key| {
            let value = project(value, key);
            Some(match maximum {
                Some(old) => std::cmp::max(value, old),
                None => value,
            })
        })
    }

    pub fn median(&self) -> CollectionResult<Option<f64>>
    where
        T: CollectionNumber,
    {
        Ok(Self::collect(self)?.median())
    }

    pub fn median_by<U: Into<Option<f64>>>(
        &self,
        project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> CollectionResult<Option<f64>> {
        Ok(Self::collect(self)?.median_by(project))
    }

    pub fn mode(&self) -> CollectionResult<Option<Vec<T>>>
    where
        T: Eq + Hash,
    {
        let mut counts = IndexMap::new();
        for (_, value) in Self::collect(self)? {
            *counts.entry(value).or_insert(0usize) += 1;
        }
        Ok(counts.values().copied().max().map(|max| {
            counts
                .into_iter()
                .filter_map(|(value, count)| (count == max).then_some(value))
                .collect()
        }))
    }

    pub fn count_by<K: Into<ArrayKey>>(
        self,
        project: impl Fn(&T, &ArrayKey) -> K + 'source,
    ) -> LazyCollection<'source, usize> {
        LazyCollection::from_iterators(move || -> super::SourceIterator<'source, usize> {
            let mut counts = IndexMap::new();
            for entry in self.get_iterator() {
                match entry {
                    Ok((key, value)) => {
                        *counts
                            .entry(project(&value, &key).into().normalized())
                            .or_insert(0) += 1;
                    }
                    Err(error) => return Box::new(std::iter::once(Err(error))),
                }
            }
            Box::new(counts.into_iter().map(Ok))
        })
    }
}
