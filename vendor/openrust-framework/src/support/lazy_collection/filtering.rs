use std::{any::Any, ops::RangeInclusive};

use serde_json::Value;

use super::LazyCollection;
use crate::support::{arr::truthy, data_get};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn r#where<U: PartialEq + 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: U,
    ) -> Self {
        self.filter(move |item, _| project(item) == expected)
    }

    pub fn where_strict<U: PartialEq + 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: U,
    ) -> Self {
        self.r#where(project, expected)
    }

    pub fn where_in<U: PartialEq>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: &'source [U],
    ) -> Self {
        self.filter(move |item, _| expected.contains(&project(item)))
    }

    pub fn where_not_in<U: PartialEq>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: &'source [U],
    ) -> Self {
        self.reject(move |item, _| expected.contains(&project(item)))
    }

    pub fn where_in_strict<U: PartialEq>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: &'source [U],
    ) -> Self {
        self.where_in(project, expected)
    }

    pub fn where_not_in_strict<U: PartialEq>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: &'source [U],
    ) -> Self {
        self.where_not_in(project, expected)
    }

    pub fn where_between<U: PartialOrd + 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.filter(move |item, _| bounds.contains(&project(item)))
    }

    pub fn where_not_between<U: PartialOrd + 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.filter(move |item, _| {
            let value = project(item);
            value < *bounds.start() || value > *bounds.end()
        })
    }
}

impl<'source, T: PartialEq + 'source> LazyCollection<'source, T> {
    pub fn take_until_value(self, expected: T) -> Self {
        self.take_until(move |item, _| item == &expected)
    }

    pub fn take_while_value(self, expected: T) -> Self {
        self.take_while(move |item, _| item == &expected)
    }

    pub fn skip_until_value(self, expected: T) -> Self {
        self.skip_until(move |item, _| item == &expected)
    }

    pub fn skip_while_value(self, expected: T) -> Self {
        self.skip_while(move |item, _| item == &expected)
    }
}

impl<'source, T: 'source> LazyCollection<'source, Option<T>> {
    pub fn where_null(self) -> Self {
        self.filter(|value, _| value.is_none())
    }

    pub fn where_not_null(self) -> Self {
        self.filter(|value, _| value.is_some())
    }
}

impl<'source> LazyCollection<'source, Value> {
    /// Keeps values that are truthy using PHP's associative-array rules for JSON.
    pub fn filter_truthy(self) -> Self {
        self.filter(|value, _| truthy(value))
    }

    pub fn where_null(self) -> Self {
        self.filter(|value, _| value.is_null())
    }

    pub fn where_not_null(self) -> Self {
        self.reject(|value, _| value.is_null())
    }

    pub fn where_null_at(self, path: &'source str) -> Self {
        self.filter(move |value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }

    pub fn where_not_null_at(self, path: &'source str) -> Self {
        self.reject(move |value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }
}

impl<'source> LazyCollection<'source, Box<dyn Any>> {
    pub fn where_instance_of<T: 'static>(self) -> LazyCollection<'source, Box<T>> {
        LazyCollection::from_iterators(move || {
            self.get_iterator().filter_map(|entry| match entry {
                Ok((key, value)) => value.downcast().ok().map(|value| Ok((key, value))),
                Err(error) => Some(Err(error)),
            })
        })
    }
}
