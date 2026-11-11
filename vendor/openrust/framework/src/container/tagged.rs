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

/// An injectable tag sequence. It borrows its original container when iterated.
pub struct TaggedServices<T: ?Sized> {
    identity: super::scope::Identity,
    tag: Option<Arc<str>>,
    count: usize,
    output: PhantomData<fn() -> T>,
}

impl<T: ?Sized + Send + Sync + 'static> TaggedServices<T> {
    pub(super) fn new(container: &super::Container, name: &str) -> Self {
        let (tag, count) = container.tagged_identity(name);
        Self {
            identity: container.identity.clone(),
            tag,
            count,
            output: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter<'a>(
        &self,
        container: &'a super::Container,
    ) -> Result<TaggedIter<'a, super::Container, T>, BindingResolutionError> {
        Ok(self.borrow(container)?.iter())
    }

    #[allow(non_snake_case, reason = "Async lazy tagged resolution")]
    pub async fn collectAsync(
        &self,
        container: &super::Container,
    ) -> Result<Vec<Arc<T>>, BindingResolutionError> {
        self.borrow(container)?.collectAsync().await
    }

    fn borrow<'a>(
        &self,
        container: &'a super::Container,
    ) -> Result<Tagged<'a, super::Container, T>, BindingResolutionError> {
        if self.identity != container.identity {
            return Err(BindingResolutionError::TaggedContainerMismatch);
        }
        super::scope::check_open(container)?;
        Ok(container.tagged_sequence(self.tag.clone(), self.count))
    }
}

impl super::Container {
    pub(super) fn tagged_identity(&self, name: &str) -> (Option<Arc<str>>, usize) {
        self.registry
            .read()
            .expect("container lock poisoned")
            .tags
            .get(name)
            .map_or((None, 0), |keys| (Some(Arc::from(name)), keys.len()))
    }

    pub(super) fn tagged_sequence<T: ?Sized + Send + Sync + 'static>(
        &self,
        tag: Option<Arc<str>>,
        count: usize,
    ) -> Tagged<'_, Self, T> {
        Tagged::new(self, count, move || {
            tag.as_deref()
                .and_then(|tag| {
                    self.registry
                        .read()
                        .expect("container lock poisoned")
                        .tags
                        .get(tag)
                        .cloned()
                })
                .unwrap_or_default()
        })
    }
}
