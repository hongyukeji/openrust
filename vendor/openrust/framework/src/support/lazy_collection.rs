use std::{cell::RefCell, collections::VecDeque, iter, rc::Rc, sync::Arc};

use super::{ArrayKey, Collection, CollectionError, CollectionResult, Enumerable};

mod timing;

type Entry<T> = (ArrayKey, T);
type SourceIterator<'source, T> = Box<dyn Iterator<Item = CollectionResult<Entry<T>>> + 'source>;

/// Produces an iterator when traversal starts.
pub type LazySource<'source, T> = dyn Fn() -> SourceIterator<'source, T> + 'source;

/// A keyed collection that evaluates its source on demand.
pub struct LazyCollection<'source, T> {
    source: Rc<LazySource<'source, T>>,
}

impl<T> Clone for LazyCollection<'_, T> {
    fn clone(&self) -> Self {
        Self {
            source: Rc::clone(&self.source),
        }
    }
}

impl<'source, T: 'source> Default for LazyCollection<'source, T> {
    fn default() -> Self {
        Self::empty()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn make<I>(source: impl Fn() -> I + 'source) -> Self
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: 'source,
    {
        Self::tryMake(move || source().into_iter().map(Ok))
    }

    pub fn tryMake<I>(source: impl Fn() -> I + 'source) -> Self
    where
        I: IntoIterator<Item = CollectionResult<T>>,
        I::IntoIter: 'source,
    {
        Self::from_source(move || index_values(source().into_iter()))
    }

    pub fn fromEntries<K, I>(source: impl Fn() -> I + 'source) -> Self
    where
        K: Into<ArrayKey> + 'source,
        I: IntoIterator<Item = (K, T)>,
        I::IntoIter: 'source,
    {
        Self::tryFromEntries(move || source().into_iter().map(Ok))
    }

    pub fn tryFromEntries<K, I>(source: impl Fn() -> I + 'source) -> Self
    where
        K: Into<ArrayKey> + 'source,
        I: IntoIterator<Item = CollectionResult<(K, T)>>,
        I::IntoIter: 'source,
    {
        Self::from_source(move || {
            source()
                .into_iter()
                .map(|entry| entry.map(|(key, value)| (key.into().normalized(), value)))
        })
    }

    pub fn empty() -> Self {
        Self::from_iterators(iter::empty)
    }

    fn from_iterators<I>(source: impl Fn() -> I + 'source) -> Self
    where
        I: Iterator<Item = CollectionResult<Entry<T>>> + 'source,
    {
        let source = Rc::new(source);
        Self::from_source(move || {
            let source = Rc::clone(&source);
            let mut iterator = None;
            iter::from_fn(move || iterator.get_or_insert_with(|| source()).next())
        })
    }

    fn from_source<I>(source: impl Fn() -> I + 'source) -> Self
    where
        I: Iterator<Item = CollectionResult<Entry<T>>> + 'source,
    {
        Self {
            source: Rc::new(move || Box::new(source())),
        }
    }

    pub fn getIterator(&self) -> LazyIterator<'source, T> {
        self.into_iter()
    }

    pub fn all(&self) -> CollectionResult<Vec<Entry<T>>> {
        Ok(self.collect()?.into_iter().collect())
    }

    pub fn collect(&self) -> CollectionResult<Collection<T>> {
        let mut output = Collection::new();
        for entry in self.getIterator() {
            let (key, value) = entry?;
            output.put(key, value);
        }
        Ok(output)
    }

    pub fn eager(&self) -> CollectionResult<Self>
    where
        T: Clone,
    {
        Ok(self.collect()?.lazy())
    }

    pub fn remember(self) -> Self
    where
        T: Clone,
    {
        let state = Rc::new(RefCell::new(Remembered {
            iterator: Some(self.getIterator()),
            entries: Vec::new(),
            error: None,
        }));
        Self::from_iterators(move || {
            let state = Rc::clone(&state);
            let mut index = 0;
            iter::from_fn(move || {
                let entry = state.borrow_mut().entry(index)?;
                index += 1;
                Some(entry)
            })
        })
    }

    pub fn map<U: 'source>(
        self,
        callback: impl Fn(&T, &ArrayKey) -> U + 'source,
    ) -> LazyCollection<'source, U> {
        self.tryMap(move |value, key| Ok(callback(value, key)))
    }

    pub fn tryMap<U: 'source>(
        self,
        callback: impl Fn(&T, &ArrayKey) -> CollectionResult<U> + 'source,
    ) -> LazyCollection<'source, U> {
        let callback = Rc::new(callback);
        LazyCollection::from_iterators(move || {
            let callback = Rc::clone(&callback);
            self.getIterator().map(move |entry| {
                let (key, value) = entry?;
                let mapped = callback(&value, &key)?;
                Ok((key, mapped))
            })
        })
    }

    pub fn filter(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        let predicate = Rc::new(predicate);
        Self::from_iterators(move || {
            let predicate = Rc::clone(&predicate);
            self.getIterator().filter(move |entry| {
                entry
                    .as_ref()
                    .map_or(true, |(key, value)| predicate(value, key))
            })
        })
    }

    pub fn reject(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        self.filter(move |value, key| !predicate(value, key))
    }

    pub fn tapEach(self, callback: impl Fn(&T, &ArrayKey) + 'source) -> Self {
        let callback = Rc::new(callback);
        Self::from_iterators(move || {
            let callback = Rc::clone(&callback);
            self.getIterator().inspect(move |entry| {
                if let Ok((key, value)) = entry {
                    callback(value, key);
                }
            })
        })
    }

    pub fn values(self) -> Self {
        Self::from_iterators(move || {
            index_values(
                self.getIterator()
                    .map(|entry| entry.map(|(_, value)| value)),
            )
        })
    }

    pub fn take(self, limit: isize) -> Self {
        if let Ok(limit) = usize::try_from(limit) {
            return Self::from_iterators(move || self.getIterator().take(limit));
        }

        let limit = limit.unsigned_abs();
        Self::from_iterators(move || {
            let mut iterator = Some(self.getIterator());
            let mut tail = VecDeque::new();
            iter::from_fn(move || {
                if let Some(iterator) = iterator.take() {
                    for entry in iterator {
                        match entry {
                            Ok(entry) => {
                                if tail.len() == limit {
                                    tail.pop_front();
                                }
                                tail.push_back(entry);
                            }
                            Err(error) => return Some(Err(error)),
                        }
                    }
                }
                tail.pop_front().map(Ok)
            })
        })
    }

    pub fn skip(self, count: usize) -> Self {
        Self::from_iterators(move || {
            let mut remaining = count;
            self.getIterator().filter(move |entry| {
                if entry.is_err() || remaining == 0 {
                    return true;
                }
                remaining -= 1;
                false
            })
        })
    }

    pub fn takeWhile(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        let predicate = Rc::new(predicate);
        Self::from_iterators(move || {
            let predicate = Rc::clone(&predicate);
            self.getIterator().take_while(move |entry| {
                entry
                    .as_ref()
                    .map_or(true, |(key, value)| predicate(value, key))
            })
        })
    }

    pub fn takeUntil(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        self.takeWhile(move |value, key| !predicate(value, key))
    }

    pub fn skipWhile(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        let predicate = Rc::new(predicate);
        Self::from_iterators(move || {
            let predicate = Rc::clone(&predicate);
            self.getIterator().skip_while(move |entry| {
                entry
                    .as_ref()
                    .is_ok_and(|(key, value)| predicate(value, key))
            })
        })
    }

    pub fn skipUntil(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        self.skipWhile(move |value, key| !predicate(value, key))
    }

    pub fn chunk(self, size: usize, preserve_keys: bool) -> LazyCollection<'source, Collection<T>> {
        if size == 0 {
            return LazyCollection::empty();
        }
        LazyCollection::from_iterators(move || {
            let mut iterator = self.getIterator();
            index_values(iter::from_fn(move || {
                let mut chunk = Collection::new();
                while chunk.count() < size {
                    match iterator.next() {
                        Some(Ok((key, value))) if preserve_keys => {
                            chunk.put(key, value);
                        }
                        Some(Ok((_, value))) => {
                            if let Err(error) = chunk.push(value) {
                                return Some(Err(error));
                            }
                        }
                        Some(Err(error)) => return Some(Err(error)),
                        None => break,
                    }
                }
                (!chunk.isEmpty()).then_some(Ok(chunk))
            }))
        })
    }

    pub fn first(&self) -> CollectionResult<Option<T>> {
        Ok(self
            .getIterator()
            .next()
            .transpose()?
            .map(|(_, value)| value))
    }

    pub fn firstWhere(
        &self,
        mut predicate: impl FnMut(&T, &ArrayKey) -> bool,
    ) -> CollectionResult<Option<T>> {
        for entry in self.getIterator() {
            let (key, value) = entry?;
            if predicate(&value, &key) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    pub fn count(&self) -> CollectionResult<usize> {
        self.getIterator()
            .try_fold(0, |count, entry| entry.map(|_| count + 1))
    }

    pub fn each(&self, mut callback: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<&Self> {
        for entry in self.getIterator() {
            let (key, value) = entry?;
            if !callback(&value, &key) {
                break;
            }
        }
        Ok(self)
    }
}

/// A traversal of a lazy collection that stops at its first source error.
pub struct LazyIterator<'source, T> {
    iterator: Option<SourceIterator<'source, T>>,
}

impl<T> Iterator for LazyIterator<'_, T> {
    type Item = CollectionResult<Entry<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.iterator.as_mut()?.next();
        if !matches!(entry, Some(Ok(_))) {
            self.iterator = None;
        }
        entry
    }
}

