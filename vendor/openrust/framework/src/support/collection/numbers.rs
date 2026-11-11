use std::iter::Sum;

use super::{ArrayKey, Collection};

/// A numeric item that can participate in floating-point aggregates.
pub trait CollectionNumber {
    fn as_f64(&self) -> f64;
}

macro_rules! numbers {
    ($($number:ty),+ $(,)?) => {
        $(
            impl CollectionNumber for $number {
                fn as_f64(&self) -> f64 { *self as f64 }
            }
        )+
    };
}

numbers!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);

#[allow(non_snake_case, reason = "Matches Laravel's Collection API")]
impl<T> Collection<T> {
    pub fn sum(&self) -> T
    where
        for<'a> T: Sum<&'a T>,
    {
        self.items.values().sum()
    }

    pub fn sumBy<U: Sum<U>>(&self, mut project: impl FnMut(&T, &ArrayKey) -> U) -> U {
        self.iter().map(|(key, value)| project(value, key)).sum()
    }

    pub fn avg(&self) -> Option<f64>
    where
        T: CollectionNumber,
    {
        self.avgBy(|value, _| value.as_f64())
    }

    pub fn average(&self) -> Option<f64>
    where
        T: CollectionNumber,
    {
        self.avg()
    }

    pub fn avgBy<U: Into<Option<f64>>>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> Option<f64> {
        let (sum, count) = self
            .iter()
            .filter_map(|(key, value)| project(value, key).into())
            .fold((0.0, 0usize), |(sum, count), value| {
                (sum + value, count + 1)
            });
        (count > 0).then(|| sum / count as f64)
    }

    pub fn median(&self) -> Option<f64>
    where
        T: CollectionNumber,
    {
        self.medianBy(|value, _| value.as_f64())
    }

    pub fn medianBy<U: Into<Option<f64>>>(
        &self,
        mut project: impl FnMut(&T, &ArrayKey) -> U,
    ) -> Option<f64> {
        let mut values: Vec<_> = self
            .iter()
            .filter_map(|(key, value)| project(value, key).into())
            .collect();
        values.sort_by(f64::total_cmp);
        let middle = values.len() / 2;
        let value = *values.get(middle)?;
        Some(if values.len().is_multiple_of(2) {
            values[middle - 1].midpoint(value)
        } else {
            value
        })
    }

    pub fn min(&self) -> Option<&T>
    where
        T: Ord,
    {
        self.items.values().min()
    }

    pub fn max(&self) -> Option<&T>
    where
        T: Ord,
    {
        self.items.values().max()
    }

    pub fn minBy<U: Ord>(&self, mut project: impl FnMut(&T, &ArrayKey) -> U) -> Option<U> {
        self.iter().map(|(key, value)| project(value, key)).min()
    }

    pub fn maxBy<U: Ord>(&self, mut project: impl FnMut(&T, &ArrayKey) -> U) -> Option<U> {
        self.iter().map(|(key, value)| project(value, key)).max()
    }
}
