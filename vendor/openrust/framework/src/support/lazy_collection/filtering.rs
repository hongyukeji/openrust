use std::{any::Any, ops::RangeInclusive};

use serde_json::Value;

use super::LazyCollection;
use crate::support::{arr::truthy, data_get};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn r#where<U: PartialEq + 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: U,
    ) -> Self {
        self.filter(move |item, _| project(item) == expected)
    }

    pub fn whereStrict<U: PartialEq + 'source>(
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

    pub fn whereInStrict<U: PartialEq>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: &'source [U],
    ) -> Self {
        self.where_in(project, expected)
    }

    pub fn whereNotInStrict<U: PartialEq>(
        self,
        project: impl Fn(&T) -> U + 'source,
        expected: &'source [U],
    ) -> Self {
        self.where_not_in(project, expected)
    }

    pub fn whereBetween<U: PartialOrd + 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
        bounds: RangeInclusive<U>,
    ) -> Self {
        self.filter(move |item, _| bounds.contains(&project(item)))
    }

    pub fn whereNotBetween<U: PartialOrd + 'source>(
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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source, T: PartialEq + 'source> LazyCollection<'source, T> {
    pub fn takeUntilValue(self, expected: T) -> Self {
        self.takeUntil(move |item, _| item == &expected)
    }

    pub fn takeWhileValue(self, expected: T) -> Self {
        self.takeWhile(move |item, _| item == &expected)
    }

    pub fn skipUntilValue(self, expected: T) -> Self {
        self.skipUntil(move |item, _| item == &expected)
    }

    pub fn skipWhileValue(self, expected: T) -> Self {
        self.skipWhile(move |item, _| item == &expected)
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source, T: 'source> LazyCollection<'source, Option<T>> {
    pub fn where_null(self) -> Self {
        self.filter(|value, _| value.is_none())
    }

    pub fn where_not_null(self) -> Self {
        self.filter(|value, _| value.is_some())
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source> LazyCollection<'source, Value> {
    /// Keeps values that are truthy using PHP's associative-array rules for JSON.
    pub fn filterTruthy(self) -> Self {
        self.filter(|value, _| truthy(value))
    }

    pub fn where_null(self) -> Self {
        self.filter(|value, _| value.is_null())
    }

    pub fn where_not_null(self) -> Self {
        self.reject(|value, _| value.is_null())
    }

    pub fn whereNullAt(self, path: &'source str) -> Self {
        self.filter(move |value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }

    pub fn whereNotNullAt(self, path: &'source str) -> Self {
        self.reject(move |value, _| data_get(value, Some(path)).is_none_or(|value| value.is_null()))
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'source> LazyCollection<'source, Box<dyn Any>> {
    pub fn whereInstanceOf<T: 'static>(self) -> LazyCollection<'source, Box<T>> {
        LazyCollection::from_iterators(move || {
            self.getIterator().filter_map(|entry| match entry {
                Ok((key, value)) => value.downcast().ok().map(|value| Ok((key, value))),
                Err(error) => Some(Err(error)),
            })
        })
    }
}