impl<T> iter::FusedIterator for LazyIterator<'_, T> {}

impl<'source, T> IntoIterator for &LazyCollection<'source, T> {
    type Item = CollectionResult<Entry<T>>;
    type IntoIter = LazyIterator<'source, T>;

    fn into_iter(self) -> Self::IntoIter {
        LazyIterator {
            iterator: Some((self.source)()),
        }
    }
}

impl<T: Clone> Collection<T> {
    pub fn lazy<'source>(self) -> LazyCollection<'source, T>
    where
        T: 'source,
    {
        let collection = Rc::new(self);
        LazyCollection::from_iterators(move || {
            let collection = Rc::clone(&collection);
            let mut index = 0;
            iter::from_fn(move || {
                let (key, value) = Collection::all(collection.as_ref()).get_index(index)?;
                index += 1;
                Some(Ok((key.clone(), value.clone())))
            })
        })
    }
}

impl<'source, T: 'source> Enumerable<'source> for LazyCollection<'source, T> {
    type Item = T;
    type Rebind<U: 'source> = LazyCollection<'source, U>;

    fn all(self) -> CollectionResult<Vec<Entry<T>>> {
        LazyCollection::all(&self)
    }

    fn collect(self) -> CollectionResult<Collection<T>> {
        LazyCollection::collect(&self)
    }

