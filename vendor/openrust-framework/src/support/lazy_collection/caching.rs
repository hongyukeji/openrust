use std::iter::{FusedIterator, Peekable};

use super::{Collection, CollectionResult, Entry, LazyCollection, LazyIterator};

/// A single traversal with one entry of lookahead and optional full caching.
pub struct LazyCachingIterator<'source, T> {
    iterator: Peekable<LazyIterator<'source, T>>,
    cache: Option<FullCache<T>>,
}

struct FullCache<T> {
    entries: Collection<T>,
    copy: fn(&T) -> T,
}

impl<T> LazyCachingIterator<'_, T> {
    /// Indicates whether another result, including a pending error, is available.
    pub fn has_next(&mut self) -> bool {
        self.iterator.peek().is_some()
    }

    /// Returns the cache of successful entries already yielded, when enabled.
    pub fn get_cache(&self) -> Option<&Collection<T>> {
        self.cache.as_ref().map(|cache| &cache.entries)
    }
}

impl<T> Iterator for LazyCachingIterator<'_, T> {
    type Item = CollectionResult<Entry<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.iterator.next()?;
        if let Ok((key, value)) = &entry {
            if let Some(cache) = &mut self.cache {
                cache.entries.put(key, (cache.copy)(value));
            }
            let _ = self.iterator.peek();
        }
        Some(entry)
    }
}

impl<T> FusedIterator for LazyCachingIterator<'_, T> {}

impl<'source, T: 'source> LazyCollection<'source, T> {
    /// Creates a traversal with one entry of lookahead.
    pub fn get_caching_iterator(&self) -> LazyCachingIterator<'source, T> {
        LazyCachingIterator {
            iterator: self.get_iterator().peekable(),
            cache: None,
        }
    }

    /// Creates a traversal that also retains the last yielded value for each key.
    pub fn get_caching_iterator_with_full_cache(&self) -> LazyCachingIterator<'source, T>
    where
        T: Clone,
    {
        LazyCachingIterator {
            cache: Some(FullCache {
                entries: Collection::new(),
                copy: T::clone,
            }),
            ..self.get_caching_iterator()
        }
    }
}
