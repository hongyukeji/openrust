use super::{ArrayKey, CollectionError, CollectionResult, LazyCollection};

impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn pipe<U>(self, callback: impl FnOnce(Self) -> U) -> U {
        callback(self)
    }

    pub fn pipe_into<U: From<Self>>(self) -> U {
        self.into()
    }

    pub fn pipe_through<F: FnOnce(Self) -> Self>(
        self,
        stages: impl IntoIterator<Item = F>,
    ) -> Self {
        stages.into_iter().fold(self, |items, stage| stage(items))
    }

    pub fn tap(self, callback: impl FnOnce(&Self)) -> Self {
        callback(&self);
        self
    }

    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when_else(condition, callback, std::convert::identity)
    }

    pub fn when_else(
        self,
        condition: bool,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> Self {
        if condition {
            callback(self)
        } else {
            otherwise(self)
        }
    }

    pub fn unless(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, callback)
    }

    pub fn unless_else(
        self,
        condition: bool,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> Self {
        self.when_else(!condition, callback, otherwise)
    }

    pub fn when_empty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.when_empty_else(callback, std::convert::identity)
    }

    pub fn when_empty_else(
        self,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> CollectionResult<Self> {
        let empty = self.is_empty()?;
        Ok(self.when_else(empty, callback, otherwise))
    }

    pub fn when_not_empty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.when_not_empty_else(callback, std::convert::identity)
    }

    pub fn when_not_empty_else(
        self,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> CollectionResult<Self> {
        self.when_empty_else(otherwise, callback)
    }

    pub fn unless_empty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.when_not_empty(callback)
    }

    pub fn unless_not_empty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.when_empty(callback)
    }

    pub fn ensure(self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<Self> {
        self.every(predicate)?
            .then_some(self)
            .ok_or(CollectionError::InvalidArgument)
    }
}
