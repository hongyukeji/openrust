use super::ServiceKey;
use crate::contracts::container::{BindingResolutionError, Container};
use std::{
    marker::PhantomData,
    sync::{Arc, OnceLock},
};

/// A lazy service sequence. Each iteration resolves the current tagged identities anew.
pub struct Tagged<'a, C: Container, T: ?Sized> {
    container: &'a C,
    keys: Box<dyn Fn() -> Vec<ServiceKey> + Send + Sync + 'a>,
    count: OnceLock<usize>,
    service: PhantomData<fn() -> T>,
}

impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> Tagged<'a, C, T> {
    pub(crate) fn new(
        container: &'a C,
        keys: impl Fn() -> Vec<ServiceKey> + Send + Sync + 'a,
    ) -> Self {
        Self {
            container,
            keys: Box::new(keys),
            count: OnceLock::new(),
            service: PhantomData,
        }
    }

    /// Counts identities without constructing services, caching the first count like Laravel.
    pub fn len(&self) -> usize {
        *self.count.get_or_init(|| (self.keys)().len())
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn iter(&self) -> TaggedIter<'a, C, T> {
        TaggedIter {
            container: self.container,
            keys: (self.keys)().into_iter(),
            service: PhantomData,
        }
    }
}

/// Resolves each identity only when it is requested by the iterator.
pub struct TaggedIter<'a, C: Container, T: ?Sized> {
    container: &'a C,
    keys: std::vec::IntoIter<ServiceKey>,
    service: PhantomData<fn() -> T>,
}
impl<C: Container, T: ?Sized + Send + Sync + 'static> Iterator for TaggedIter<'_, C, T> {
    type Item = Result<Arc<T>, BindingResolutionError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.keys.next().map(|key| self.container.get(key))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.keys.size_hint()
    }
}
impl<C: Container, T: ?Sized + Send + Sync + 'static> ExactSizeIterator for TaggedIter<'_, C, T> {}
impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> IntoIterator for Tagged<'a, C, T> {
    type Item = Result<Arc<T>, BindingResolutionError>;
    type IntoIter = TaggedIter<'a, C, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> IntoIterator for &Tagged<'a, C, T> {
    type Item = Result<Arc<T>, BindingResolutionError>;
    type IntoIter = TaggedIter<'a, C, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
