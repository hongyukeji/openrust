use std::{iter, rc::Rc};

use super::{ArrayKey, Collection, CollectionError, CollectionResult, LazyCollection};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn for_page(self, page: usize, per_page: usize) -> Self {
        let offset = page.saturating_sub(1).saturating_mul(per_page);
        let source = self.skip(offset);
        Self::from_iterators(move || source.get_iterator().take(per_page))
    }

    pub fn split(self, groups: usize) -> CollectionResult<LazyCollection<'source, Collection<T>>> {
        if groups == 0 {
            return Err(CollectionError::InvalidArgument);
        }
        Ok(self.try_passthru(move |items| items.split(groups)))
    }

    pub fn split_in(
        self,
        groups: usize,
    ) -> CollectionResult<LazyCollection<'source, Collection<T>>> {
        if groups == 0 {
            return Err(CollectionError::InvalidArgument);
        }
        let size = self.count()?.div_ceil(groups);
        Ok(self.chunk(size, true))
    }

    pub fn partition(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<(Collection<T>, Collection<T>)> {
        let mut passed = Collection::new();
        let mut failed = Collection::new();
        for entry in self {
            let (key, value) = entry?;
            let target = if predicate(&value, &key) {
                &mut passed
            } else {
                &mut failed
            };
            target.put(key, value);
        }
        Ok((passed, failed))
    }

    pub fn pad(self, size: isize, value: T) -> Self
    where
        T: Clone,
    {
        if size < 0 {
            return self.try_passthru(move |items| items.pad_with(size, || value.clone()));
        }
        let value = Rc::new(value);
        Self::from_iterators(move || {
            let value = Rc::clone(&value);
            let mut source = self.get_iterator();
            let mut remaining = size.unsigned_abs();
            let mut next_key = Some(0i64);
            iter::from_fn(move || {
                if let Some(entry) = source.next() {
                    return Some(entry.inspect(|(key, _)| {
                        remaining = remaining.saturating_sub(1);
                        if let ArrayKey::Integer(index) = key
                            && next_key.is_some_and(|next| *index >= next)
                        {
                            next_key = index.checked_add(1);
                        }
                    }));
                }
                remaining = remaining.checked_sub(1)?;
                let Some(key) = next_key else {
                    return Some(Err(CollectionError::IndexOverflow));
                };
                next_key = key.checked_add(1);
                Some(Ok((key.into(), value.as_ref().clone())))
            })
        })
    }
}
