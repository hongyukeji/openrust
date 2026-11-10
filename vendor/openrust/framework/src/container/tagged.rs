use super::ServiceKey;
use crate::contracts::container::{BindingResolutionError, Container};
use std::{marker::PhantomData, sync::Arc};

/// A lazy service sequence. Each iteration resolves the current tagged identities anew.
pub struct Tagged<'a, C: Container, T: ?Sized> {
    container: &'a C,
    keys: Box<dyn Fn() -> Vec<ServiceKey> + Send + Sync + 'a>,
    count: usize,
    service: PhantomData<fn() -> T>,
}

impl<'a, C: Container, T: ?Sized + Send + Sync + 'static> Tagged<'a, C, T> {
    pub(crate) fn new(
        container: &'a C,
        count: usize,
        keys: impl Fn() -> Vec<ServiceKey> + Send + Sync + 'a,
    ) -> Self {
        Self {
            container,
            keys: Box::new(keys),
            count,
            service: PhantomData,
        }
    }

    /// Returns the identity count captured when the sequence was created.
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn iter(&self) -> TaggedIter<'a, C, T> {
        TaggedIter {
            container: self.container,
            keys: (self.keys)().into_iter(),
            pending: None,
            service: PhantomData,
        }
    }
}

/// Resolves each identity only when it is requested by the iterator.
pub struct TaggedIter<'a, C: Container, T: ?Sized> {
    container: &'a C,
    keys: std::vec::IntoIter<ServiceKey>,
    pending: Option<ServiceKey>,
    service: PhantomData<fn() -> T>,
}
impl<C: Container, T: ?Sized + Send + Sync + 'static> Iterator for TaggedIter<'_, C, T> {
    type Item = Result<Arc<T>, BindingResolutionError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.pending
            .take()
            .or_else(|| self.keys.next())
            .map(|key| self.container.get(key))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.keys.len() + usize::from(self.pending.is_some());
        (remaining, Some(remaining))
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

impl<C: Container, T: ?Sized + Send + Sync + 'static> Tagged<'_, C, T> {
    #[allow(non_snake_case, reason = "Async lazy tagged resolution")]
    pub async fn collectAsync(&self) -> Result<Vec<Arc<T>>, BindingResolutionError> {
        let mut values = Vec::new();
        let mut iter = self.iter();
        while let Some(value) = iter.nextAsync().await {
            values.push(value?);
        }
        Ok(values)
    }
}
impl<C: Container, T: ?Sized + Send + Sync + 'static> TaggedIter<'_, C, T> {
    #[allow(non_snake_case, reason = "Async lazy tagged resolution")]
    pub async fn nextAsync(&mut self) -> Option<Result<Arc<T>, BindingResolutionError>> {
        if self.pending.is_none() {
            self.pending = self.keys.next();
        }
        let key = self.pending.clone()?;
        let result = self.container.getAsync(key).await;
        self.pending = None;
        Some(result)
    }
}