    fn map<U: 'source>(self, callback: impl Fn(&T, &ArrayKey) -> U + 'source) -> Self::Rebind<U> {
        self.map(callback)
    }

    fn filter(self, predicate: impl Fn(&T, &ArrayKey) -> bool + 'source) -> Self {
        self.filter(predicate)
    }

    fn values(self) -> Self {
        self.values()
    }

    fn take(self, limit: isize) -> Self {
        self.take(limit)
    }
}

fn index_values<T>(
    iterator: impl Iterator<Item = CollectionResult<T>>,
) -> impl Iterator<Item = CollectionResult<Entry<T>>> {
    iterator.enumerate().map(|(index, value)| {
        let key = i64::try_from(index).map_err(|_| CollectionError::IndexOverflow)?;
        Ok((key.into(), value?))
    })
}

struct Remembered<T, I> {
    iterator: Option<I>,
    entries: Vec<Entry<T>>,
    error: Option<Arc<CollectionError>>,
}

impl<T: Clone, I: Iterator<Item = CollectionResult<Entry<T>>>> Remembered<T, I> {
    fn entry(&mut self, index: usize) -> Option<CollectionResult<Entry<T>>> {
        if let Some(entry) = self.entries.get(index) {
            return Some(Ok(entry.clone()));
        }
        if let Some(error) = &self.error {
            return Some(Err(CollectionError::Shared(Arc::clone(error))));
        }
        match self.iterator.as_mut()?.next() {
            Some(Ok(entry)) => {
                self.entries.push(entry.clone());
                Some(Ok(entry))
            }
            Some(Err(error)) => {
                self.iterator = None;
                let error = Arc::new(error);
                self.error = Some(Arc::clone(&error));
                Some(Err(CollectionError::Shared(error)))
            }
            None => {
                self.iterator = None;
                None
            }
        }
    }
}
