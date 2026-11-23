use std::{collections::HashSet, iter, rc::Rc};

use super::{ArrayKey, Collection, LazyCollection};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn only<K: Into<ArrayKey>>(self, keys: impl IntoIterator<Item = K>) -> Self {
        let keys: HashSet<_> = keys
            .into_iter()
            .map(|key| key.into().normalized())
            .collect();
        Self::from_iterators(move || {
            let mut remaining = keys.clone();
            let mut source = self.get_iterator();
            let mut finished = false;
            iter::from_fn(move || {
                if finished {
                    return None;
                }
                for entry in source.by_ref() {
                    match entry {
                        Ok((key, value)) if remaining.remove(&key) => {
                            finished = remaining.is_empty();
                            return Some(Ok((key, value)));
                        }
                        Err(error) => return Some(Err(error)),
                        Ok(_) => {}
                    }
                }
                None
            })
        })
    }

    pub fn except<K: Into<ArrayKey>>(self, keys: impl IntoIterator<Item = K>) -> Self {
        let keys: Vec<_> = keys.into_iter().map(Into::into).collect();
        self.passthru(move |items| items.except(&keys))
    }

    pub fn key_by<K: Into<ArrayKey>>(self, project: impl Fn(&T, &ArrayKey) -> K + 'source) -> Self {
        let project = Rc::new(project);
        Self::from_iterators(move || {
            let project = Rc::clone(&project);
            self.get_iterator().map(move |entry| {
                entry.map(|(key, value)| (project(&value, &key).into().normalized(), value))
            })
        })
    }

    pub fn pluck<U: 'source>(
        self,
        project: impl Fn(&T) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        self.map(move |value, _| project(value)).values()
    }

    pub fn pluck_with_keys<K: Into<ArrayKey>, U: 'source>(
        self,
        key: impl Fn(&T) -> K + 'source,
        value: impl Fn(&T) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        let project = Rc::new(move |item: &T| (key(item).into().normalized(), value(item)));
        LazyCollection::from_iterators(move || {
            let project = Rc::clone(&project);
            self.get_iterator()
                .map(move |entry| entry.map(|(_, item)| project(&item)))
        })
    }

    pub fn map_with_keys<K, U, I>(
        self,
        callback: impl Fn(&T, &ArrayKey) -> I + 'source,
    ) -> LazyCollection<'source, U>
    where
        K: Into<ArrayKey> + 'source,
        U: 'source,
        I: IntoIterator<Item = (K, U)> + 'source,
        I::IntoIter: 'source,
    {
        let callback = Rc::new(callback);
        LazyCollection::from_iterators(move || {
            let callback = Rc::clone(&callback);
            let mut source = self.get_iterator();
            let mut projected: Option<I::IntoIter> = None;
            iter::from_fn(move || {
                loop {
                    if let Some((key, value)) = projected.as_mut().and_then(Iterator::next) {
                        return Some(Ok((key.into().normalized(), value)));
                    }
                    match source.next()? {
                        Ok((key, value)) => projected = Some(callback(&value, &key).into_iter()),
                        Err(error) => return Some(Err(error)),
                    }
                }
            })
        })
    }

    pub fn group_by<K: Into<ArrayKey>>(
        self,
        project: impl Fn(&T, &ArrayKey) -> K + 'source,
        preserve_keys: bool,
    ) -> LazyCollection<'source, Collection<T>> {
        self.passthru(move |items| items.group_by(&project, preserve_keys))
    }

    pub fn map_to_dictionary<K: Into<ArrayKey>, U: 'source>(
        self,
        project: impl Fn(&T, &ArrayKey) -> (K, U) + 'source,
    ) -> LazyCollection<'source, Vec<U>> {
        self.passthru(move |items| items.map_to_dictionary(&project))
    }

    pub fn map_to_groups<K: Into<ArrayKey>, U: 'source>(
        self,
        project: impl Fn(&T, &ArrayKey) -> (K, U) + 'source,
    ) -> LazyCollection<'source, Collection<U>> {
        self.passthru(move |items| items.map_to_groups(&project))
    }
}
