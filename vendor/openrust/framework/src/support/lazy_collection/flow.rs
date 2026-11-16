use super::{ArrayKey, CollectionError, CollectionResult, LazyCollection};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: 'source> LazyCollection<'source, T> {
    pub fn pipe<U>(self, callback: impl FnOnce(Self) -> U) -> U {
        callback(self)
    }

    pub fn pipeInto<U: From<Self>>(self) -> U {
        self.into()
    }

    pub fn pipeThrough<F: FnOnce(Self) -> Self>(self, stages: impl IntoIterator<Item = F>) -> Self {
        stages.into_iter().fold(self, |items, stage| stage(items))
    }

    pub fn tap(self, callback: impl FnOnce(&Self)) -> Self {
        callback(&self);
        self
    }

    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.whenElse(condition, callback, std::convert::identity)
    }

    pub fn whenElse(
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

    pub fn unlessElse(
        self,
        condition: bool,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> Self {
        self.whenElse(!condition, callback, otherwise)
    }

    pub fn whenEmpty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.whenEmptyElse(callback, std::convert::identity)
    }

    pub fn whenEmptyElse(
        self,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> CollectionResult<Self> {
        let empty = self.isEmpty()?;
        Ok(self.whenElse(empty, callback, otherwise))
    }

    pub fn whenNotEmpty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.whenNotEmptyElse(callback, std::convert::identity)
    }

    pub fn whenNotEmptyElse(
        self,
        callback: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> CollectionResult<Self> {
        self.whenEmptyElse(otherwise, callback)
    }

    pub fn unlessEmpty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.whenNotEmpty(callback)
    }

    pub fn unlessNotEmpty(self, callback: impl FnOnce(Self) -> Self) -> CollectionResult<Self> {
        self.whenEmpty(callback)
    }

    pub fn ensure(self, predicate: impl FnMut(&T, &ArrayKey) -> bool) -> CollectionResult<Self> {
        self.every(predicate)?
            .then_some(self)
            .ok_or(CollectionError::InvalidArgument)
    }
}
