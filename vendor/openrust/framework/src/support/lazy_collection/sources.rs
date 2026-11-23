use std::{iter, num::NonZeroUsize, rc::Rc};

use serde_json::Value;

use super::{Collection, CollectionResult, Entry, LazyCollection, index_values};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn times(count: usize, callback: impl Fn(usize) -> T + 'source) -> Self {
        let callback = Rc::new(callback);
        Self::from_iterators(move || {
            let callback = Rc::clone(&callback);
            index_values((1..=count).map(move |index| Ok(callback(index))))
        })
    }

    pub fn wrap(item: Option<T>) -> Self
    where
        T: Clone,
    {
        let Some(item) = item else {
            return Self::empty();
        };
        let item = Rc::new(item);
        Self::from_iterators(move || {
            let item = Rc::clone(&item);
            iter::once_with(move || Ok((0.into(), item.as_ref().clone())))
        })
    }

    pub fn unwrap(self) -> CollectionResult<Vec<Entry<T>>> {
        Self::all(&self)
    }
}

impl LazyCollection<'_, i64> {
    pub fn range(start: i64, end: i64, step: NonZeroUsize) -> Self {
        if start <= end {
            Self::make(move || (start..=end).step_by(step.get()))
        } else {
            Self::make(move || (end..=start).rev().step_by(step.get()))
        }
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl LazyCollection<'_, Value> {
    pub fn fromValue(value: Value) -> Self {
        Collection::fromValue(value).into()
    }
}
